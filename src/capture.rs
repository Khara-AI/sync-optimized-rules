//! Turning the files a repository already has under `.claude/` into a scope,
//! and moving those originals aside once sor installs them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use serde_json::Value;

use crate::config::{Config, check_name};
use crate::engine::BACKUP;
use crate::error::{Error, IoContext, Result};
use crate::fsutil;
use crate::settings::{self, Item};
use crate::store::{Area, Meta};

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

#[derive(Debug, Default, Serialize)]
pub struct Tidy {
    pub moved: Vec<String>,
    /// Events whose hook entries left `.claude/settings.json`.
    pub hooks: Vec<String>,
    pub applied: bool,
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

/// Move originals now installed by sor to `backup/`, and drop the hook
/// entries of `.claude/settings.json` that ran them (after a backup).
pub fn tidy(config: &Config, repo: &Path, paths: &[PathBuf], apply: bool) -> Result<Tidy> {
    let repo = fs::canonicalize(repo).at(repo)?;
    let files = select(config, &repo, paths)?;
    let mut touched: Vec<String> = files.iter().map(|f| format!(".claude/{f}")).collect();
    let settings_file = repo.join(PROJECT_SETTINGS);
    let mut project = fsutil::read_json(&settings_file)?.unwrap_or(Value::Null);
    let entries = hook_entries(&project, &files);
    if !entries.is_empty() {
        touched.push(PROJECT_SETTINGS.to_owned());
    }
    refuse_tracked(&repo, &touched)?;

    let report = Tidy {
        moved: touched[..files.len()].to_vec(),
        hooks: entries.iter().map(|(e, _, _)| e.clone()).collect(),
        applied: apply,
    };
    if !apply {
        return Ok(report);
    }
    let stamp = fsutil::stamp();
    for rel in &report.moved {
        fsutil::relocate(&repo, rel, BACKUP, &stamp)?;
        fsutil::relocate_cache(&repo, rel, BACKUP, &stamp)?;
        fsutil::prune_empty(&repo.join(".claude"), &repo.join(rel));
    }
    if !entries.is_empty() {
        let saved = repo.join(BACKUP).join(&stamp).join(PROJECT_SETTINGS);
        fsutil::copy_atomic(&settings_file, &saved)?;
        for (event, original, _) in &entries {
            settings::remove(
                &mut project,
                &Item {
                    path: vec!["hooks".into(), event.clone()],
                    value: original.clone(),
                    set: false,
                },
            );
        }
        fsutil::write_json(&settings_file, &project)?;
    }
    Ok(report)
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

/// sor never moves or edits a file git tracks: that change belongs in a commit.
fn refuse_tracked(repo: &Path, paths: &[String]) -> Result<()> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["ls-files", "--"])
        .args(paths)
        .output()
        .map_err(|e| Error::Io(repo.to_path_buf(), e))?;
    if !out.status.success() {
        return Err(Error::invalid(format!(
            "git could not list the tracked files of {}",
            repo.display()
        )));
    }
    let tracked = String::from_utf8_lossy(&out.stdout);
    if tracked.trim().is_empty() {
        return Ok(());
    }
    Err(Error::invalid(format!(
        "tracked by git, change them with a commit instead: {}",
        tracked.split_whitespace().collect::<Vec<_>>().join(", ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
