//! Turning the files a repository already has under `.claude/` into a scope,
//! and moving those originals aside once sor installs them.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::config::{Config, check_name};
use crate::error::{Error, IoContext, Result};
use crate::fsutil;
use crate::settings::{self, Item};
use crate::store::{Area, Meta};

mod tidy;

pub use tidy::{Tidy, tidy};

const PROJECT_SETTINGS: &str = ".claude/settings.json";
const PERMISSIONS: [&str; 3] = ["allow", "deny", "ask"];

#[derive(Debug, Default, Serialize)]
pub struct Capture {
    pub group: String,
    pub scope: String,
    pub files: Vec<String>,
    pub hooks: Vec<String>,
    pub permissions: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tidy: Option<Tidy>,
}

pub struct Options<'a> {
    pub description: Option<&'a str>,
    pub permissions: bool,
    pub move_originals: bool,
}

/// Copy `paths` (under `repo/.claude/`) into the scope and share it.
pub fn capture(
    config: &mut Config,
    group: &str,
    scope: &str,
    repo: &Path,
    paths: &[PathBuf],
    opts: &Options,
) -> Result<Capture> {
    check_name("scope", scope)?;
    config.group(group)?;
    let repo = fs::canonicalize(repo).at(repo)?;
    let files = select(config, &repo, paths)?;
    let store = config.store();
    for rel in &files {
        let (area, inner) = Area::of(rel);
        store.put(group, scope, area, inner, &repo.join(".claude").join(rel))?;
    }

    let mut meta = if store.exists(group, scope) {
        store.meta(group, scope)?
    } else {
        Meta::default()
    };
    if let Some(d) = opts.description {
        meta.description = d.to_owned();
    }
    let project = fsutil::read_json(&repo.join(PROJECT_SETTINGS))?.unwrap_or(Value::Null);
    let mut items: Vec<Item> = hook_entries(&project, &files)
        .into_iter()
        .map(|(event, _, rewritten)| Item {
            path: vec!["hooks".into(), event],
            value: rewritten,
            set: false,
        })
        .collect();
    let hooks: Vec<String> = items.iter().map(|i| i.path[1].clone()).collect();
    let before = items.len();
    if opts.permissions {
        for key in PERMISSIONS {
            if let Some(list) = project["permissions"][key].as_array() {
                items.extend(list.iter().map(|v| Item {
                    path: vec!["permissions".into(), key.into()],
                    value: v.clone(),
                    set: false,
                }));
            }
        }
    }
    let permissions = items.len() - before;
    let mut doc = Value::Object(std::mem::take(&mut meta.settings));
    for item in &items {
        settings::add(&mut doc, item);
    }
    if let Value::Object(map) = doc {
        meta.settings = map;
    }
    store.save_meta(group, scope, &meta)?;
    config.share(group, &[scope.to_owned()])?;

    let tidy = if opts.move_originals {
        Some(tidy(config, &repo, paths, true)?)
    } else {
        None
    };
    Ok(Capture {
        group: group.to_owned(),
        scope: scope.to_owned(),
        files,
        hooks,
        permissions,
        tidy,
    })
}

/// A file of `.claude/` a scope could take.
#[derive(Debug, Serialize)]
pub struct Candidate {
    /// Relative to `.claude/`.
    pub path: String,
    /// A hook of `.claude/settings.json` runs it, and will follow it.
    pub hooked: bool,
}

/// What `repo/.claude/` holds that a scope could capture.
pub fn candidates(config: &Config, repo: &Path) -> Result<Vec<Candidate>> {
    let project = fsutil::read_json(&repo.join(PROJECT_SETTINGS))?.unwrap_or(Value::Null);
    Ok(fsutil::files_under(&repo.join(".claude"))
        .into_iter()
        .filter(|f| !owned_elsewhere(config, f))
        .map(|path| Candidate {
            hooked: !hook_entries(&project, std::slice::from_ref(&path)).is_empty(),
            path,
        })
        .collect())
}

/// Allow, deny and ask entries of `repo/.claude/settings.json`.
pub fn permission_count(repo: &Path) -> Result<usize> {
    let project = fsutil::read_json(&repo.join(PROJECT_SETTINGS))?.unwrap_or(Value::Null);
    Ok(PERMISSIONS
        .iter()
        .filter_map(|k| project["permissions"][k].as_array())
        .map(Vec::len)
        .sum())
}

