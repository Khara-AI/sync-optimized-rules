//! Paths as people type and read them: `~/…`, tab completion, folders.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use sync_optimized_rules::discover;
use sync_optimized_rules::error::{IoContext, Result};
use sync_optimized_rules::fsutil;

use super::Session;

fn homes() -> Vec<PathBuf> {
    let Some(home) = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
    else {
        return Vec::new();
    };
    // Paths sor shows are canonical: a home behind a symlink needs both forms.
    match fs::canonicalize(&home) {
        Ok(real) if real != home => vec![home, real],
        _ => vec![home],
    }
}

/// `~/…` for paths under the home folder.
pub fn short(path: &Path) -> String {
    let rest = homes()
        .into_iter()
        .find_map(|h| path.strip_prefix(h).ok().map(fsutil::slash));
    match rest {
        Some(rest) if rest.is_empty() => "~".into(),
        Some(rest) => format!("~/{rest}"),
        None => path.display().to_string(),
    }
}

pub fn expand(text: &str) -> PathBuf {
    match (text.strip_prefix('~'), homes().into_iter().next()) {
        (Some(rest), Some(h)) if rest.is_empty() || rest.starts_with('/') => {
            h.join(rest.trim_start_matches('/'))
        }
        _ => PathBuf::from(text),
    }
}

/// Complete the last part of a folder path, the way a shell does.
pub fn complete(typed: &str) -> (String, Vec<String>) {
    let (dir, part) = match typed.rfind('/') {
        Some(i) => typed.split_at(i + 1),
        None => ("", typed),
    };
    let base = if dir.is_empty() {
        expand(".")
    } else {
        expand(dir)
    };
    let mut names: Vec<String> = fs::read_dir(&base)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .filter(|n| n.starts_with(part) && (part.starts_with('.') || !n.starts_with('.')))
        .collect();
    names.sort();
    match names.as_slice() {
        [] => (typed.to_owned(), Vec::new()),
        [one] => (format!("{dir}{one}/"), Vec::new()),
        many => {
            let common = many.iter().skip(1).fold(many[0].clone(), |acc, n| {
                acc.chars()
                    .zip(n.chars())
                    .take_while(|(a, b)| a == b)
                    .map(|(a, _)| a)
                    .collect()
            });
            let mut shown: Vec<String> = many.iter().take(12).cloned().collect();
            if many.len() > 12 {
                shown.push(format!("+{}", many.len() - 12));
            }
            (format!("{dir}{common}"), shown)
        }
    }
}

/// The current folder, or its parent when it is itself a repository: the
/// folder where its sibling repositories are.
pub fn start_folder() -> PathBuf {
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if discover::is_repo(&cwd) {
        cwd.parent().map_or(cwd.clone(), Path::to_path_buf)
    } else {
        cwd
    }
}

impl Session<'_> {
    /// A folder typed with tab completion; `need` tells what it must hold.
    pub(super) fn ask_folder(
        &self,
        title: &str,
        start: &Path,
        need: Option<&str>,
    ) -> Result<Option<PathBuf>> {
        let mut default = short(start);
        if !default.ends_with('/') {
            default.push('/');
        }
        let Some(text) = self.ui.folder(
            title,
            &default,
            |s| match need {
                _ if !expand(s).is_dir() => Err("not a folder".into()),
                Some(sub) if !expand(s).join(sub).is_dir() => {
                    Err(format!("no {sub}/ folder there"))
                }
                _ => Ok(()),
            },
            complete,
        )?
        else {
            return Ok(None);
        };
        let path = expand(&text);
        Ok(Some(fs::canonicalize(&path).at(&path)?))
    }
}
