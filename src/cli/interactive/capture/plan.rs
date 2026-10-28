//! Which scope receives which files, and the permission rules.

use std::path::PathBuf;

use sync_optimized_rules::capture::{self, Found};
use sync_optimized_rules::config;
use sync_optimized_rules::error::Result;

use super::kind;
use crate::cli::interactive::{Session, plural};
use crate::cli::tui::{Item, List};

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
        let mut total = 0;
        for r in repos {
            total += capture::permission_count(r)?;
        }
        if total == 0 {
            return Ok(Some(None));
        }
        let rules = plural(total, "permission rule", "permission rules");
        let whose = if repos.len() > 1 { "their" } else { "its" };
        let question = format!("Also share {whose} {rules} (allow, deny, ask)?");
        match self.ui.confirm(&question, false)? {
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