/// Files named by `paths`, relative to `repo/.claude/`. Folders expand to
/// their files; sor's own files, settings and CLAUDE.md are never taken.
fn select(config: &Config, repo: &Path, paths: &[PathBuf]) -> Result<Vec<String>> {
    let claude = repo.join(".claude");
    let mut out: Vec<String> = Vec::new();
    for p in paths {
        let abs = if p.is_absolute() {
            p.clone()
        } else {
            repo.join(p)
        };
        let abs = fs::canonicalize(&abs).at(&abs)?;
        let rel = abs.strip_prefix(&claude).map(fsutil::slash).map_err(|_| {
            Error::invalid(format!(
                "{} is not under {}",
                abs.display(),
                claude.display()
            ))
        })?;
        let found: Vec<String> = if abs.is_dir() {
            fsutil::files_under(&abs)
                .into_iter()
                .map(|f| {
                    if rel.is_empty() {
                        f
                    } else {
                        format!("{rel}/{f}")
                    }
                })
                .filter(|f| !owned_elsewhere(config, f))
                .collect()
        } else if owned_elsewhere(config, &rel) {
            return Err(Error::invalid(format!(
                ".claude/{rel} cannot be captured: sor manages it, or it holds settings or CLAUDE.md"
            )));
        } else {
            vec![rel]
        };
        for f in found {
            if !out.contains(&f) {
                out.push(f);
            }
        }
    }
    if out.is_empty() {
        return Err(Error::invalid("nothing to capture"));
    }
    Ok(out)
}

fn owned_elsewhere(config: &Config, rel: &str) -> bool {
    matches!(rel, "settings.json" | "settings.local.json" | "CLAUDE.md")
        || rel.starts_with("sor/")
        || config.groups.keys().any(|g| {
            rel.starts_with(&format!("rules/{g}/")) || rel.starts_with(&format!("agents/{g}/"))
        })
}

/// Hook entries whose command runs one of `files`, as (event, entry, entry
/// with the path rewritten to the scope's installed folder).
fn hook_entries(project: &Value, files: &[String]) -> Vec<(String, Value, Value)> {
    let Some(hooks) = project["hooks"].as_object() else {
        return Vec::new();
    };
    let files: Vec<&String> = files
        .iter()
        .filter(|f| Area::of(f).0 == Area::Files)
        .collect();
    let mut out = Vec::new();
    for (event, list) in hooks {
        for entry in list.as_array().into_iter().flatten() {
            let text = entry.to_string();
            let mut rewritten = text.clone();
            for f in &files {
                for var in ["$CLAUDE_PROJECT_DIR", "${CLAUDE_PROJECT_DIR}"] {
                    rewritten = rewritten
                        .replace(&format!("{var}/.claude/{f}"), &format!("{{{{dir}}}}/{f}"));
                }
            }
            if rewritten != text {
                if let Ok(v) = serde_json::from_str(&rewritten) {
                    out.push((event.clone(), entry.clone(), v));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;
    use serde_json::json;

    #[test]
    fn candidates_leave_out_settings_and_sor_files() {
        let tmp = TempDir::new("candidates");
        let repo = tmp.path().join("app");
        let settings = json!({ "hooks": { "Stop": [{ "hooks": [{ "type": "command",
            "command": "sh $CLAUDE_PROJECT_DIR/.claude/hooks/end.sh" }] }] } });
        for (rel, text) in [
            ("rules/git.md", String::new()),
            ("hooks/end.sh", String::new()),
            ("CLAUDE.md", String::new()),
            ("sor/state.json", String::new()),
            ("settings.json", settings.to_string()),
        ] {
            let path = repo.join(".claude").join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let config = Config::load(&tmp.path().join("home")).unwrap();
        let found: Vec<(String, bool)> = candidates(&config, &repo)
            .unwrap()
            .into_iter()
            .map(|c| (c.path, c.hooked))
            .collect();
        assert_eq!(
            found,
            [
                ("hooks/end.sh".into(), true),
                ("rules/git.md".into(), false)
            ]
        );
    }

    #[test]
    fn hook_paths_are_rewritten_to_the_scope_folder() {
        let project = json!({ "hooks": { "PreToolUse": [
            { "matcher": "Bash", "hooks": [{ "type": "command",
              "command": "python3 \"$CLAUDE_PROJECT_DIR/.claude/hooks/guard.py\"" }] },
            { "matcher": "Edit", "hooks": [{ "type": "command", "command": "other" }] }
        ] } });
        let found = hook_entries(&project, &["hooks/guard.py".into()]);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].2["hooks"][0]["command"],
            "python3 \"{{dir}}/hooks/guard.py\""
        );
    }
}
