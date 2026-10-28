//! What sor keeps inside a member repository: its state file and its block in
//! `.git/info/exclude`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::fsutil;
use crate::settings::Item;

pub const STATE: &str = ".claude/sor/state.json";
pub const SETTINGS: &str = ".claude/settings.local.json";
const BEGIN: &str = "# >>> sync-optimized-rules";
const END: &str = "# <<< sync-optimized-rules";

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// Hash of every file sor wrote, by repository-relative path.
    pub files: BTreeMap<String, String>,
    /// Files the user chose to keep, with the group version they declined.
    pub kept: BTreeMap<String, String>,
    /// Settings items sor added to settings.local.json.
    pub settings: Vec<Item>,
}

impl State {
    pub fn load(repo: &Path) -> Result<Self> {
        let file = repo.join(STATE);
        match fsutil::read_json(&file)? {
            Some(v) => serde_json::from_value(v).map_err(|e| Error::Json(file, e)),
            None => Ok(Self::default()),
        }
    }

    pub fn save(&self, repo: &Path) -> Result<()> {
        fsutil::write_json(&repo.join(STATE), self)
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.kept.is_empty() && self.settings.is_empty()
    }
}

/// The exclude file git uses for this repository, worktrees included.
fn exclude_file(repo: &Path) -> Option<PathBuf> {
    let dot_git = repo.join(".git");
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else {
        let text = fs::read_to_string(&dot_git).ok()?;
        let dir = repo.join(text.strip_prefix("gitdir:")?.trim());
        match fs::read_to_string(dir.join("commondir")) {
            Ok(common) => dir.join(common.trim()),
            Err(_) => dir,
        }
    };
    Some(git_dir.join("info").join("exclude"))
}

/// Rewrite sor's marked block so its files never reach a commit. Returns
/// whether the block changes; `apply: false` only checks.
pub fn update_exclude(repo: &Path, entries: &[String], apply: bool) -> Result<bool> {
    let Some(file) = exclude_file(repo) else {
        return Ok(false);
    };
    let text = match fs::read_to_string(&file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(Error::Io(file, e)),
    };
    let block = std::iter::once(BEGIN.to_owned())
        .chain(entries.iter().cloned())
        .chain(std::iter::once(END.to_owned()))
        .collect::<Vec<_>>()
        .join("\n");
    let new = match (text.find(BEGIN), text.find(END)) {
        (Some(b), Some(e)) if b < e => format!("{}{block}{}", &text[..b], &text[e + END.len()..]),
        _ if text.is_empty() || text.ends_with('\n') => format!("{text}{block}\n"),
        _ => format!("{text}\n{block}\n"),
    };
    if new == text || !apply {
        return Ok(new != text);
    }
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).at(dir)?;
    }
    fsutil::write_atomic(&file, new.as_bytes())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn exclude_block_is_replaced_in_place() {
        let tmp = TempDir::new("exclude");
        fs::create_dir_all(tmp.path().join(".git/info")).unwrap();
        let file = tmp.path().join(".git/info/exclude");
        fs::write(&file, "*.log\n").unwrap();
        assert!(update_exclude(tmp.path(), &["/a/".into()], true).unwrap());
        assert!(update_exclude(tmp.path(), &["/b/".into()], false).unwrap());
        assert!(update_exclude(tmp.path(), &["/b/".into()], true).unwrap());
        assert!(!update_exclude(tmp.path(), &["/b/".into()], true).unwrap());
        let text = fs::read_to_string(file).unwrap();
        assert_eq!(text, format!("*.log\n{BEGIN}\n/b/\n{END}\n"));
    }
}
