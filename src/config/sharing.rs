//! What a group installs where: shared scopes, exclusions, imports, agents.

use std::fs;
use std::path::{Path, PathBuf};

use super::Config;
use crate::error::{Error, IoContext, Result};

/// One scope to install in a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub group: String,
    pub scope: String,
    pub agents: bool,
}

impl Config {
    fn check_scopes(&self, group: &str, scopes: &[String]) -> Result<()> {
        self.group(group)?;
        let store = self.store();
        match scopes.iter().find(|s| !store.exists(group, s)) {
            Some(s) => Err(Error::invalid(format!(
                "scope '{s}' does not exist in group '{group}' (see sor scope list)"
            ))),
            None => Ok(()),
        }
    }

    pub fn share(&mut self, group: &str, scopes: &[String]) -> Result<()> {
        self.check_scopes(group, scopes)?;
        let g = self.group_mut(group)?;
        for s in scopes {
            if !g.scopes.contains(s) {
                g.scopes.push(s.clone());
            }
        }
        Ok(())
    }

    /// Stop installing scopes; members drop them at their next sync.
    pub fn unshare(&mut self, group: &str, scopes: &[String]) -> Result<()> {
        self.group_mut(group)?
            .scopes
            .retain(|s| !scopes.contains(s));
        Ok(())
    }

    pub fn set_agents(&mut self, group: &str, on: bool) -> Result<()> {
        self.group_mut(group)?.agents = on;
        Ok(())
    }

    pub fn exclude(&mut self, group: &str, repo: &Path, scopes: &[String]) -> Result<PathBuf> {
        let repo = fs::canonicalize(repo).at(repo)?;
        if !self.repos(group)?.contains(&repo) {
            return Err(Error::invalid(format!(
                "{} is not a repository of '{group}'",
                repo.display()
            )));
        }
        let list = self
            .group_mut(group)?
            .exclude
            .entry(repo.clone())
            .or_default();
        for s in scopes {
            if !list.contains(s) {
                list.push(s.clone());
            }
        }
        Ok(repo)
    }

    pub fn include(&mut self, group: &str, repo: &Path, scopes: &[String]) -> Result<PathBuf> {
        let repo = fs::canonicalize(repo).unwrap_or_else(|_| repo.to_path_buf());
        let g = self.group_mut(group)?;
        if let Some(list) = g.exclude.get_mut(&repo) {
            list.retain(|s| !scopes.contains(s));
            if list.is_empty() {
                g.exclude.remove(&repo);
            }
        }
        Ok(repo)
    }

    /// Install single scopes of `from` in the members of `group`.
    pub fn import(&mut self, group: &str, from: &str, scopes: &[String]) -> Result<()> {
        if group == from {
            return Err(Error::invalid("a group cannot import from itself"));
        }
        self.group(group)?;
        self.check_scopes(from, scopes)?;
        let list = self
            .group_mut(group)?
            .imports
            .entry(from.to_owned())
            .or_default();
        for s in scopes {
            if !list.contains(s) {
                list.push(s.clone());
            }
        }
        Ok(())
    }

    pub fn unimport(&mut self, group: &str, from: &str, scopes: &[String]) -> Result<()> {
        let imports = &mut self.group_mut(group)?.imports;
        if let Some(list) = imports.get_mut(from) {
            list.retain(|s| !scopes.contains(s));
            if list.is_empty() {
                imports.remove(from);
            }
        }
        Ok(())
    }

    /// Scopes `repo` receives from all its groups, each once.
    pub fn plan(&self, repo: &Path) -> Result<Vec<Planned>> {
        let mut out: Vec<Planned> = Vec::new();
        for (name, g) in &self.groups {
            if !self.repos(name)?.iter().any(|r| r == repo) {
                continue;
            }
            let excluded = g.exclude.get(repo);
            let own = g.scopes.iter().map(|s| (name, s));
            let imported = g
                .imports
                .iter()
                .flat_map(|(from, list)| list.iter().map(move |s| (from, s)));
            for (group, scope) in own.chain(imported) {
                if excluded.is_some_and(|x| x.contains(scope)) {
                    continue;
                }
                match out
                    .iter_mut()
                    .find(|p| p.group == *group && p.scope == *scope)
                {
                    Some(p) => p.agents |= g.agents,
                    None => out.push(Planned {
                        group: group.clone(),
                        scope: scope.clone(),
                        agents: g.agents,
                    }),
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn plan_applies_exclusions_and_imports() {
        let tmp = TempDir::new("plan");
        let repo = tmp.path().join("app");
        fs::create_dir_all(repo.join(".git")).unwrap();
        let mut config = Config::load(&tmp.path().join("home")).unwrap();
        for name in ["a", "b"] {
            config.add_group(name, "").unwrap();
            config.add_member(name, &repo, false, None).unwrap();
        }
        let a = config.group_mut("a").unwrap();
        a.scopes = vec!["commits".into(), "rust".into()];
        a.exclude.insert(repo.clone(), vec!["rust".into()]);
        let b = config.group_mut("b").unwrap();
        b.agents = true;
        b.imports.insert("a".into(), vec!["commits".into()]);
        let plan = config.plan(&repo).unwrap();
        assert_eq!(
            plan,
            vec![Planned {
                group: "a".into(),
                scope: "commits".into(),
                agents: true,
            }]
        );
    }
}
