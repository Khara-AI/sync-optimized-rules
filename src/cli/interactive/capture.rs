//! Capturing rules: from one or several repositories, into one or several
//! scopes, with the versions that differ merged by Claude when asked.

use std::path::PathBuf;

use sync_optimized_rules::capture::{self, Found};
use sync_optimized_rules::engine;
use sync_optimized_rules::error::Result;
use sync_optimized_rules::store::Area;

mod plan;

use super::paths::{short, start_folder};
use super::{Session, name, plural};
use crate::cli::tui::{Item, List, Tone};

/// Versions of one path a scope holds side by side.
struct Versions {
    scope: String,
    path: String,
    names: Vec<String>,
}

impl Session<'_> {
    pub(super) fn capture(&mut self, group: &str) -> Result<()> {
        let Some(repos) = self.source_repos(group)? else {
            return Ok(());
        };
        let found = capture::gather(self.config, &repos)?;
        if found.is_empty() {
            self.warn("their .claude/ folders have nothing to capture");
            return Ok(());
        }
        let several = repos.len() > 1;
        let items = found
            .iter()
            .map(|f| {
                let mut hint = kind(f).to_owned();
                if several {
                    let from: Vec<String> = f.repos.iter().map(|r| name(r)).collect();
                    hint = format!("{hint} · {}", from.join(", "));
                }
                Item::new(&f.name, hint)
            })
            .collect();
        let names: Vec<String> = repos.iter().map(|r| name(r)).collect();
        let mut notes = vec![format!("from {}", names.join(", "))];
        let mut differ: Vec<&str> = found
            .iter()
            .filter(|f| f.is_version())
            .map(|f| f.path.as_str())
            .collect();
        differ.dedup();
        if !differ.is_empty() {
            notes.push(format!(
                "{} differ between repositories: each version carries its repository's name",
                plural(differ.len(), "file", "files")
            ));
        }
        let list = List::multi("Files to capture", items, Vec::new())
            .notes(notes)
            .required();
        let Some(picked) = self.ui.tick(list)? else {
            return Ok(());
        };
        let Some(batches) = self.plan(group, &found, &picked)? else {
            return Ok(());
        };
        let Some(perms_to) = self.permissions(&repos, &batches)? else {
            return Ok(());
        };

        let mut versions: Vec<Versions> = Vec::new();
        for (n, b) in batches.iter().enumerate() {
            let _lock = engine::Lock::wait(self.config.root())?;
            let files = capture::scope_names(&found, &b.files);
            let from: &[PathBuf] = if perms_to == Some(n) { &repos } else { &[] };
            let report = capture::capture_found(
                self.config,
                group,
                &b.scope,
                &files,
                b.description.as_deref(),
                from,
            )?;
            self.ok(&format!(
                "{} captured into {group}/{}",
                plural(report.files.len(), "file", "files"),
                b.scope
            ));
            if !report.hooks.is_empty() {
                let hooks = report.hooks.join(", ");
                self.ui
                    .say(Tone::Dim, &format!("  with their {hooks} hooks"));
            }
            if report.permissions > 0 {
                let rules = plural(report.permissions, "permission rule", "permission rules");
                self.ui.say(Tone::Dim, &format!("  and {rules}"));
            }
            for f in files.iter().filter(|f| f.is_version()) {
                match versions
                    .iter_mut()
                    .find(|v| v.scope == b.scope && v.path == f.path)
                {
                    Some(v) => v.names.push(f.name.clone()),
                    None => versions.push(Versions {
                        scope: b.scope.clone(),
                        path: f.path.clone(),
                        names: vec![f.name.clone()],
                    }),
                }
            }
        }
        self.config.save()?;
        self.dirty = true;
        self.merge_versions(group, &versions)?;
        self.tidy_sources(group, &repos, &found, &picked)
    }

    /// Repositories to capture from: members, or any folder with a .claude/.
    fn source_repos(&self, group: &str) -> Result<Option<Vec<PathBuf>>> {
        let mut repos: Vec<PathBuf> = Vec::new();
        let mut items: Vec<Item> = Vec::new();
        for repo in self.config.repos(group)? {
            let n = capture::candidates(self.config, &repo)?.len();
            if n > 0 {
                let hint = format!("{} · {}", plural(n, "file", "files"), short(&repo));
                items.push(Item::new(name(&repo), hint));
                repos.push(repo);
            }
        }
        let other = repos.len();
        let mut picked = if repos.is_empty() {
            vec![other]
        } else {
            items.push(Item::new(
                "Another repository…",
                "one outside the group, by its folder",
            ));
            let list = List::multi("Capture from which repositories?", items, Vec::new())
                .notes(vec!["tick several to gather their rules".into()])
                .required();
            let Some(picked) = self.ui.tick(list)? else {
                return Ok(None);
            };
            picked
        };
        if picked.contains(&other) {
            picked.retain(|&i| i != other);
            let title = "Repository to capture from";
            match self.ask_folder(title, &start_folder(), Some(".claude"))? {
                Some(path) if !repos.contains(&path) => {
                    picked.push(repos.len());
                    repos.push(path);
                }
                Some(_) => {}
                None if picked.is_empty() => return Ok(None),
                None => {}
            }
        }
        Ok(Some(picked.into_iter().map(|i| repos[i].clone()).collect()))
    }

    fn merge_versions(&self, group: &str, versions: &[Versions]) -> Result<()> {
        if versions.is_empty() {
            return Ok(());
        }
        self.warn(&format!(
            "{} with several versions to merge:",
            plural(versions.len(), "file", "files")
        ));
        for v in versions {
            self.ui.say(
                Tone::Dim,
                &format!("  {}/{}: {}", v.scope, v.path, v.names.join(", ")),
            );
        }
        if capture::claude().is_none() {
            let store = short(&self.config.store().dir(group, ""));
            self.warn(&format!(
                "claude was not found in PATH: the versions stay side by side; merge them by hand in {store}"
            ));
            return Ok(());
        }
        if self
            .ui
            .confirm("Merge each into one file with Claude?", true)?
            != Some(true)
        {
            return Ok(());
        }
        for v in versions {
            self.ui
                .say(Tone::Dim, &format!("  Claude is merging {}…", v.path));
            let _lock = engine::Lock::wait(self.config.root())?;
            match capture::merge(self.config, group, &v.scope, &v.path, &v.names) {
                Ok(()) => self.ok(&format!("{}/{} merged", v.scope, v.path)),
                Err(e) => self.warn(&format!("{}: {e}; the versions stay side by side", v.path)),
            }
        }
        Ok(())
    }

    /// Offer to install the scopes in the source repositories of the group
    /// and to move their originals aside.
    fn tidy_sources(
        &mut self,
        group: &str,
        repos: &[PathBuf],
        found: &[Found],
        picked: &[usize],
    ) -> Result<()> {
        let members = self.config.repos(group)?;
        let sources: Vec<&PathBuf> = repos.iter().filter(|r| members.contains(r)).collect();
        if sources.is_empty() {
            return Ok(());
        }
        let names: Vec<String> = sources.iter().map(|r| name(r)).collect();
        let question = format!(
            "Install them in {} and move the originals aside?",
            names.join(", ")
        );
        if self.ui.confirm(&question, false)? != Some(true) {
            return Ok(());
        }
        for repo in sources {
            let _lock = engine::Lock::wait(self.config.root())?;
            let installed = engine::run(self.config, repo, true)?;
            if installed.pending() {
                self.warn(&format!(
                    "{} has conflicts: settle them with Check and sync first",
                    name(repo)
                ));
                continue;
            }
            let paths: Vec<PathBuf> = picked
                .iter()
                .map(|&i| &found[i])
                .filter(|f| f.repos.contains(repo))
                .map(|f| PathBuf::from(".claude").join(&f.path))
                .collect();
            match capture::tidy(self.config, repo, &paths, true) {
                Ok(t) => {
                    self.ok(&format!(
                        "{}: {} moved to .claude/sor/backup/",
                        name(repo),
                        plural(t.moved.len(), "original", "originals")
                    ));
                    if !t.hooks.is_empty() {
                        self.ui.say(
                            Tone::Dim,
                            "  their hooks left .claude/settings.json (a copy is in the backup)",
                        );
                    }
                }
                Err(e) => self.warn(&format!("{}: {e}", name(repo))),
            }
        }
        Ok(())
    }
}

fn kind(f: &Found) -> &'static str {
    match Area::of(&f.path).0 {
        Area::Rules => "rule",
        Area::Agents => "agent",
        Area::Files if f.hooked => "file · its hook follows",
        Area::Files => "file",
    }
}
