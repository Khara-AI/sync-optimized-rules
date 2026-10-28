//! Merging the versions of one file a capture kept side by side, with Claude
//! Code when it is installed.

use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::config::Config;
use crate::error::{Error, IoContext, Result};
use crate::fsutil;
use crate::store::{Area, Store};

/// The `claude` command, when it is on the PATH.
pub fn claude() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["claude.exe", "claude.cmd"]
    } else {
        &["claude"]
    };
    env::split_paths(&env::var_os("PATH")?)
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|p| p.is_file())
}

/// Ask Claude to merge `names` into `path`, then replace them with the result.
pub fn merge(
    config: &Config,
    group: &str,
    scope: &str,
    path: &str,
    names: &[String],
) -> Result<()> {
    let claude = claude().ok_or_else(|| Error::invalid("claude was not found in PATH"))?;
    let store = config.store();
    let mut prompt = format!(
        "Merge these versions of the file `{path}` into one file. Keep every rule, \
         instruction and behaviour of each version, say each thing once, and keep the \
         language, tone and format of the originals. Reply with the merged file only: \
         no explanation, no code fence.\n"
    );
    for name in names {
        let file = scope_file(&store, group, scope, name);
        let text = fs::read_to_string(&file).at(&file)?;
        prompt.push_str(&format!("\n=== version {name} ===\n{text}\n=== end ===\n"));
    }
    let mut child = Command::new(&claude)
        .args(["-p", "--tools", "", "--no-session-persistence"])
        .current_dir(store.dir(group, scope))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .at(&claude)?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(prompt.as_bytes()).at(&claude)?;
    }
    let out = child.wait_with_output().at(&claude)?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr);
        return Err(Error::invalid(format!("claude failed: {}", why.trim())));
    }
    let merged = unfence(&String::from_utf8_lossy(&out.stdout));
    if merged.trim().is_empty() {
        return Err(Error::invalid("claude answered nothing"));
    }
    replace(&store, group, scope, path, names, &merged)
}

/// Replace the versions `names` of `path` with `merged`, written elsewhere.
pub fn merge_with(
    config: &Config,
    group: &str,
    scope: &str,
    path: &str,
    names: &[String],
    merged: &str,
) -> Result<()> {
    let store = config.store();
    for name in names {
        let file = scope_file(&store, group, scope, name);
        if !file.is_file() {
            return Err(Error::invalid(format!("{group}/{scope} has no {name}")));
        }
    }
    if merged.trim().is_empty() {
        return Err(Error::invalid("the merged file is empty"));
    }
    replace(&store, group, scope, path, names, merged)
}

/// Write the merged file, archive the versions and point their hooks at it.
fn replace(
    store: &Store,
    group: &str,
    scope: &str,
    path: &str,
    names: &[String],
    merged: &str,
) -> Result<()> {
    let dest = scope_file(store, group, scope, path);
    if let Some(first) = names.first() {
        let src = scope_file(store, group, scope, first);
        if dest != src {
            fsutil::copy_atomic(&src, &dest)?;
        }
    }
    let mut text = merged.trim_end().to_owned();
    text.push('\n');
    let perms = fs::metadata(&dest).ok().map(|m| m.permissions());
    fsutil::write_atomic(&dest, text.as_bytes())?;
    if let Some(p) = perms {
        fs::set_permissions(&dest, p).at(&dest)?;
    }
    for name in names.iter().filter(|n| *n != path) {
        let (area, inner) = Area::of(name);
        let rel = format!("{}/{inner}", area.dir());
        store.archive(group, scope, &rel)?;
        let file = store.dir(group, scope).join(&rel);
        fs::remove_file(&file).at(&file)?;
        fsutil::prune_empty(&store.dir(group, scope), &file);
    }

    let mut meta = store.meta(group, scope)?;
    let mut json = Value::Object(std::mem::take(&mut meta.settings)).to_string();
    for name in names {
        json = json.replace(
            &format!("{{{{dir}}}}/{name}"),
            &format!("{{{{dir}}}}/{path}"),
        );
    }
    let mut settings: Value =
        serde_json::from_str(&json).map_err(|e| Error::invalid(e.to_string()))?;
    if let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) {
        for list in hooks.values_mut().filter_map(Value::as_array_mut) {
            let mut seen: Vec<Value> = Vec::new();
            list.retain(|v| {
                let new = !seen.contains(v);
                seen.push(v.clone());
                new
            });
        }
    }
    if let Value::Object(map) = settings {
        meta.settings = map;
    }
    store.save_meta(group, scope, &meta)
}

fn scope_file(store: &Store, group: &str, scope: &str, name: &str) -> PathBuf {
    let (area, inner) = Area::of(name);
    store.dir(group, scope).join(area.dir()).join(inner)
}

/// Drop the code fence a model may wrap a whole file in.
fn unfence(text: &str) -> String {
    let t = text.trim();
    match (t.strip_prefix("```"), t.ends_with("```")) {
        (Some(rest), true) if t.len() > 6 => {
            let body = rest.split_once('\n').map_or("", |(_, b)| b);
            body.strip_suffix("```").unwrap_or(body).to_owned()
        }
        _ => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Meta;
    use crate::testutil::TempDir;
    use serde_json::json;

    #[test]
    fn a_merge_replaces_the_versions_and_their_hooks() {
        let tmp = TempDir::new("merge");
        let store = Store::new(tmp.path());
        let src = tmp.path().join("src");
        fs::write(&src, "x").unwrap();
        for name in ["hooks/app-guard.py", "hooks/lib-guard.py"] {
            store.put("g", "s", Area::Files, name, &src).unwrap();
        }
        let hook = |f: &str| {
            json!({ "matcher": "Bash", "hooks": [{ "type": "command",
                "command": format!("python3 \"{{{{dir}}}}/{f}\"") }] })
        };
        let mut meta = Meta::default();
        meta.settings.insert(
            "hooks".into(),
            json!({ "PreToolUse": [hook("hooks/app-guard.py"), hook("hooks/lib-guard.py")] }),
        );
        store.save_meta("g", "s", &meta).unwrap();

        let names = [
            "hooks/app-guard.py".to_owned(),
            "hooks/lib-guard.py".to_owned(),
        ];
        replace(&store, "g", "s", "hooks/guard.py", &names, "merged").unwrap();

        let files: Vec<String> = store.files("g", "s").into_iter().map(|f| f.rel).collect();
        assert_eq!(files, ["hooks/guard.py"]);
        let meta = store.meta("g", "s").unwrap();
        assert_eq!(
            meta.settings["hooks"]["PreToolUse"],
            json!([hook("hooks/guard.py")])
        );
        assert_eq!(unfence("```md\nbody\n```"), "body\n");
    }
}
