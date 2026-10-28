//! Finding git repositories inside a workspace folder.

use std::fs;
use std::path::{Path, PathBuf};

const SKIP: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "venv",
    "__pycache__",
];

pub fn is_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// Git repositories under `root`, down to `depth` levels. A repository's own
/// subfolders are not searched: nested checkouts belong to their parent.
pub fn repos(root: &Path, depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(root, depth, &mut found);
    found
}

fn walk(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if is_repo(dir) {
        found.push(dir.to_path_buf());
        return;
    }
    if depth == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut children: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| !n.starts_with('.') && !SKIP.contains(&n))
        })
        .collect();
    children.sort();
    for child in children {
        walk(&child, depth - 1, found);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn finds_repos_within_depth_and_skips_nested_and_hidden() {
        let tmp = TempDir::new("discover");
        for p in [
            "a/.git",
            "a/inner/.git",
            "group/b/.git",
            "deep/x/y/.git",
            ".hidden/c/.git",
            "node_modules/d/.git",
        ] {
            fs::create_dir_all(tmp.path().join(p)).unwrap();
        }
        let found: Vec<_> = repos(tmp.path(), 2)
            .iter()
            .map(|p| p.strip_prefix(tmp.path()).unwrap().to_path_buf())
            .collect();
        assert_eq!(found, [Path::new("a"), &Path::new("group").join("b")]);
    }
}
