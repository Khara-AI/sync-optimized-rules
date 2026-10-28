//! Status, sync and settling conflicts one file at a time.

use std::path::{Path, PathBuf};
use std::process::Command;

use sync_optimized_rules::engine::{self, FileState, Report, Resolution};
use sync_optimized_rules::error::Result;

use super::{Session, name, plural};
use crate::cli::render;
use crate::cli::tui::{Item, Tone};

#[derive(Clone, Copy)]
enum Settle {
    Keep,
    Take,
    Promote,
    Diff,
    Later,
}

/// Something a sync can install or remove on its own.
fn applicable(r: &Report) -> bool {
    r.files.iter().any(|f| {
        matches!(
            f.state,
            FileState::Missing | FileState::Outdated | FileState::Stale
        )
    }) || !r.settings.missing.is_empty()
        || !r.settings.removed.is_empty()
        || r.exclude
}

impl Session<'_> {
    fn repositories(&self) -> Result<Vec<PathBuf>> {
        let mut repos = self.config.all_repos()?;
        repos.extend(self.leaving.iter().cloned());
        repos.sort();
        repos.dedup();
        Ok(repos)
    }

    fn preview(&self) -> Result<Vec<Report>> {
        self.repositories()?
            .iter()
            .map(|r| engine::run(self.config, r, false))
            .collect()
    }

    pub(super) fn status(&mut self) -> Result<()> {
        let reports = self.preview()?;
        let mut shown = false;
        for r in reports.iter().filter(|r| !r.clean) {
            render::report(r, false);
            shown = true;
        }
        if !shown {
            self.ok(&format!(
                "{} up to date",
                plural(reports.len(), "repository", "repositories")
            ));
        }
        Ok(())
    }

    pub(super) fn sync(&mut self) -> Result<()> {
        let reports = self.preview()?;
        let changing: Vec<&Report> = reports.iter().filter(|r| !r.clean).collect();
        if changing.is_empty() {
            self.dirty = false;
            self.leaving.clear();
            self.ok(&format!(
                "{} up to date",
                plural(reports.len(), "repository", "repositories")
            ));
            return Ok(());
        }
        changing.iter().for_each(|r| render::report(r, false));

        let apply: Vec<&Path> = changing
            .iter()
            .filter(|r| applicable(r))
            .map(|r| r.repo.as_path())
            .collect();
        if !apply.is_empty() {
            let question = format!(
                "Apply to {}?",
                plural(apply.len(), "repository", "repositories")
            );
            if self.ui.confirm(&question, true)? != Some(true) {
                return Ok(());
            }
            for repo in &apply {
                engine::run(self.config, repo, true)?;
            }
            self.dirty = false;
            self.leaving.clear();
            self.ok(&format!(
                "{} synced",
                plural(apply.len(), "repository", "repositories")
            ));
        }

        for r in &changing {
            for f in r.files.iter().filter(|f| f.state.is_conflict()) {
                if !self.settle(&r.repo, &f.path, f.state)? {
                    return Ok(());
                }
            }
            for clash in &r.settings.clash {
                self.ui.say(
                    Tone::Dim,
                    &format!(
                        "  {} keeps its own {clash}: change it there to take the group's",
                        name(&r.repo)
                    ),
                );
            }
        }
        Ok(())
    }

    /// Returns false when the user leaves the remaining conflicts for later.
    fn settle(&mut self, repo: &Path, file: &str, state: FileState) -> Result<bool> {
        let why = if state == FileState::Modified {
            "you edited the copy sor installed"
        } else {
            "a file was already there before sor"
        };
        let title = format!("{file} in {}", name(repo));
        loop {
            let entries = vec![
                (
                    Item::new("Keep mine here", "until the group's version changes again"),
                    Settle::Keep,
                ),
                (
                    Item::new(
                        "Take the group's version",
                        "mine goes to .claude/sor/backup/",
                    ),
                    Settle::Take,
                ),
                (
                    Item::new("Promote mine to the group", "every member gets it"),
                    Settle::Promote,
                ),
                (Item::new("Show the difference", ""), Settle::Diff),
                (Item::new("Decide later", ""), Settle::Later),
            ];
            let Some(choice) = self.menu(&title, vec![why.to_owned()], entries)? else {
                return Ok(false);
            };
            let how = match choice {
                Settle::Keep => Resolution::Keep,
                Settle::Take => Resolution::Take,
                Settle::Promote => Resolution::Promote,
                Settle::Diff => {
                    self.diff(repo, file)?;
                    continue;
                }
                Settle::Later => return Ok(true),
            };
            let done = engine::resolve(self.config, repo, file, how)?;
            if how == Resolution::Promote {
                self.dirty = true;
            }
            self.ok(&done);
            return Ok(true);
        }
    }

    fn diff(&self, repo: &Path, file: &str) -> Result<()> {
        let group = engine::group_copy(self.config, repo, file)?;
        let color = if self.ui.color() {
            "--color=always"
        } else {
            "--no-color"
        };
        let shown = Command::new("git")
            .args(["--no-pager", "diff", "--no-index", color, "--"])
            .arg(&group)
            .arg(repo.join(file))
            .output();
        match shown {
            Ok(out) => {
                self.ui.say(Tone::Dim, "  − the group's version · + yours");
                let text = String::from_utf8_lossy(&out.stdout);
                // The header repeats both full paths: the hunks are enough.
                for line in text.lines().skip_while(|l| !unstyled(l).starts_with("@@")) {
                    println!("{line}");
                }
            }
            Err(e) => self.warn(&format!("git shows the difference, and could not run: {e}")),
        }
        Ok(())
    }
}

/// A line without its leading colour codes.
fn unstyled(line: &str) -> &str {
    let mut rest = line;
    while let Some(after) = rest.strip_prefix("\x1b[") {
        rest = after.split_once('m').map_or("", |(_, r)| r);
    }
    rest
}
