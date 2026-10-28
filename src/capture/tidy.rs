//! Moving originals aside once sor installs them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use serde_json::Value;

use super::{PROJECT_SETTINGS, hook_entries, select};
use crate::config::Config;
use crate::engine::BACKUP;
use crate::error::{Error, IoContext, Result};
use crate::fsutil;
use crate::settings::{self, Item};

#[derive(Debug, Default, Serialize)]
pub struct Tidy {
    pub moved: Vec<String>,
    /// Events whose hook entries left `.claude/settings.json`.
    pub hooks: Vec<String>,
    pub applied: bool,
}

/// Move originals now installed by sor to `backup/`, and drop the hook
/// entries of `.claude/settings.json` that ran them (after a backup).
pub fn tidy(config: &Config, repo: &Path, paths: &[PathBuf], apply: bool) -> Result<Tidy> {
    let repo = fs::canonicalize(repo).at(repo)?;
    let files = select::files(config, &repo, paths)?;
    let mut touched: Vec<String> = files.iter().map(|f| format!(".claude/{f}")).collect();
    let settings_file = repo.join(PROJECT_SETTINGS);
    let mut project = fsutil::read_json(&settings_file)?.unwrap_or(Value::Null);
    let pairs: Vec<(&str, &str)> = files.iter().map(|f| (f.as_str(), f.as_str())).collect();
    let entries = hook_entries(&project, &pairs);
    if !entries.is_empty() {
        touched.push(PROJECT_SETTINGS.to_owned());
    }
    refuse_tracked(&repo, &touched)?;

    let report = Tidy {
        moved: touched[..files.len()].to_vec(),
        hooks: entries.iter().map(|(e, _, _)| e.clone()).collect(),
        applied: apply,
    };
    if !apply {
        return Ok(report);
    }
    let stamp = fsutil::stamp();
    for rel in &report.moved {
        fsutil::relocate(&repo, rel, BACKUP, &stamp)?;
        fsutil::relocate_cache(&repo, rel, BACKUP, &stamp)?;
        fsutil::prune_empty(&repo.join(".claude"), &repo.join(rel));
    }
    if !entries.is_empty() {
        let saved = repo.join(BACKUP).join(&stamp).join(PROJECT_SETTINGS);
        fsutil::copy_atomic(&settings_file, &saved)?;
        for (event, original, _) in &entries {
            settings::remove(
                &mut project,
                &Item {
                    path: vec!["hooks".into(), event.clone()],
                    value: original.clone(),
                    set: false,
                },
            );
        }
        fsutil::write_json(&settings_file, &project)?;
    }
    Ok(report)
}

/// sor never moves or edits a file git tracks: that change belongs in a commit.
fn refuse_tracked(repo: &Path, paths: &[String]) -> Result<()> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["ls-files", "--"])
        .args(paths)
        .output()
        .map_err(|e| Error::Io(repo.to_path_buf(), e))?;
    if !out.status.success() {
        return Err(Error::invalid(format!(
            "git could not list the tracked files of {}",
            repo.display()
        )));
    }
    let tracked = String::from_utf8_lossy(&out.stdout);
    if tracked.trim().is_empty() {
        return Ok(());
    }
    Err(Error::invalid(format!(
        "tracked by git, change them with a commit instead: {}",
        tracked.split_whitespace().collect::<Vec<_>>().join(", ")
    )))
}
