//! Human and JSON renderings of command results.

use std::path::PathBuf;

use serde::Serialize;
use serde_json::{Value, json};
use sync_optimized_rules::capture::{Capture, Tidy};
use sync_optimized_rules::config::Config;
use sync_optimized_rules::engine::{FileState, Report};
use sync_optimized_rules::error::Result;

fn print_json(value: &impl Serialize) {
    println!("{}", json!(value));
}

pub fn done(json: bool, value: Value, human: &str) {
    if json {
        println!("{value}");
    } else {
        println!("{human}");
    }
}

pub fn paths(list: &[PathBuf], json: bool) {
    if json {
        print_json(&list);
    } else {
        list.iter().for_each(|p| println!("{}", p.display()));
    }
}

pub fn groups(config: &Config, json: bool) {
    if json {
        let groups: Vec<_> = config
            .groups
            .iter()
            .map(|(name, g)| {
                json!({
                    "name": name,
                    "description": g.description,
                    "members": g.members,
                    "scopes": g.scopes,
                    "agents": g.agents,
                    "exclude": g.exclude,
                    "imports": g.imports,
                })
            })
            .collect();
        print_json(&groups);
        return;
    }
    if config.groups.is_empty() {
        println!("No group yet. Create one with: sor group add <name>");
        return;
    }
    for (name, g) in &config.groups {
        let agents = if g.agents { ", agents on" } else { "" };
        println!(
            "{name}  ({} members{agents})  {}",
            g.members.len(),
            g.description
        );
        if !g.scopes.is_empty() {
            println!("  shares   {}", g.scopes.join(", "));
        }
        for (from, list) in &g.imports {
            println!("  imports  {} from {from}", list.join(", "));
        }
        for (repo, list) in &g.exclude {
            println!("  excludes {} in {}", list.join(", "), repo.display());
        }
    }
}

pub fn scopes(config: &Config, only: Option<&str>, json: bool) -> Result<()> {
    let store = config.store();
    let mut rows = Vec::new();
    for (name, g) in &config.groups {
        if only.is_some_and(|o| o != name) {
            continue;
        }
        if only.is_some() {
            config.group(name)?;
        }
        for scope in store.scopes(name) {
            let meta = store.meta(name, &scope)?;
            rows.push(json!({
                "group": name,
                "scope": scope,
                "description": meta.description,
                "files": store.files(name, &scope).len(),
                "shared": g.scopes.contains(&scope),
            }));
        }
    }
    if let Some(o) = only {
        config.group(o)?;
    }
    if json {
        print_json(&rows);
        return Ok(());
    }
    if rows.is_empty() {
        println!("No scope yet. Capture one with: sor scope capture <group> <scope> <path>...");
    }
    for r in rows {
        let shared = if r["shared"] == true {
            ""
        } else {
            "  (not shared)"
        };
        println!(
            "{}/{}  {} files{shared}  {}",
            r["group"].as_str().unwrap_or_default(),
            r["scope"].as_str().unwrap_or_default(),
            r["files"],
            r["description"].as_str().unwrap_or_default()
        );
    }
    Ok(())
}

pub fn capture(c: &Capture, json: bool) {
    if json {
        print_json(c);
        return;
    }
    println!("{}/{}: {} files captured", c.group, c.scope, c.files.len());
    c.files.iter().for_each(|f| println!("  .claude/{f}"));
    if !c.hooks.is_empty() {
        println!("  hooks for {} follow the files", c.hooks.join(", "));
    }
    if c.permissions > 0 {
        println!("  {} permission entries", c.permissions);
    }
    match &c.tidy {
        Some(t) => tidy(t, false),
        None => println!("Shared in '{}'. Run sor status, then sor sync.", c.group),
    }
}

pub fn tidy(t: &Tidy, json: bool) {
    if json {
        print_json(t);
        return;
    }
    let verb = if t.applied { "moved" } else { "would move" };
    for f in &t.moved {
        println!("{verb} {f} to .claude/sor/backup/");
    }
    if !t.hooks.is_empty() {
        let verb = if t.applied { "removed" } else { "would remove" };
        println!(
            "{verb} {} hook entries from .claude/settings.json (a copy goes to backup/)",
            t.hooks.join(", ")
        );
    }
}

pub fn reports(reports: &[Report], json: bool) {
    if json {
        print_json(&reports);
        return;
    }
    if reports.is_empty() {
        println!("No repository in any group yet.");
    }
    for r in reports {
        report(r);
    }
}

fn report(r: &Report) {
    let scopes = if r.scopes.is_empty() {
        "no scope".to_owned()
    } else {
        r.scopes.join(", ")
    };
    println!("{}  [{scopes}]", r.repo.display());
    let ok = r
        .files
        .iter()
        .filter(|f| matches!(f.state, FileState::Ok | FileState::Kept))
        .count();
    for f in &r.files {
        let word = match (f.state, r.applied) {
            (FileState::Ok | FileState::Kept, _) => continue,
            (FileState::Missing, true) => "installed",
            (FileState::Outdated, true) => "updated",
            (FileState::Stale, true) => "trashed",
            (s, _) => s.as_str(),
        };
        println!("  {word:<9} {}", f.path);
    }
    let (add, del) = if r.applied {
        ("added", "removed")
    } else {
        ("missing", "stale")
    };
    for (word, list) in [(add, &r.settings.missing), (del, &r.settings.removed)] {
        if !list.is_empty() {
            println!("  {word:<9} settings {}", by_key(list));
        }
    }
    for s in &r.settings.clash {
        println!("  clash     setting {s} (you set another value; sor leaves it)");
    }
    if r.exclude {
        let word = if r.applied { "updated" } else { "outdated" };
        println!("  {word:<9} .git/info/exclude block");
    }
    if r.clean {
        println!("  up to date ({ok} files)");
    } else if r.pending() {
        println!(
            "  resolve with: sor resolve {} <file> keep|take|promote",
            r.repo.display()
        );
    }
}

/// "permissions.allow ×10, hooks.PreToolUse" from "<key> <value>" entries.
fn by_key(entries: &[String]) -> String {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for e in entries {
        let key = e.split_once(' ').map_or(e.as_str(), |(k, _)| k);
        match counts.iter_mut().find(|(k, _)| *k == key) {
            Some((_, n)) => *n += 1,
            None => counts.push((key, 1)),
        }
    }
    counts
        .iter()
        .map(|(k, n)| {
            if *n > 1 {
                format!("{k} ×{n}")
            } else {
                (*k).to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
