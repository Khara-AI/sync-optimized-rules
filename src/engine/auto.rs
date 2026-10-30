//! Auto-sync: promote plain local edits, then sync every member.
//!
//! Only what a person would settle the same way every time is automatic. A
//! rule edited here while the group's version did not move is promoted; if
//! both moved, it stays a conflict. Support files run as hooks in the other
//! members, so they are never promoted without asking.

use std::collections::BTreeSet;
use std::fs::{self, TryLockError};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde::Serialize;

use super::{FileState, Resolution, classify, resolve, run, targets};
use crate::config::Config;
use crate::error::{Error, IoContext, Result};
use crate::fsutil;
use crate::repo::State;
use crate::store::Area;

const LOCK: &str = "sync.lock";

/// Held while sor writes to the store or a repository, so that two of them
/// never mix their writes. The system releases it even if the process is killed.
pub struct Lock {
    _file: fs::File,
}

impl Lock {
    /// Take the lock, or `None` if another sor holds it.
    pub fn try_take(root: &Path) -> Result<Option<Self>> {
        fs::create_dir_all(root).at(root)?;
        let path = root.join(LOCK);
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .at(&path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(e)) => Err(Error::Io(path, e)),
        }
    }

    /// Wait for the lock: a command someone typed should not give up at once.
    pub fn wait(root: &Path) -> Result<Self> {
        for _ in 0..150 {
            if let Some(lock) = Self::try_take(root)? {
                return Ok(lock);
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(Error::invalid(
            "another sor is still writing: try again when it is done",
        ))
    }
}

#[derive(Debug, Serialize)]
pub struct AutoFile {
    pub repo: PathBuf,
    pub file: String,
}

#[derive(Debug, Default, Serialize)]
pub struct AutoReport {
    /// Local edits that became the group's version.
    pub promoted: Vec<AutoFile>,
    /// Files installed, updated or moved aside by the sync.
    pub updated: Vec<AutoFile>,
    /// Conflicts and setting clashes left for the user.
    pub pending: Vec<AutoFile>,
    /// Repositories that could not be synced, with the reason.
    pub failed: Vec<AutoFile>,
}

/// Rules and agents of auto groups `repo` is a member of, edited there while the group's version
/// stayed what sor last wrote there.
fn promote(config: &Config, repo: &Path, out: &mut Vec<AutoFile>) -> Result<()> {
    let state = State::load(repo)?;
    for t in targets(&config.store(), &config.plan(repo)?) {
        let auto = config.group(&t.group).is_ok_and(|g| g.auto);
        if !auto || t.area == Area::Files || state.files.get(&t.path) != Some(&t.want) {
            continue;
        }
        let cur = fsutil::hash_file(&repo.join(&t.path));
        // A repository that only borrows the scope receives it, it does not write to it.
        if classify(&t, cur.as_deref(), &state) == FileState::Modified
            && config.repos(&t.group)?.iter().any(|r| r == repo)
        {
            resolve(config, repo, &t.path, Resolution::Promote)?;
            out.push(AutoFile {
                repo: repo.to_path_buf(),
                file: t.path,
            });
        }
    }
    Ok(())
}

/// Run one auto-sync. `None` if another sor is already syncing: the next
/// trigger picks up whatever it missed.
pub fn auto(config: &Config) -> Result<Option<AutoReport>> {
    let mut report = AutoReport::default();
    let repos: BTreeSet<PathBuf> = config.auto_repos()?.into_iter().collect();
    if repos.is_empty() {
        return Ok(Some(report));
    }
    let Some(_lock) = Lock::try_take(config.root())? else {
        return Ok(None);
    };
    let fail = |repo: &Path, e: Error| AutoFile {
        repo: repo.to_path_buf(),
        file: e.to_string(),
    };
    // Every promotion first, so that one pass brings every member up to date.
    let mut usable = Vec::new();
    for repo in &repos {
        match promote(config, repo, &mut report.promoted) {
            Ok(()) => usable.push(repo),
            Err(e) => report.failed.push(fail(repo, e)),
        }
    }
    for repo in usable {
        let r = match run(config, repo, true) {
            Ok(r) => r,
            Err(e) => {
                report.failed.push(fail(repo, e));
                continue;
            }
        };
        let entry = |file: String| AutoFile {
            repo: repo.clone(),
            file,
        };
        for f in r.files {
            if f.state.is_conflict() {
                report.pending.push(entry(f.path));
            } else if matches!(
                f.state,
                FileState::Missing | FileState::Outdated | FileState::Stale
            ) {
                report.updated.push(entry(f.path));
            }
        }
        report
            .pending
            .extend(r.settings.clash.into_iter().map(entry));
    }
    Ok(Some(report))
}
