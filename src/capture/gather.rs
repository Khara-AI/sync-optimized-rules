//! Files of several repositories gathered for one capture: identical copies
//! count once, and each different version of a path takes the name of its
//! repository until the versions are merged.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::candidates;
use crate::config::Config;
use crate::error::Result;
use crate::fsutil;

#[derive(Debug, Clone, Serialize)]
pub struct Found {
    /// Relative to `.claude/` in the repositories.
    pub path: String,
    /// Relative to `.claude/` once in the scope: `path`, or
    /// `<folder>/<repository>-<file>` when the repositories disagree.
    pub name: String,
    /// Repositories holding this exact version, the first one is copied.
    pub repos: Vec<PathBuf>,
    /// A hook of `.claude/settings.json` runs it, and will follow it.
    pub hooked: bool,
}

impl Found {
    pub fn is_version(&self) -> bool {
        self.name != self.path
    }
}

pub fn gather(config: &Config, repos: &[PathBuf]) -> Result<Vec<Found>> {
    let mut out: Vec<(Found, Option<String>)> = Vec::new();
    for repo in repos {
        for c in candidates(config, repo)? {
            let hash = fsutil::hash_file(&repo.join(".claude").join(&c.path));
            match out.iter_mut().find(|(f, h)| f.path == c.path && *h == hash) {
                Some((f, _)) => {
                    f.repos.push(repo.clone());
                    f.hooked |= c.hooked;
                }
                None => out.push((
                    Found {
                        name: c.path.clone(),
                        path: c.path,
                        repos: vec![repo.clone()],
                        hooked: c.hooked,
                    },
                    hash,
                )),
            }
        }
    }
    let mut found: Vec<Found> = out.into_iter().map(|(f, _)| f).collect();
    let paths: Vec<String> = found.iter().map(|f| f.path.clone()).collect();
    for f in &mut found {
        if paths.iter().filter(|p| **p == f.path).count() > 1 {
            f.name = version_name(&f.path, &f.repos[0]);
        }
    }
    found.sort_by(|a, b| a.path.cmp(&b.path).then(a.name.cmp(&b.name)));
    Ok(found)
}

/// `rules/git.md` from `~/code/app` becomes `rules/app-git.md`.
pub fn version_name(path: &str, repo: &Path) -> String {
    let owner = repo
        .file_name()
        .map_or_else(|| "repo".into(), |n| n.to_string_lossy().into_owned());
    match path.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/{owner}-{file}"),
        None => format!("{owner}-{path}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;
    use std::fs;

    #[test]
    fn same_files_count_once_and_versions_take_their_repository_name() {
        let tmp = TempDir::new("gather");
        let (a, b) = (tmp.path().join("app"), tmp.path().join("lib"));
        for (repo, rel, text) in [
            (&a, "rules/git.md", "same"),
            (&b, "rules/git.md", "same"),
            (&a, "rules/rust.md", "short lines"),
            (&b, "rules/rust.md", "no unwrap"),
            (&b, "agents/scout.md", "scout"),
        ] {
            let path = repo.join(".claude").join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        let config = Config::load(&tmp.path().join("home")).unwrap();
        let found = gather(&config, &[a.clone(), b.clone()]).unwrap();
        let names: Vec<(&str, usize)> = found
            .iter()
            .map(|f| (f.name.as_str(), f.repos.len()))
            .collect();
        assert_eq!(
            names,
            [
                ("agents/scout.md", 1),
                ("rules/git.md", 2),
                ("rules/app-rust.md", 1),
                ("rules/lib-rust.md", 1),
            ]
        );
        assert!(found[2].is_version() && !found[1].is_version());
    }
}
