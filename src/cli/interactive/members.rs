//! Repositories and workspaces of a group.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use sync_optimized_rules::config::DEFAULT_DEPTH;
use sync_optimized_rules::discover;
use sync_optimized_rules::error::{IoContext, Result};
use sync_optimized_rules::fsutil;

use super::{Session, expand, plural, short};
use crate::cli::tui::{Item, List};

#[derive(Clone, Copy)]
enum MemberAction {
    Repos,
    Workspace,
    Remove,
}

impl Session<'_> {
    pub(super) fn members(&mut self, group: &str) -> Result<()> {
        loop {
            let Some(g) = self.config.groups.get(group) else {
                return Ok(());
            };
            let notes: Vec<String> = g
                .members
                .iter()
                .map(|m| {
                    if m.workspace {
                        let n = discover::repos(&m.path, m.depth.unwrap_or(DEFAULT_DEPTH)).len();
                        format!(
                            "workspace  {} · {}",
                            short(&m.path),
                            plural(n, "repository", "repositories")
                        )
                    } else {
                        format!("repository {}", short(&m.path))
                    }
                })
                .collect();
            let mut e = vec![
                (
                    Item::new("Add repositories", "pick them in a folder"),
                    MemberAction::Repos,
                ),
                (
                    Item::new(
                        "Add a workspace",
                        "every repository inside, new ones included",
                    ),
                    MemberAction::Workspace,
                ),
            ];
            if !g.members.is_empty() {
                e.push((Item::new("Remove", ""), MemberAction::Remove));
            }
            let Some(action) = self.menu(&format!("Repositories of {group}"), notes, e)? else {
                return Ok(());
            };
            let result = match action {
                MemberAction::Repos => self.add_repos(group).map(|_| ()),
                MemberAction::Workspace => self.add_workspace(group),
                MemberAction::Remove => self.remove_members(group),
            };
            self.guard(result)?;
        }
    }

    fn ask_folder(&self, title: &str) -> Result<Option<PathBuf>> {
        let Some(text) = self.ui.text(title, &short(&start_folder()), |s| {
            if expand(s).is_dir() {
                Ok(())
            } else {
                Err("not a folder".into())
            }
        })?
        else {
            return Ok(None);
        };
        let path = expand(&text);
        Ok(Some(fs::canonicalize(&path).at(&path)?))
    }

    /// Returns whether repositories were added.
    pub(super) fn add_repos(&mut self, group: &str) -> Result<bool> {
        let Some(folder) = self.ask_folder("Folder to look into")? else {
            return Ok(false);
        };
        let known = self.config.repos(group)?;
        let found: Vec<PathBuf> = discover::repos(&folder, DEFAULT_DEPTH)
            .into_iter()
            .filter(|r| !known.contains(r))
            .collect();
        if found.is_empty() {
            self.warn(&format!(
                "no new git repository in {} ({DEFAULT_DEPTH} levels down)",
                short(&folder)
            ));
            return Ok(false);
        }
        let items = found
            .iter()
            .map(|r| {
                let label = match r.strip_prefix(&folder).map(fsutil::slash) {
                    Ok(rel) if !rel.is_empty() => rel,
                    _ => short(r),
                };
                let hint = if r.join(".claude").is_dir() {
                    "has .claude/"
                } else {
                    ""
                };
                Item::new(label, hint)
            })
            .collect();
        let list = List::multi(
            &format!("Repositories to add to {group}"),
            items,
            Vec::new(),
        )
        .required();
        let Some(picked) = self.ui.tick(list)? else {
            return Ok(false);
        };
        for &i in &picked {
            self.config.add_member(group, &found[i], false, None)?;
        }
        self.config.save()?;
        self.dirty = true;
        self.ok(&format!(
            "{} added",
            plural(picked.len(), "repository", "repositories")
        ));
        Ok(true)
    }

    fn add_workspace(&mut self, group: &str) -> Result<()> {
        let Some(folder) = self.ask_folder("Workspace folder")? else {
            return Ok(());
        };
        let n = discover::repos(&folder, DEFAULT_DEPTH).len();
        let question = format!(
            "Add {} ({} now; new ones join at each sync)?",
            short(&folder),
            plural(n, "repository", "repositories")
        );
        if self.ui.confirm(&question, true)? != Some(true) {
            return Ok(());
        }
        self.config.add_member(group, &folder, true, None)?;
        self.config.save()?;
        self.dirty = true;
        self.ok(&format!("workspace {} added", short(&folder)));
        Ok(())
    }

    fn remove_members(&mut self, group: &str) -> Result<()> {
        let Some(g) = self.config.groups.get(group) else {
            return Ok(());
        };
        let members: Vec<PathBuf> = g.members.iter().map(|m| m.path.clone()).collect();
        let items = g
            .members
            .iter()
            .map(|m| Item::new(short(&m.path), if m.workspace { "workspace" } else { "" }))
            .collect();
        let list = List::multi("Remove which?", items, Vec::new()).required();
        let Some(picked) = self.ui.tick(list)? else {
            return Ok(());
        };
        let before = self.config.repos(group)?;
        for &i in &picked {
            self.config.remove_member(group, &members[i])?;
        }
        self.config.save()?;
        let after = self.config.repos(group)?;
        self.leaving
            .extend(before.into_iter().filter(|r| !after.contains(r)));
        self.dirty = true;
        self.ok(&format!(
            "{} removed; the next sync cleans what sor installed there",
            plural(picked.len(), "member", "members")
        ));
        Ok(())
    }
}

/// The current folder, or its parent when it is itself a repository: the
/// folder where its sibling repositories are.
fn start_folder() -> PathBuf {
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if discover::is_repo(&cwd) {
        cwd.parent().map_or(cwd.clone(), Path::to_path_buf)
    } else {
        cwd
    }
}
