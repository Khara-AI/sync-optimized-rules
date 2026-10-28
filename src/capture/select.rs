//! Which files of `.claude/` a capture takes.

use std::fs;
use std::path::{Path, PathBuf};

use super::Found;
use crate::config::Config;
use crate::error::{Error, IoContext, Result};
use crate::fsutil;

/// Files named by `paths`, relative to `repo/.claude/`. Folders expand to
/// their files; sor's own files, settings and CLAUDE.md are never taken.
pub(super) fn files(config: &Config, repo: &Path, paths: &[PathBuf]) -> Result<Vec<String>> {
    let claude = repo.join(".claude");
    let mut out: Vec<String> = Vec::new();
    for p in paths {
        let abs = if p.is_absolute() {
            p.clone()
        } else {
            repo.join(p)
        };
        let abs = fs::canonicalize(&abs).at(&abs)?;
        let rel = abs.strip_prefix(&claude).map(fsutil::slash).map_err(|_| {
            Error::invalid(format!(
                "{} is not under {}",
                abs.display(),
                claude.display()
            ))
        })?;
        let found: Vec<String> = if abs.is_dir() {
            fsutil::files_under(&abs)
                .into_iter()
                .map(|f| {
                    if rel.is_empty() {
                        f
                    } else {
                        format!("{rel}/{f}")
                    }
                })
                .filter(|f| !owned_elsewhere(config, f))
                .collect()
        } else if owned_elsewhere(config, &rel) {
            return Err(Error::invalid(format!(
                ".claude/{rel} cannot be captured: sor manages it, or it holds settings or CLAUDE.md"
            )));
        } else {
            vec![rel]
        };
        for f in found {
            if !out.contains(&f) {
                out.push(f);
            }
        }
    }
    if out.is_empty() {
        return Err(Error::invalid("nothing to capture"));
    }
    Ok(out)
}

pub(super) fn owned_elsewhere(config: &Config, rel: &str) -> bool {
    matches!(rel, "settings.json" | "settings.local.json" | "CLAUDE.md")
        || rel.starts_with("sor/")
        || config.groups.keys().any(|g| {
            rel.starts_with(&format!("rules/{g}/")) || rel.starts_with(&format!("agents/{g}/"))
        })
}

/// Indexes of the gathered files `args` name: a version name (`rules/app-git.md`)
/// takes that version, a path (`rules/git.md`, or a folder) takes all of them.
pub(super) fn versions(found: &[Found], args: &[PathBuf]) -> Result<Vec<usize>> {
    let mut picked: Vec<usize> = Vec::new();
    for arg in args {
        let a = fsutil::slash(arg);
        let a = a.trim_start_matches("./");
        let a = a
            .strip_prefix(".claude/")
            .unwrap_or(a)
            .trim_end_matches('/');
        let exact: Vec<usize> = (0..found.len()).filter(|&i| found[i].name == a).collect();
        let matching = if exact.is_empty() {
            (0..found.len())
                .filter(|&i| {
                    let p = &found[i].path;
                    p == a || p.strip_prefix(a).is_some_and(|r| r.starts_with('/'))
                })
                .collect()
        } else {
            exact
        };
        if matching.is_empty() {
            return Err(Error::invalid(format!(
                ".claude/{a} is in none of these repositories (see sor scan)"
            )));
        }
        for i in matching {
            if !picked.contains(&i) {
                picked.push(i);
            }
        }
    }
    Ok(picked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_takes_every_version_and_a_version_name_one() {
        let f = |path: &str, name: &str| Found {
            path: path.into(),
            name: name.into(),
            repos: Vec::new(),
            hooked: false,
        };
        let found = [
            f("rules/git.md", "rules/app-git.md"),
            f("rules/git.md", "rules/lib-git.md"),
            f("rules/rust.md", "rules/rust.md"),
            f("hooks/guard.py", "hooks/guard.py"),
        ];
        let pick = |args: &[&str]| {
            let args: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
            versions(&found, &args)
        };
        assert_eq!(pick(&[".claude/rules/git.md"]).unwrap(), [0, 1]);
        assert_eq!(pick(&["rules/lib-git.md", "hooks"]).unwrap(), [1, 3]);
        assert_eq!(pick(&["rules"]).unwrap(), [0, 1, 2]);
        assert!(pick(&["rules/go.md"]).is_err());
    }
}
