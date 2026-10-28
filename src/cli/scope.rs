//! Finding repositories, scanning what they could share, capturing and
//! merging: everything the capture menus do, as commands Claude can drive.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};
use sync_optimized_rules::capture::{self, Options};
use sync_optimized_rules::config::{Config, DEFAULT_DEPTH};
use sync_optimized_rules::error::{IoContext, Result};
use sync_optimized_rules::{discover, engine};

use super::{Flags, canonical, render};

/// Handles the command when it is one of these; false otherwise.
pub(super) fn run(config: &mut Config, w: &[&str], f: &Flags) -> Result<bool> {
    match w {
        ["find", folder] => find(config, &canonical(folder)?, f),
        ["scan", repos @ ..] if !repos.is_empty() => {
            let repos = repos
                .iter()
                .map(|r| canonical(r))
                .collect::<Result<Vec<_>>>()?;
            scan(config, &repos, f.json)
        }
        ["scope", "list"] => render::scopes(config, None, f.json),
        ["scope", "list", group] => render::scopes(config, Some(group), f.json),
        ["scope", "capture", group, scope, paths @ ..] if !paths.is_empty() => {
            let from = if f.from.is_empty() {
                vec![PathBuf::from(".")]
            } else {
                f.from.clone()
            };
            let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
            let opts = Options {
                description: f.description.as_deref(),
                permissions: f.permissions,
                move_originals: f.move_originals,
            };
            let report = capture::capture(config, group, scope, &from, &paths, &opts)?;
            config.save()?;
            render::capture(&report, f.json);
            Ok(())
        }
        ["scope", "merge", group, scope, path, versions @ ..] if !versions.is_empty() => {
            let names: Vec<String> = versions.iter().map(|v| (*v).to_owned()).collect();
            match &f.file {
                Some(file) => {
                    let text = fs::read_to_string(file).at(file)?;
                    capture::merge_with(config, group, scope, path, &names, &text)?;
                }
                None => capture::merge(config, group, scope, path, &names)?,
            }
            render::done(
                f.json,
                json!({ "group": group, "scope": scope, "merged": path, "versions": names }),
                &format!(
                    "{group}/{scope}: {} merged into {path} (run sor sync)",
                    names.join(", ")
                ),
            );
            Ok(())
        }
        ["diff", repo, file] => {
            let repo = canonical(repo)?;
            let group = engine::group_copy(config, &repo, file)?;
            println!("--- the group's version: {}", group.display());
            println!("+++ yours: {}", repo.join(file).display());
            Command::new("git")
                .args(["--no-pager", "diff", "--no-index", "--no-color", "--"])
                .arg(&group)
                .arg(repo.join(file))
                .status()
                .at("git")?;
            Ok(())
        }
        _ => return Ok(false),
    }
    .map(|()| true)
}

fn find(config: &Config, folder: &Path, f: &Flags) -> Result<()> {
    let mut groups: Vec<(&String, Vec<PathBuf>)> = Vec::new();
    for name in config.groups.keys() {
        groups.push((name, config.repos(name)?));
    }
    let mut rows: Vec<Value> = Vec::new();
    for repo in discover::repos(folder, f.depth.unwrap_or(DEFAULT_DEPTH)) {
        let files = if repo.join(".claude").is_dir() {
            capture::candidates(config, &repo)?.len()
        } else {
            0
        };
        let member: Vec<&String> = groups
            .iter()
            .filter(|(_, repos)| repos.contains(&repo))
            .map(|(g, _)| *g)
            .collect();
        rows.push(
            json!({ "path": repo, "name": file_name(&repo), "files": files, "groups": member }),
        );
    }
    if f.json {
        println!("{}", Value::Array(rows));
        return Ok(());
    }
    if rows.is_empty() {
        println!("No git repository in {}", folder.display());
    }
    for r in &rows {
        let mut hint = format!("{} files in .claude/", r["files"]);
        if let Some(g) = r["groups"].as_array().filter(|g| !g.is_empty()) {
            let names: Vec<&str> = g.iter().filter_map(Value::as_str).collect();
            hint.push_str(&format!(", in {}", names.join(", ")));
        }
        println!("{}  ({hint})", r["path"].as_str().unwrap_or_default());
    }
    Ok(())
}

fn scan(config: &Config, repos: &[PathBuf], json: bool) -> Result<()> {
    let found = capture::gather(config, repos)?;
    let mut permissions: Vec<Value> = Vec::new();
    for repo in repos {
        let [allow, deny, ask] = capture::permission_counts(repo)?;
        permissions.push(json!({ "repo": repo, "allow": allow, "deny": deny, "ask": ask }));
    }
    if json {
        println!("{}", json!({ "files": found, "permissions": permissions }));
        return Ok(());
    }
    if found.is_empty() {
        println!("Nothing to capture in their .claude/ folders");
    }
    for f in &found {
        let from: Vec<String> = f.repos.iter().map(|r| file_name(r)).collect();
        let hook = if f.hooked { "  (its hook follows)" } else { "" };
        println!(".claude/{}  from {}{hook}", f.name, from.join(", "));
    }
    if found.iter().any(capture::Found::is_version) {
        println!(
            "Files named <repository>-<file> differ between repositories: capture one, or all and merge them"
        );
    }
    for p in &permissions {
        println!(
            "{}: {} allow, {} deny, {} ask",
            p["repo"].as_str().unwrap_or_default(),
            p["allow"],
            p["deny"],
            p["ask"]
        );
    }
    Ok(())
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}
