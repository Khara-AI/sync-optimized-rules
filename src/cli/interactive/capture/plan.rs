//! Which scope receives which files, and the permission rules.

use std::path::PathBuf;

use sync_optimized_rules::capture::{self, Found};
use sync_optimized_rules::config;
use sync_optimized_rules::error::Result;

use super::kind;
use crate::cli::interactive::{Session, plural};
use crate::cli::tui::{Item, List, Tone};

/// Files bound for one scope, as indexes into the gathered files.
pub(super) struct Batch {
    pub scope: String,
    /// Set for a scope this capture creates.
    pub description: Option<String>,
    pub files: Vec<usize>,
}

#[derive(Clone, Copy)]
enum Layout {
    One,
    Several,
}

impl Session<'_> {
    /// Which scope receives which files.
    pub(super) fn plan(
        &self,
        group: &str,
        found: &[Found],
        picked: &[usize],
    ) -> Result<Option<Vec<Batch>>> {
        let layout = if picked.len() > 1 {
            let entries = vec![
                (Item::new("One scope", "all of them together"), Layout::One),
                (
                    Item::new(
                        "Several scopes",
                        "e.g. anonymity and code: tick the files of each",
                    ),
                    Layout::Several,
                ),
            ];
            let title = format!("Share these {} files in…", picked.len());
            match self.menu(&title, Vec::new(), entries)? {
                Some(l) => l,
                None => return Ok(None),
            }
        } else {
            Layout::One
        };
        let mut batches: Vec<Batch> = Vec::new();
        if let Layout::One = layout {
            let Some((scope, description)) =
                self.pick_scope(group, &batches, "Into which scope?")?
            else {
                return Ok(None);
            };
            batches.push(Batch {
                scope,
                description,
                files: picked.to_vec(),
            });
            return Ok(Some(batches));
        }

        let mut left = picked.to_vec();
        while !left.is_empty() {
            let title = if batches.is_empty() {
                "First scope".to_owned()
            } else {
                format!("Scope for the {} left", plural(left.len(), "file", "files"))
            };
            let Some((scope, description)) = self.pick_scope(group, &batches, &title)? else {
                if batches.is_empty() {
                    return Ok(None);
                }
                let question = format!("Leave the {} out?", plural(left.len(), "file", "files"));
                if self.ui.confirm(&question, false)? == Some(true) {
                    break;
                }
                continue;
            };
            let items = left
                .iter()
                .map(|&i| Item::new(&found[i].name, kind(&found[i])))
                .collect();
            let list = List::multi(&format!("Files for {scope}"), items, Vec::new()).required();
            let Some(ticked) = self.ui.tick(list)? else {
                continue;
            };
            let chosen: Vec<usize> = ticked.iter().map(|&t| left[t]).collect();
            left.retain(|i| !chosen.contains(i));
            match batches.iter_mut().find(|b| b.scope == scope) {
                Some(b) => b.files.extend(chosen),
                None => batches.push(Batch {
                    scope,
                    description,
                    files: chosen,
                }),
            }
        }
        Ok(Some(batches))
    }

    /// An existing scope, one this capture creates, or a new one (with its
    /// description).
    fn pick_scope(
        &self,
        group: &str,
        batches: &[Batch],
        title: &str,
    ) -> Result<Option<(String, Option<String>)>> {
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
        for b in batches.iter().filter(|b| b.description.is_some()) {
            let hint = format!("new · {}", plural(b.files.len(), "file", "files"));
            entries.push((Item::new(&b.scope, hint), Some(b.scope.clone())));
        }
        if !entries.is_empty() {
            entries.push((Item::new("+ New scope", ""), None));
            match self.menu(title, Vec::new(), entries)? {
                None => return Ok(None),
                Some(Some(s)) => {
                    let description = batches
                        .iter()
                        .find(|b| b.scope == s)
                        .and_then(|b| b.description.clone());
                    return Ok(Some((s, description)));
                }
                Some(None) => {}
            }
        }
        let mut taken = store.scopes(group);
        taken.extend(batches.iter().map(|b| b.scope.clone()));
        let Some(scope) = self
            .ui
            .text("Scope name (anonymity, rust, commits…)", "", |s| {
                config::check_name("scope", s).map_err(|e| e.to_string())?;
                if taken.iter().any(|t| t == s) {
                    return Err(format!("'{s}' exists: pick it in the list instead"));
                }
                Ok(())
            })?
        else {
            return Ok(None);
        };
        let Some(description) = self.ui.text("What is it for? (optional)", "", |_| Ok(()))? else {
            return Ok(None);
        };
        Ok(Some((scope, Some(description))))
    }

    /// `None` when left with esc; otherwise the batch that takes the
    /// permission rules, if any.
    pub(super) fn permissions(
        &self,
        repos: &[PathBuf],
        batches: &[Batch],
    ) -> Result<Option<Option<usize>>> {
        let mut counts = [0; 3];
        for r in repos {
            let c = capture::permission_counts(r)?;
            counts.iter_mut().zip(c).for_each(|(t, n)| *t += n);
        }
        if counts == [0; 3] {
            return Ok(Some(None));
        }
        let [allow, deny, ask] = counts;
        let split: Vec<String> = [(allow, "allow"), (deny, "deny"), (ask, "ask")]
            .iter()
            .filter(|(n, _)| *n > 0)
            .map(|(n, k)| format!("{n} {k}"))
            .collect();
        for line in [
            "  Permissions of .claude/settings.json: commands Claude runs without asking (allow),",
            "  never runs (deny), or asks about first (ask). Shared, every repository of the group gets them.",
        ] {
            self.ui.say(Tone::Dim, line);
        }
        if allow > 0 {
            self.ui.say(
                Tone::Dim,
                "  Allow rules grant the same rights in every repository: check they fit them all.",
            );
        }
        let whose = if repos.len() > 1 { "their" } else { "its" };
        let question = format!("Share {whose} permissions too? ({})", split.join(", "));
        // Deny and ask only restrict Claude: safe to share by default.
        match self.ui.confirm(&question, allow == 0)? {
            None => return Ok(None),
            Some(false) => return Ok(Some(None)),
            Some(true) => {}
        }
        if batches.len() == 1 {
            return Ok(Some(Some(0)));
        }
        let entries = batches
            .iter()
            .enumerate()
            .map(|(n, b)| (Item::new(&b.scope, ""), n))
            .collect();
        Ok(self
            .menu("Which scope takes them?", Vec::new(), entries)?
            .map(Some))
    }
}
