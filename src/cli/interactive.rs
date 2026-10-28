//! `sor` without arguments, in a terminal: groups, repositories, scopes and
//! sync driven from menus instead of commands.

mod capture;
mod members;
mod paths;
mod scopes;
mod sync;

use std::path::{Path, PathBuf};

use sync_optimized_rules::config::{self, Config};
use sync_optimized_rules::error::Result;

use super::tui::{self, Item, List, Tone, Ui};
use paths::short;

pub struct Session<'a> {
    ui: Ui,
    config: &'a mut Config,
    /// Changes not installed in the repositories yet.
    dirty: bool,
    /// Repositories that left every group and still hold what sor installed.
    leaving: Vec<PathBuf>,
}

#[derive(Clone)]
enum Main {
    Group(String),
    New,
    Status,
    Sync,
    Quit,
}

#[derive(Clone, Copy)]
enum GroupAction {
    Members,
    Capture,
    Shared,
    Borrow,
    OptOut,
    Agents,
    Describe,
    Sync,
    Forget,
}

pub fn run(config: &mut Config) -> Result<u8> {
    let mut s = Session {
        ui: Ui::new(),
        config,
        dirty: false,
        leaving: Vec::new(),
    };
    s.ui.say(Tone::Strong, "sor · Claude Code rules in sync");
    s.ui.say(Tone::Dim, "↑↓ move · enter choose · esc back · ctrl+c quit");
    loop {
        let mut entries: Vec<(Item, Main)> = s
            .config
            .groups
            .keys()
            .map(|n| (Item::new(n, s.group_hint(n)), Main::Group(n.clone())))
            .collect();
        entries.push((
            Item::new("+ New group", "repositories that share rules"),
            Main::New,
        ));
        if !s.config.groups.is_empty() {
            entries.push((
                Item::new("Status", "what differs, repository by repository"),
                Main::Status,
            ));
            let hint = if s.dirty { "changes waiting" } else { "" };
            entries.push((Item::new("Sync", hint), Main::Sync));
        }
        entries.push((Item::new("Quit", ""), Main::Quit));
        let result = match s.menu("What do you want to do?", Vec::new(), entries)? {
            None | Some(Main::Quit) => break,
            Some(Main::Group(g)) => s.group(&g),
            Some(Main::New) => s.new_group(),
            Some(Main::Status) => s.status(),
            Some(Main::Sync) => s.sync(),
        };
        s.guard(result)?;
    }
    if s.dirty || !s.leaving.is_empty() {
        s.ui.say(Tone::Dim, "Some changes are not installed yet:");
        let result = s.sync();
        s.guard(result)?;
    }
    Ok(0)
}

