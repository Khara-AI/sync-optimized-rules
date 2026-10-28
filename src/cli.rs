//! Command-line interface: parse arguments, call the library, print results.
//!
//! Every command that reports something accepts `--json`; the human output
//! and the JSON are two renderings of the same values.

use std::ffi::OsString;
use std::path::PathBuf;

use lexopt::prelude::*;
use serde_json::json;
use sync_optimized_rules::config::Config;
use sync_optimized_rules::error::{Error, Result};
use sync_optimized_rules::paths;

const HELP: &str = "\
sor - keep Claude Code rules in sync across repositories

Usage:
  sor group list                         list groups
  sor group add <name> [--description <text>]
  sor group remove <name>                forget a group (its files stay on disk)
  sor member add <group> <path> [--workspace] [--depth <n>]
  sor member remove <group> <path>
  sor repos <group>                      repositories of a group, workspaces expanded

Options:
  --json        machine-readable output
  -h, --help    show this help
  -V, --version show the version

Configuration: $SOR_HOME, else ~/.config/sync-optimized-rules.";

pub fn run(args: impl IntoIterator<Item = OsString>) -> Result<()> {
    let mut parser = lexopt::Parser::from_iter(std::iter::once(OsString::from("sor")).chain(args));
    let mut words: Vec<String> = Vec::new();
    let mut json = false;
    let mut workspace = false;
    let mut depth = None;
    let mut description = String::new();

    while let Some(arg) = parser.next().map_err(usage)? {
        match arg {
            Long("json") => json = true,
            Long("workspace") => workspace = true,
            Long("depth") => depth = Some(parser.value().map_err(usage)?.parse().map_err(usage)?),
            Long("description") => {
                description = parser.value().map_err(usage)?.string().map_err(usage)?
            }
            Short('h') | Long("help") => {
                println!("{HELP}");
                return Ok(());
            }
            Short('V') | Long("version") => {
                println!("sor {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            Value(v) => words.push(v.string().map_err(usage)?),
            _ => return Err(usage(arg.unexpected())),
        }
    }

    let mut config = Config::load(&paths::config_home()?)?;
    let w: Vec<&str> = words.iter().map(String::as_str).collect();
    match w.as_slice() {
        [] => {
            println!("{HELP}");
            Ok(())
        }
        ["group", "list"] => {
            group_list(&config, json);
            Ok(())
        }
        ["group", "add", name] => {
            config.add_group(name, &description)?;
            config.save()?;
            done(
                json,
                json!({ "added": name }),
                &format!("group '{name}' created"),
            );
            Ok(())
        }
        ["group", "remove", name] => {
            config.remove_group(name)?;
            config.save()?;
            done(
                json,
                json!({ "removed": name }),
                &format!("group '{name}' forgotten"),
            );
            Ok(())
        }
        ["member", "add", group, path] => {
            let path = config.add_member(group, &PathBuf::from(path), workspace, depth)?;
            config.save()?;
            let kind = if workspace { "workspace" } else { "repository" };
            done(
                json,
                json!({ "group": group, "added": path, "workspace": workspace }),
                &format!("{kind} {} added to '{group}'", path.display()),
            );
            Ok(())
        }
        ["member", "remove", group, path] => {
            config.remove_member(group, &PathBuf::from(path))?;
            config.save()?;
            done(
                json,
                json!({ "group": group, "removed": path }),
                &format!("{path} removed from '{group}'"),
            );
            Ok(())
        }
        ["repos", group] => {
            let repos = config.repos(group)?;
            if json {
                println!("{}", json!(repos));
            } else {
                repos.iter().for_each(|r| println!("{}", r.display()));
            }
            Ok(())
        }
        _ => Err(Error::invalid(format!(
            "unknown command '{}' (see sor --help)",
            words.join(" ")
        ))),
    }
}

fn group_list(config: &Config, json: bool) {
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
                    "imports": g.imports,
                })
            })
            .collect();
        println!("{}", json!(groups));
        return;
    }
    if config.groups.is_empty() {
        println!("No group yet. Create one with: sor group add <name>");
        return;
    }
    for (name, g) in &config.groups {
        let agents = if g.agents { ", agents shared" } else { "" };
        println!(
            "{name}  ({} members, {} scopes{agents})  {}",
            g.members.len(),
            g.scopes.len(),
            g.description
        );
    }
}

fn done(json: bool, value: serde_json::Value, human: &str) {
    if json {
        println!("{value}");
    } else {
        println!("{human}");
    }
}

fn usage(e: impl std::fmt::Display) -> Error {
    Error::invalid(format!("{e} (see sor --help)"))
}
