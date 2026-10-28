//! Command-line interface: parse arguments, call the library, print results.
//!
//! Every command that reports something accepts `--json`; the human output
//! and the JSON are two renderings of the same values (see `render`).

mod interactive;
mod render;
mod scope;
mod tui;
mod update;

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use lexopt::prelude::*;
use serde_json::json;
use sync_optimized_rules::capture;
use sync_optimized_rules::config::Config;
use sync_optimized_rules::engine::{self, Resolution};
use sync_optimized_rules::error::{Error, IoContext, Result};
use sync_optimized_rules::paths;

/// Exit code when a repository drifted from its groups or has conflicts.
pub const DRIFT: u8 = 2;

const HELP: &str = "\
sor - keep Claude Code rules in sync across repositories

  sor, sor setup                              interactive setup, with keyboard menus

Groups:
  sor group list
  sor group add <name> [--description <text>]
  sor group remove <name>                     forget a group (its files stay on disk)
  sor group share|unshare <group> <scope>...  install a scope in every member, or stop
  sor group agents <group> on|off             also install the scopes' agents
  sor group exclude|include <group> <repo> <scope>...
  sor group import <group> <from-group> <scope>...
  sor member add <group> <path> [--workspace] [--depth <n>]
  sor member remove <group> <path>
  sor repos <group>                           repositories, workspaces expanded

Scopes:
  sor find <folder> [--depth <n>]             git repositories inside, and their groups
  sor scan <repo>...                          what their .claude/ folders could share
  sor scope list [group]
  sor scope capture <group> <scope> <path>... [--from <repo>]... [--description <text>]
                    [--permissions] [--move]
      copy files of <repo>/.claude/ into the scope (hooks that run them follow;
      --permissions also takes allow/deny/ask; --move then tidies the originals).
      With several --from, versions that differ keep their repository's name.
  sor scope merge <group> <scope> <path> <version>... [--file <merged>]
      replace the versions with one file: <merged>, else merged by claude

Repositories (all repositories of all groups when <repo> is omitted):
  sor status [repo]                           exit code 2 on drift or conflict
  sor sync [repo] [--dry-run]
  sor diff <repo> <file>                      the group's version of a conflict against yours
  sor resolve <repo> <file> keep|take|promote
  sor tidy <repo> <path>... [--dry-run]       move originals to .claude/sor/backup/

Maintenance:
  sor update [--dry-run]                      sor to the latest release, and the Claude
                                              Code plugin with its skills when installed

Options:
  --json        machine-readable output
  -h, --help    show this help
  -V, --version show the version

Configuration: $SOR_HOME, else ~/.config/sync-optimized-rules.";

#[derive(Default)]
struct Flags {
    json: bool,
    workspace: bool,
    depth: Option<usize>,
    description: Option<String>,
    from: Vec<PathBuf>,
    file: Option<PathBuf>,
    dry_run: bool,
    permissions: bool,
    move_originals: bool,
}

