//! Choosing what each group and repository receives.

use sync_optimized_rules::error::Result;

use super::paths::short;
use super::{Session, name, plural};
use crate::cli::tui::{Item, List};

impl Session<'_> {
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
