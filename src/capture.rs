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

mod gather;
mod hooks;
mod merge;
mod select;
mod tidy;

use hooks::hook_entries;
use select::owned_elsewhere;

pub use gather::{Found, gather, scope_names};
pub use merge::{claude, merge, merge_with};
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

/// Copy `paths` (under `.claude/` of `repos`) into the scope and share it.
/// From several repositories, versions that differ keep their repository's
/// name until merged; a path names all its versions, a version name just one.
pub fn capture(
    config: &mut Config,
    group: &str,
    scope: &str,
    repos: &[PathBuf],
    paths: &[PathBuf],
    opts: &Options,
) -> Result<Capture> {
    let repos = repos
        .iter()
        .map(|r| fs::canonicalize(r).at(r))
        .collect::<Result<Vec<_>>>()?;
    let found: Vec<Found> = match repos.as_slice() {
        [] => return Err(Error::invalid("capture from which repository?")),
        [repo] => select::files(config, repo, paths)?
            .into_iter()
            .map(|path| Found {
                name: path.clone(),
                path,
                repos: vec![repo.clone()],
                hooked: false,
            })
            .collect(),
        _ if opts.move_originals => {
            return Err(Error::invalid(
                "--move takes a single --from: run sor tidy in each repository instead",
            ));
        }
        _ => {
            let all = gather(config, &repos)?;
            scope_names(&all, &select::versions(&all, paths)?)
        }
    };
    let permissions: &[PathBuf] = if opts.permissions { &repos } else { &[] };
    let mut report = capture_found(config, group, scope, &found, opts.description, permissions)?;
    if let ([repo], true) = (repos.as_slice(), opts.move_originals) {
        report.tidy = Some(tidy(config, repo, paths, true)?);
    }
    Ok(report)
}

/// Copy gathered files into the scope under their scope names, with the hooks
/// that run them and the permission rules of `permissions_from`, and share it.
pub fn capture_found(
    config: &mut Config,
    group: &str,
    scope: &str,
    found: &[Found],
    description: Option<&str>,
    permissions_from: &[PathBuf],
) -> Result<Capture> {
    check_name("scope", scope)?;
    config.group(group)?;
    if found.is_empty() {
        return Err(Error::invalid("nothing to capture"));
    }
    let store = config.store();
    for f in found {
        let repo = f
            .repos
            .first()
            .ok_or_else(|| Error::invalid(format!("{} comes from no repository", f.path)))?;
        let (area, inner) = Area::of(&f.name);
        store.put(
            group,
            scope,
            area,
            inner,
            &repo.join(".claude").join(&f.path),
        )?;
    }

    let mut meta = if store.exists(group, scope) {
        store.meta(group, scope)?
    } else {
        Meta::default()
    };
    if let Some(d) = description {
        meta.description = d.to_owned();
    }
    let mut repos: Vec<&PathBuf> = found.iter().flat_map(|f| &f.repos).collect();
    repos.extend(permissions_from);
    repos.sort();
    repos.dedup();
    let mut items: Vec<Item> = Vec::new();
    let mut hooks: Vec<String> = Vec::new();
    let mut permissions = 0;
    for repo in repos {
        let project = fsutil::read_json(&repo.join(PROJECT_SETTINGS))?.unwrap_or(Value::Null);
        let files: Vec<(&str, &str)> = found
            .iter()
            .filter(|f| f.repos.contains(repo))
            .map(|f| (f.path.as_str(), f.name.as_str()))
            .collect();
        for (event, _, rewritten) in hook_entries(&project, &files) {
            if !hooks.contains(&event) {
                hooks.push(event.clone());
            }
            items.push(Item {
                path: vec!["hooks".into(), event],
                value: rewritten,
                set: false,
            });
        }
        if !permissions_from.contains(repo) {
            continue;
        }
        for key in PERMISSIONS {
            for v in project["permissions"][key].as_array().into_iter().flatten() {
                permissions += 1;
                items.push(Item {
                    path: vec!["permissions".into(), key.into()],
                    value: v.clone(),
                    set: false,
                });
            }
        }
    }
    let mut doc = Value::Object(std::mem::take(&mut meta.settings));
    for item in &items {
        settings::add(&mut doc, item);
    }
    if let Value::Object(map) = doc {
        meta.settings = map;
    }
    store.save_meta(group, scope, &meta)?;
    config.share(group, &[scope.to_owned()])?;
    Ok(Capture {
        group: group.to_owned(),
        scope: scope.to_owned(),
        files: found.iter().map(|f| f.name.clone()).collect(),
        hooks,
        permissions,
        tidy: None,
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
            hooked: !hook_entries(&project, &[(path.as_str(), path.as_str())]).is_empty(),
            path,
        })
        .collect())
}

/// Allow, deny and ask entries of `repo/.claude/settings.json`, in that order.
pub fn permission_counts(repo: &Path) -> Result<[usize; 3]> {
    let project = fsutil::read_json(&repo.join(PROJECT_SETTINGS))?.unwrap_or(Value::Null);
    Ok(PERMISSIONS.map(|k| project["permissions"][k].as_array().map_or(0, Vec::len)))
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
}
