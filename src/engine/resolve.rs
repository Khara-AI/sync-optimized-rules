//! Settling a conflict on one file: keep, take or promote.

use std::path::{Path, PathBuf};

use super::{BACKUP, FileState, Target, classify, targets};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::fsutil;
use crate::repo::State;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// Keep the local file until the group's version changes again.
    Keep,
    /// Install the group's version; the local file goes to `backup/`.
    Take,
    /// Make the local file the group's version, for every member.
    Promote,
}

impl Resolution {
    pub fn parse(word: &str) -> Result<Self> {
        match word {
            "keep" => Ok(Self::Keep),
            "take" => Ok(Self::Take),
            "promote" => Ok(Self::Promote),
            _ => Err(Error::invalid(format!(
                "unknown resolution '{word}': use keep, take or promote"
            ))),
        }
    }
}

fn target(config: &Config, repo: &Path, file: &str) -> Result<(String, Target)> {
    let file = file.replace('\\', "/");
    let file = Path::new(&file)
        .strip_prefix(repo)
        .map_or(file.clone(), fsutil::slash);
    let t = targets(&config.store(), &config.plan(repo)?)
        .into_iter()
        .find(|t| t.path == file)
        .ok_or_else(|| {
            Error::invalid(format!(
                "{file} is not a file sor installs in {}",
                repo.display()
            ))
        })?;
    Ok((file, t))
}

/// The group's copy of a file sor installs in `repo`.
pub fn group_copy(config: &Config, repo: &Path, file: &str) -> Result<PathBuf> {
    Ok(target(config, repo, file)?.1.src)
}

pub fn resolve(config: &Config, repo: &Path, file: &str, how: Resolution) -> Result<String> {
    let store = config.store();
    let (file, t) = target(config, repo, file)?;
    let t = &t;
    let mut state = State::load(repo)?;
    let dest = repo.join(&t.path);
    let cur = fsutil::hash_file(&dest);
    let st = classify(t, cur.as_deref(), &state);
    let Some(cur) = cur.filter(|_| st.is_conflict() || st == FileState::Kept) else {
        return Err(Error::invalid(format!(
            "nothing to resolve: {file} is {}",
            st.as_str()
        )));
    };
    let done = match how {
        Resolution::Keep => {
            state.kept.insert(t.path.clone(), t.want.clone());
            format!("kept the local {file}")
        }
        Resolution::Take => {
            let moved = fsutil::relocate(repo, &t.path, BACKUP, &fsutil::stamp())?;
            fsutil::copy_atomic(&t.src, &dest)?;
            state.files.insert(t.path.clone(), t.want.clone());
            state.kept.remove(&t.path);
            format!("installed the group's {file}; yours is in {moved}")
        }
        Resolution::Promote => {
            store.put(&t.group, &t.scope, t.area, &t.rel, &dest)?;
            state.files.insert(t.path.clone(), cur);
            state.kept.remove(&t.path);
            format!(
                "{file} is now the version of {}; other members get it at their next sync",
                t.label()
            )
        }
    };
    state.save(repo)?;
    Ok(done)
}
