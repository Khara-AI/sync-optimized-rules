//! Capturing scopes and choosing what each group and repository receives.

use std::fs;
use std::path::PathBuf;

use sync_optimized_rules::capture::{self, Options};
use sync_optimized_rules::config;
use sync_optimized_rules::engine;
use sync_optimized_rules::error::{IoContext, Result};
use sync_optimized_rules::store::Area;

use super::{Session, expand, name, plural, short};
use crate::cli::tui::{Item, List, Tone};

impl Session<'_> {
    pub(super) fn capture(&mut self, group: &str) -> Result<()> {
        let Some(repo) = self.source_repo(group)? else {
            return Ok(());
        };
        let found = capture::candidates(self.config, &repo)?;
        if found.is_empty() {
            self.warn(&format!("{}/.claude/ has nothing to capture", short(&repo)));
            return Ok(());
        }
        let items = found
            .iter()
            .map(|c| {
                let kind = match Area::of(&c.path).0 {
                    Area::Rules => "rule",
                    Area::Agents => "agent",
                    Area::Files if c.hooked => "file · its hook follows",
                    Area::Files => "file",
                };
                Item::new(&c.path, kind)
            })
            .collect();
        let list = List::multi("Files to share", items, Vec::new())
            .notes(vec![format!("in {}/.claude/", short(&repo))])
            .required();
        let Some(picked) = self.ui.tick(list)? else {
            return Ok(());
        };

        let store = self.config.store();
        let mut entries: Vec<(Item, Option<String>)> = store
            .scopes(group)
            .into_iter()
            .map(|s| {
                let hint = store
                    .meta(group, &s)
                    .map(|m| m.description)
                    .unwrap_or_default();
                (Item::new(&s, hint), Some(s))
            })
            .collect();
        let scope = if entries.is_empty() {
            self.scope_name(group)?
        } else {
            entries.push((Item::new("+ New scope", ""), None));
            match self.menu("Into which scope?", Vec::new(), entries)? {
                None => return Ok(()),
                Some(Some(s)) => Some(s),
                Some(None) => self.scope_name(group)?,
            }
        };
        let Some(scope) = scope else {
            return Ok(());
        };
        let description = if store.exists(group, &scope) {
            None
        } else {
            let Some(d) = self.ui.text("What is it for? (optional)", "", |_| Ok(()))? else {
                return Ok(());
            };
            Some(d)
        };
        let perms = capture::permission_count(&repo)?;
        let permissions = if perms == 0 {
            false
        } else {
            let question = format!(
                "Also share its {} (allow, deny, ask)?",
                plural(perms, "permission rule", "permission rules")
            );
            let Some(yes) = self.ui.confirm(&question, false)? else {
                return Ok(());
            };
            yes
        };

        let paths: Vec<PathBuf> = picked
            .iter()
            .map(|&i| PathBuf::from(".claude").join(&found[i].path))
            .collect();
        let opts = Options {
            description: description.as_deref(),
            permissions,
            move_originals: false,
        };
        let report = capture::capture(self.config, group, &scope, &repo, &paths, &opts)?;
        self.config.save()?;
        self.dirty = true;
        self.ok(&format!(
            "{} captured into {group}/{scope}",
            plural(report.files.len(), "file", "files")
        ));
        if !report.hooks.is_empty() {
            self.ui.say(
                Tone::Dim,
                &format!("  with their {} hooks", report.hooks.join(", ")),
            );
        }
        if report.permissions > 0 {
            let rules = plural(report.permissions, "permission rule", "permission rules");
            self.ui.say(Tone::Dim, &format!("  and {rules}"));
        }

        if !self.config.repos(group)?.contains(&repo) {
            return Ok(());
        }
        let question = format!(
            "Install them in {} and move its originals aside?",
            name(&repo)
        );
        if self.ui.confirm(&question, false)? != Some(true) {
            return Ok(());
        }
        let installed = engine::run(self.config, &repo, true)?;
        if installed.pending() {
            self.warn("that repository has conflicts: settle them with Check and sync first");
            return Ok(());
        }
        let tidy = capture::tidy(self.config, &repo, &paths, true)?;
        self.ok(&format!(
            "{} moved to .claude/sor/backup/",
            plural(tidy.moved.len(), "original", "originals")
        ));
        if !tidy.hooks.is_empty() {
            self.ui.say(
                Tone::Dim,
                "  their hooks left .claude/settings.json (a copy is in the backup)",
            );
        }
        Ok(())
    }

    fn source_repo(&self, group: &str) -> Result<Option<PathBuf>> {
        let mut entries: Vec<(Item, Option<PathBuf>)> = Vec::new();
        for repo in self.config.repos(group)? {
            let n = capture::candidates(self.config, &repo)?.len();
            if n > 0 {
                let hint = format!("{} · {}", plural(n, "file", "files"), short(&repo));
                entries.push((Item::new(name(&repo), hint), Some(repo)));
            }
        }
        entries.push((
            Item::new("Another folder", "a repository outside the group"),
            None,
        ));
        match self.menu("Capture from which repository?", Vec::new(), entries)? {
            None => Ok(None),
            Some(Some(repo)) => Ok(Some(repo)),
            Some(None) => {
                let Some(text) = self.ui.text("Repository path", "", |s| {
                    if expand(s).join(".claude").is_dir() {
                        Ok(())
                    } else {
                        Err("no .claude/ folder there".into())
                    }
                })?
                else {
                    return Ok(None);
                };
                let path = expand(&text);
                Ok(Some(fs::canonicalize(&path).at(&path)?))
            }
        }
    }

    fn scope_name(&self, group: &str) -> Result<Option<String>> {
        let taken = self.config.store().scopes(group);
        self.ui
            .text("Scope name (commits, rust, anonymity…)", "", |s| {
                config::check_name("scope", s).map_err(|e| e.to_string())?;
                if taken.iter().any(|t| t == s) {
                    return Err(format!("'{s}' exists: pick it in the list instead"));
                }
                Ok(())
            })
    }

    pub(super) fn shared(&mut self, group: &str) -> Result<()> {
        let store = self.config.store();
        let scopes = store.scopes(group);
        let shared = &self.config.group(group)?.scopes;
        let checked = scopes.iter().map(|s| shared.contains(s)).collect();
        let items = scopes
            .iter()
            .map(|s| {
                let n = store.files(group, s).len();
                let mut hint = plural(n, "file", "files");
                if let Ok(m) = store.meta(group, s) {
                    if !m.description.is_empty() {
                        hint = format!("{} · {hint}", m.description);
                    }
                }
                Item::new(s, hint)
            })
            .collect();
        let title = format!("Scopes {group} installs in its repositories");
        let Some(picked) = self.ui.tick(List::multi(&title, items, checked))? else {
            return Ok(());
        };
        let (on, off) = split(scopes, &picked);
        self.config.share(group, &on)?;
        self.config.unshare(group, &off)?;
        self.config.save()?;
        self.dirty = true;
        Ok(())
    }

    pub(super) fn borrow(&mut self, group: &str) -> Result<()> {
        let store = self.config.store();
        let imports = &self.config.group(group)?.imports;
        let mut pairs: Vec<(String, String)> = Vec::new();
        for other in self.config.groups.keys().filter(|o| *o != group) {
            pairs.extend(store.scopes(other).into_iter().map(|s| (other.clone(), s)));
        }
        let checked = pairs
            .iter()
            .map(|(o, s)| imports.get(o).is_some_and(|l| l.contains(s)))
            .collect();
        let items = pairs
            .iter()
            .map(|(o, s)| {
                let hint = store.meta(o, s).map(|m| m.description).unwrap_or_default();
                Item::new(format!("{o}/{s}"), hint)
            })
            .collect();
        let title = format!("Scopes of other groups that {group} installs too");
        let Some(picked) = self.ui.tick(List::multi(&title, items, checked))? else {
            return Ok(());
        };
        for (i, (from, scope)) in pairs.iter().enumerate() {
            let one = std::slice::from_ref(scope);
            if picked.contains(&i) {
                self.config.import(group, from, one)?;
            } else {
                self.config.unimport(group, from, one)?;
            }
        }
        self.config.save()?;
        self.dirty = true;
        Ok(())
    }

    pub(super) fn opt_out(&mut self, group: &str) -> Result<()> {
        let g = self.config.group(group)?;
        let entries = self
            .config
            .repos(group)?
            .into_iter()
            .map(|r| {
                let hint = g
                    .exclude
                    .get(&r)
                    .map(|l| format!("skips {}", l.join(", ")))
                    .unwrap_or_default();
                let hint = if hint.is_empty() { short(&r) } else { hint };
                (Item::new(name(&r), hint), r)
            })
            .collect();
        let Some(repo) = self.menu("Which repository?", Vec::new(), entries)? else {
            return Ok(());
        };
        let mut names: Vec<String> = g.scopes.clone();
        for s in g.imports.values().flatten() {
            if !names.contains(s) {
                names.push(s.clone());
            }
        }
        let skipped = g.exclude.get(&repo).cloned().unwrap_or_default();
        let checked = names.iter().map(|s| !skipped.contains(s)).collect();
        let items = names.iter().map(|s| Item::new(s, "")).collect();
        let title = format!("Scopes installed in {}", name(&repo));
        let Some(picked) = self.ui.tick(List::multi(&title, items, checked))? else {
            return Ok(());
        };
        let (keep, skip) = split(names, &picked);
        self.config.include(group, &repo, &keep)?;
        if !skip.is_empty() {
            self.config.exclude(group, &repo, &skip)?;
        }
        self.config.save()?;
        self.dirty = true;
        Ok(())
    }
}

/// The ticked names, then the others.
fn split(names: Vec<String>, ticked: &[usize]) -> (Vec<String>, Vec<String>) {
    let (mut on, mut off) = (Vec::new(), Vec::new());
    for (i, name) in names.into_iter().enumerate() {
        if ticked.contains(&i) {
            on.push(name);
        } else {
            off.push(name);
        }
    }
    (on, off)
}