impl Session<'_> {
    fn menu<A: Clone>(
        &self,
        title: &str,
        notes: Vec<String>,
        entries: Vec<(Item, A)>,
    ) -> Result<Option<A>> {
        let (items, actions): (Vec<Item>, Vec<A>) = entries.into_iter().unzip();
        let picked = self.ui.pick(List::single(title, items).notes(notes))?;
        Ok(picked.and_then(|i| actions.get(i).cloned()))
    }

    fn ok(&self, msg: &str) {
        self.ui.say(Tone::Good, &format!("✔ {msg}"));
    }

    fn warn(&self, msg: &str) {
        self.ui.say(Tone::Bad, &format!("✖ {msg}"));
    }

    /// A failed action is reported and the session goes on; only a broken
    /// terminal ends it.
    fn guard(&self, result: Result<()>) -> Result<()> {
        match result {
            Err(e) if !tui::is_terminal_error(&e) => {
                self.warn(&e.to_string());
                Ok(())
            }
            other => other,
        }
    }

    fn group_hint(&self, name: &str) -> String {
        let Some(g) = self.config.groups.get(name) else {
            return String::new();
        };
        let repos = self.config.repos(name).map_or(0, |r| r.len());
        let mut parts = vec![plural(repos, "repository", "repositories")];
        if !g.description.is_empty() {
            parts.insert(0, g.description.clone());
        }
        if !g.scopes.is_empty() {
            parts.push(format!("shares {}", g.scopes.join(", ")));
        }
        if g.agents {
            parts.push("agents on".into());
        }
        parts.join(" · ")
    }

    fn new_group(&mut self) -> Result<()> {
        let taken: Vec<String> = self.config.groups.keys().cloned().collect();
        let Some(name) = self.ui.text("Group name", "", |s| {
            config::check_name("group", s).map_err(|e| e.to_string())?;
            if taken.iter().any(|t| t == s) {
                return Err(format!("'{s}' already exists"));
            }
            Ok(())
        })?
        else {
            return Ok(());
        };
        let Some(description) = self.ui.text("Description (optional)", "", |_| Ok(()))? else {
            return Ok(());
        };
        self.config.add_group(&name, &description)?;
        self.config.save()?;
        self.ok(&format!("group {name} created"));

        // First run: repositories, then the rules they share, then install.
        if self.add_repos(&name)?
            && self
                .ui
                .confirm("Capture rules from one of them now?", true)?
                == Some(true)
        {
            self.capture(&name)?;
        }
        if self.dirty {
            self.sync()?;
        }
        self.group(&name)
    }

    fn group(&mut self, name: &str) -> Result<()> {
        loop {
            let Some(g) = self.config.groups.get(name) else {
                return Ok(());
            };
            let repos = self.config.repos(name)?;
            let store = self.config.store();
            let scopes = store.scopes(name);
            let lendable = self
                .config
                .groups
                .keys()
                .any(|o| o != name && !store.scopes(o).is_empty());
            let shared = if g.scopes.is_empty() {
                "none".to_owned()
            } else {
                g.scopes.join(", ")
            };
            let mut e = vec![
                (
                    Item::new(
                        "Repositories",
                        plural(repos.len(), "repository", "repositories"),
                    ),
                    GroupAction::Members,
                ),
                (
                    Item::new("Capture rules", "turn files of a .claude/ into a scope"),
                    GroupAction::Capture,
                ),
            ];
            if !scopes.is_empty() {
                e.push((Item::new("Shared scopes", shared), GroupAction::Shared));
            }
            if lendable {
                let borrowed: Vec<String> = g
                    .imports
                    .iter()
                    .flat_map(|(from, l)| l.iter().map(move |s| format!("{from}/{s}")))
                    .collect();
                let hint = if borrowed.is_empty() {
                    "single scopes of other groups".to_owned()
                } else {
                    borrowed.join(", ")
                };
                e.push((Item::new("Borrowed scopes", hint), GroupAction::Borrow));
            }
            if !repos.is_empty() && !(g.scopes.is_empty() && g.imports.is_empty()) {
                let hint = match g.exclude.len() {
                    0 => "a repository can skip a scope".to_owned(),
                    n => format!("{} skip scopes", plural(n, "repository", "repositories")),
                };
                e.push((Item::new("Exceptions", hint), GroupAction::OptOut));
            }
            let agents = if g.agents { "on" } else { "off" };
            e.push((
                Item::new(
                    format!("Agents: {agents}"),
                    "also share the agents of its scopes",
                ),
                GroupAction::Agents,
            ));
            let description = if g.description.is_empty() {
                "none".to_owned()
            } else {
                g.description.clone()
            };
            e.push((Item::new("Description", description), GroupAction::Describe));
            let waiting = if self.dirty { "changes waiting" } else { "" };
            e.push((Item::new("Check and sync", waiting), GroupAction::Sync));
            e.push((
                Item::new(
                    "Forget this group",
                    "what it installed goes to trash at the next sync",
                ),
                GroupAction::Forget,
            ));

            let Some(action) = self.menu(&format!("Group {name}"), Vec::new(), e)? else {
                return Ok(());
            };
            let result = match action {
                GroupAction::Members => self.members(name),
                GroupAction::Capture => self.capture(name),
                GroupAction::Shared => self.shared(name),
                GroupAction::Borrow => self.borrow(name),
                GroupAction::OptOut => self.opt_out(name),
                GroupAction::Agents => self.toggle_agents(name),
                GroupAction::Describe => self.describe(name),
                GroupAction::Sync => self.sync(),
                GroupAction::Forget => self.forget(name),
            };
            self.guard(result)?;
        }
    }

    fn toggle_agents(&mut self, group: &str) -> Result<()> {
        let on = !self.config.group(group)?.agents;
        self.config.set_agents(group, on)?;
        self.config.save()?;
        self.dirty = true;
        self.ok(&format!("agents {}", if on { "on" } else { "off" }));
        Ok(())
    }

    fn describe(&mut self, group: &str) -> Result<()> {
        let current = self.config.group(group)?.description.clone();
        let Some(text) = self.ui.text("Description", &current, |_| Ok(()))? else {
            return Ok(());
        };
        if let Some(g) = self.config.groups.get_mut(group) {
            g.description = text;
        }
        self.config.save()
    }

    fn forget(&mut self, group: &str) -> Result<()> {
        let question = format!("Forget {group}? Its scopes stay in the store");
        if self.ui.confirm(&question, false)? != Some(true) {
            return Ok(());
        }
        let repos = self.config.repos(group)?;
        self.config.remove_group(group)?;
        self.config.save()?;
        self.leaving.extend(repos);
        self.dirty = true;
        self.ok(&format!("{group} forgotten"));
        self.sync()
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// A repository's folder name, the way people call it.
fn name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| short(path), |n| n.to_string_lossy().into_owned())
}
