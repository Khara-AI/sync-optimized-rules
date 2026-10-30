//! File helpers shared by the store and the engine: hashing, atomic writes,
//! moving files aside instead of deleting them.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::error::{Error, IoContext, Result};

/// FNV-1a: detects changes, not tampering, so a cryptographic hash is not needed.
pub fn hash(data: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

pub fn hash_file(path: &Path) -> Option<String> {
    fs::read(path).ok().map(|d| hash(&d))
}

pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).at(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".sor-tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, data).at(&tmp)?;
    fs::rename(&tmp, path).at(path)
}

/// Copy `src` to `dest` atomically, keeping its permissions (hook scripts).
pub fn copy_atomic(src: &Path, dest: &Path) -> Result<()> {
    write_atomic(dest, &fs::read(src).at(src)?)?;
    let perms = fs::metadata(src).at(src)?.permissions();
    fs::set_permissions(dest, perms).at(dest)
}

/// Move `root/rel` to `root/<bin>/<stamp>/rel` and return the new relative path.
pub fn relocate(root: &Path, rel: &str, bin: &str, stamp: &str) -> Result<String> {
    let moved = format!("{bin}/{stamp}/{rel}");
    let dest = root.join(&moved);
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir).at(dir)?;
    }
    fs::rename(root.join(rel), &dest).at(&dest)?;
    Ok(moved)
}

/// Move aside the `__pycache__` that would be all that is left next to a
/// moved script, so its folder can go.
pub fn relocate_cache(root: &Path, rel: &str, bin: &str, stamp: &str) -> Result<()> {
    let Some(dir) = Path::new(rel).parent() else {
        return Ok(());
    };
    let only_cache = fs::read_dir(root.join(dir))
        .map(|mut it| it.all(|e| e.is_ok_and(|e| e.file_name() == "__pycache__")))
        .unwrap_or(false);
    let cache = dir.join("__pycache__");
    if only_cache && root.join(&cache).is_dir() {
        relocate(root, &slash(&cache), bin, stamp)?;
    }
    Ok(())
}

/// Remove the folders `file` leaves empty, up to `stop` (excluded).
pub fn prune_empty(stop: &Path, file: &Path) {
    let mut dir = file.parent();
    while let Some(d) = dir {
        if d == stop || !d.starts_with(stop) || fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}

/// Files under `dir`, as sorted '/'-separated relative paths.
pub fn files_under(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        let Ok(kind) = e.file_type() else { continue };
        if kind.is_dir() {
            if e.file_name() != "__pycache__" {
                walk(base, &path, out);
            }
        } else if kind.is_file()
            && let Ok(rel) = path.strip_prefix(base)
        {
            out.push(slash(rel));
        }
    }
}

pub fn slash(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn read_json(path: &Path) -> Result<Option<Value>> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| Error::Json(path.to_path_buf(), e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::Io(path.to_path_buf(), e)),
    }
}

pub fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let text =
        serde_json::to_string_pretty(value).map_err(|e| Error::Json(path.to_path_buf(), e))?;
    write_atomic(path, (text + "\n").as_bytes())
}

/// UTC time as `YYYYMMDD-HHMMSS`, for folders of moved-aside files.
pub fn stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil(days as i64);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 to a calendar date (Howard Hinnant's algorithm).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(19_782), (2024, 2, 29));
        assert_eq!(civil(20_754), (2026, 10, 28));
    }

    #[test]
    fn hash_changes_with_content() {
        assert_eq!(hash(b"a"), hash(b"a"));
        assert_ne!(hash(b"a"), hash(b"b"));
    }
}