pub fn run(args: impl IntoIterator<Item = OsString>) -> Result<u8> {
    let mut parser = lexopt::Parser::from_iter(std::iter::once(OsString::from("sor")).chain(args));
    let mut words: Vec<String> = Vec::new();
    let mut f = Flags::default();
    while let Some(arg) = parser.next().map_err(usage)? {
        match arg {
            Long("json") => f.json = true,
            Long("workspace") => f.workspace = true,
            Long("dry-run") => f.dry_run = true,
            Long("permissions") => f.permissions = true,
            Long("move") => f.move_originals = true,
            Long("depth") => f.depth = Some(parser.value().map_err(usage)?.parse().map_err(usage)?),
            Long("description") => {
                f.description = Some(parser.value().map_err(usage)?.string().map_err(usage)?)
            }
            Long("from") => f.from.push(parser.value().map_err(usage)?.into()),
            Long("file") => f.file = Some(parser.value().map_err(usage)?.into()),
            Short('h') | Long("help") => {
                println!("{HELP}");
                return Ok(0);
            }
            Short('V') | Long("version") => {
                println!("sor {}", env!("CARGO_PKG_VERSION"));
                return Ok(0);
            }
            Value(v) => words.push(v.string().map_err(usage)?),
            _ => return Err(usage(arg.unexpected())),
        }
    }

    let mut config = Config::load(&paths::config_home()?)?;
    let w: Vec<&str> = words.iter().map(String::as_str).collect();
    let owned = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let json = f.json;
    if scope::run(&mut config, &w, &f)? {
        return Ok(0);
    }
    match w.as_slice() {
        [] if tui::available() => return interactive::run(&mut config),
        [] => println!("{HELP}"),
        ["setup"] if tui::available() => return interactive::run(&mut config),
        ["setup"] => return Err(Error::invalid("sor setup needs a terminal")),
        ["update"] => update::run(json, f.dry_run)?,
        ["group", "list"] => render::groups(&config, json),
        ["group", "add", name] => {
            config.add_group(name, f.description.as_deref().unwrap_or(""))?;
            config.save()?;
            render::done(
                json,
                json!({ "added": name }),
                &format!("group '{name}' created"),
            );
        }
        ["group", "remove", name] => {
            config.remove_group(name)?;
            config.save()?;
            render::done(
                json,
                json!({ "removed": name }),
                &format!("group '{name}' forgotten"),
            );
        }
        ["group", verb @ ("share" | "unshare"), group, scopes @ ..] if !scopes.is_empty() => {
            let scopes = owned(scopes);
            if *verb == "share" {
                config.share(group, &scopes)?;
            } else {
                config.unshare(group, &scopes)?;
            }
            config.save()?;
            render::done(
                json,
                json!({ "group": group, *verb: scopes }),
                &format!("{verb}d in '{group}': {} (run sor sync)", scopes.join(", ")),
            );
        }
        ["group", "agents", group, state @ ("on" | "off")] => {
            config.set_agents(group, *state == "on")?;
            config.save()?;
            render::done(
                json,
                json!({ "group": group, "agents": *state == "on" }),
                &format!("agents {state} for '{group}' (run sor sync)"),
            );
        }
        [
            "group",
            verb @ ("exclude" | "include"),
            group,
            repo,
            scopes @ ..,
        ] if !scopes.is_empty() => {
            let scopes = owned(scopes);
            let repo = if *verb == "exclude" {
                config.exclude(group, Path::new(repo), &scopes)?
            } else {
                config.include(group, Path::new(repo), &scopes)?
            };
            config.save()?;
            render::done(
                json,
                json!({ "group": group, "repo": repo, *verb: scopes }),
                &format!(
                    "{verb}d {} for {} (run sor sync)",
                    scopes.join(", "),
                    repo.display()
                ),
            );
        }
        ["group", "import", group, from, scopes @ ..] if !scopes.is_empty() => {
            let scopes = owned(scopes);
            config.import(group, from, &scopes)?;
            config.save()?;
            render::done(
                json,
                json!({ "group": group, "from": from, "imported": scopes }),
                &format!(
                    "'{group}' imports {} from '{from}' (run sor sync)",
                    scopes.join(", ")
                ),
            );
        }
        ["member", "add", group, path] => {
            let path = config.add_member(group, Path::new(path), f.workspace, f.depth)?;
            config.save()?;
            let kind = if f.workspace {
                "workspace"
            } else {
                "repository"
            };
            render::done(
                json,
                json!({ "group": group, "added": path, "workspace": f.workspace }),
                &format!("{kind} {} added to '{group}'", path.display()),
            );
        }
        ["member", "remove", group, path] => {
            config.remove_member(group, Path::new(path))?;
            config.save()?;
            render::done(
                json,
                json!({ "group": group, "removed": path }),
                &format!("{path} removed from '{group}' (run sor sync {path} to clean it)"),
            );
        }
        ["repos", group] => render::paths(&config.repos(group)?, json),
        ["status"] | ["status", _] | ["sync"] | ["sync", _] => {
            let apply = w[0] == "sync" && !f.dry_run;
            let repos = match w.get(1) {
                Some(repo) => vec![canonical(repo)?],
                None => config.all_repos()?,
            };
            let reports = repos
                .iter()
                .map(|r| engine::run(&config, r, apply))
                .collect::<Result<Vec<_>>>()?;
            render::reports(&reports, json);
            let drift = if w[0] == "status" || f.dry_run {
                reports.iter().any(|r| !r.clean)
            } else {
                reports.iter().any(engine::Report::pending)
            };
            return Ok(if drift { DRIFT } else { 0 });
        }
        ["resolve", repo, file, how] => {
            let msg = engine::resolve(&config, &canonical(repo)?, file, Resolution::parse(how)?)?;
            render::done(json, json!({ "resolved": file, "how": how }), &msg);
        }
        ["tidy", repo, paths @ ..] if !paths.is_empty() => {
            let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
            let report = capture::tidy(&config, &canonical(repo)?, &paths, !f.dry_run)?;
            render::tidy(&report, json);
        }
        _ => {
            return Err(Error::invalid(format!(
                "unknown command '{}' (see sor --help)",
                words.join(" ")
            )));
        }
    }
    Ok(0)
}

fn canonical(path: &str) -> Result<PathBuf> {
    fs::canonicalize(path).at(path)
}

fn usage(e: impl std::fmt::Display) -> Error {
    Error::invalid(format!("{e} (see sor --help)"))
}
