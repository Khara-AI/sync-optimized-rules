//! The user's configuration: groups, their members and what they share.
//!
//! It lives in `<config home>/config.json` and starts empty: sor ships no
//! one's groups. The reference copy of each scope lives next to it, in
//! `<config home>/groups/<group>/<scope>/` (see `store`).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::discover;
use crate::error::{Error, IoContext, Result};
use crate::fsutil;
use crate::store::Store;

mod sharing;

pub use sharing::Planned;

const FILE: &str = "config.json";
const VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    version: u32,
    #[serde(default)]
    pub groups: BTreeMap<String, Group>,
    #[serde(skip)]
    root: PathBuf,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Group {
    pub description: String,
    pub members: Vec<Member>,
    /// Scopes of this group installed in every member.
    pub scopes: Vec<String>,
    /// Whether the `agents/` of its scopes are installed. Off by default:
    /// agents are optimisation, not safety rules.
    pub agents: bool,
    /// Scopes a given repository opts out of.
    pub exclude: BTreeMap<PathBuf, Vec<String>>,
    /// Scopes received from other groups, by source group.
    pub imports: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub path: PathBuf,
    /// A folder whose repositories are discovered, rather than one repository.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub workspace: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<usize>,
}

pub const DEFAULT_DEPTH: usize = 2;

/// Folders sor keeps for itself under `.claude/sor/`, next to group folders.
const RESERVED: [&str; 2] = ["trash", "backup"];

impl Config {
    /// Load the configuration under `root`, or an empty one if there is none yet.
    pub fn load(root: &Path) -> Result<Self> {
        let file = root.join(FILE);
        let mut config = match fs::read_to_string(&file) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| Error::Json(file.clone(), e))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self {
                version: VERSION,
                groups: BTreeMap::new(),
                root: PathBuf::new(),
            },
            Err(e) => return Err(Error::Io(file, e)),
        };
        config.root = root.to_path_buf();
        Ok(config)
    }

    /// Write atomically: a crash never leaves a half-written configuration.
    pub fn save(&self) -> Result<()> {
        fsutil::write_json(&self.root.join(FILE), self)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn group(&self, name: &str) -> Result<&Group> {
        self.groups.get(name).ok_or_else(|| unknown(name))
    }

    fn group_mut(&mut self, name: &str) -> Result<&mut Group> {
        self.groups.get_mut(name).ok_or_else(|| unknown(name))
    }

    pub fn add_group(&mut self, name: &str, description: &str) -> Result<()> {
        check_name("group", name)?;
        if RESERVED.contains(&name) {
            return Err(Error::invalid(format!("'{name}' is reserved by sor")));
        }
        if self.groups.contains_key(name) {
            return Err(Error::invalid(format!("group '{name}' already exists")));
        }
        let group = Group {
            description: description.to_owned(),
            ..Group::default()
        };
        self.groups.insert(name.to_owned(), group);
        Ok(())
    }

    /// Forget a group. Its scopes stay in the store: nothing is deleted.
    pub fn remove_group(&mut self, name: &str) -> Result<()> {
        self.groups.remove(name).ok_or_else(|| unknown(name))?;
        for group in self.groups.values_mut() {
            group.imports.remove(name);
        }
        Ok(())
    }

    pub fn add_member(
        &mut self,
        group: &str,
        path: &Path,
        workspace: bool,
        depth: Option<usize>,
    ) -> Result<PathBuf> {
        let path = fs::canonicalize(path).at(path)?;
        if !path.is_dir() {
            return Err(Error::invalid(format!(
                "{} is not a directory",
                path.display()
            )));
        }
        if !workspace && !discover::is_repo(&path) {
            return Err(Error::invalid(format!(
                "{} is not a git repository (add it as a workspace to include the repositories inside)",
                path.display()
            )));
        }
        let g = self.group_mut(group)?;
        if g.members.iter().any(|m| m.path == path) {
            return Err(Error::invalid(format!(
                "{} is already a member of '{group}'",
                path.display()
            )));
        }
        let depth = workspace.then(|| depth.unwrap_or(DEFAULT_DEPTH));
        g.members.push(Member {
            path: path.clone(),
            workspace,
            depth,
        });
        Ok(path)
    }

    pub fn remove_member(&mut self, group: &str, path: &Path) -> Result<()> {
        let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let g = self.group_mut(group)?;
        let before = g.members.len();
        g.members.retain(|m| m.path != path);
        if g.members.len() == before {
            return Err(Error::invalid(format!(
                "{} is not a member of '{group}'",
                path.display()
            )));
        }
        g.exclude.remove(&path);
        Ok(())
    }

    /// Repositories of a group: the listed ones plus those found in its workspaces.
    pub fn repos(&self, group: &str) -> Result<Vec<PathBuf>> {
        let mut out: Vec<PathBuf> = Vec::new();
        for m in &self.group(group)?.members {
            let found = if m.workspace {
                discover::repos(&m.path, m.depth.unwrap_or(DEFAULT_DEPTH))
            } else if discover::is_repo(&m.path) {
                vec![m.path.clone()]
            } else {
                Vec::new()
            };
            for repo in found {
                if !out.contains(&repo) {
                    out.push(repo);
                }
            }
        }
        Ok(out)
    }

    pub fn store(&self) -> Store {
        Store::new(&self.root)
    }

    /// Every repository of every group.
    pub fn all_repos(&self) -> Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        for name in self.groups.keys() {
            out.extend(self.repos(name)?);
        }
        out.sort();
        out.dedup();
        Ok(out)
    }
}

fn unknown(name: &str) -> Error {
    Error::invalid(format!("unknown group '{name}'"))
}

/// Names become folder names in every member: keep them portable.
pub fn check_name(kind: &str, name: &str) -> Result<()> {
    let allowed = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_';
    let why = if name.is_empty() {
        "it cannot be empty".to_owned()
    } else if name.chars().count() > 40 {
        format!("{} characters, 40 at most", name.chars().count())
    } else if name.chars().any(|c| c.is_ascii_uppercase()) {
        "lowercase only".to_owned()
    } else if name.chars().any(char::is_whitespace) {
        "no spaces".to_owned()
    } else if let Some(c) = name.chars().find(|&c| !allowed(c)) {
        format!("'{c}' is not allowed (a-z, 0-9, '-' and '_' are)")
    } else if !name.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        "it must start with a letter or a digit".to_owned()
    } else {
        return Ok(());
    };
    let fix = suggest(name);
    let hint = if fix.is_empty() {
        String::new()
    } else {
        format!(": try '{fix}'")
    };
    Err(Error::invalid(format!(
        "invalid {kind} name '{name}', {why}{hint}"
    )))
}

/// The closest valid name: lowercase, other characters turned into '-'.
fn suggest(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_matches(['-', '_']).chars().take(40).collect();
    out.trim_end_matches(['-', '_']).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn starts_empty_and_round_trips() {
        let tmp = TempDir::new("config");
        let mut config = Config::load(tmp.path()).unwrap();
        assert!(config.groups.is_empty());
        config.add_group("web", "web projects").unwrap();
        config.save().unwrap();
        let again = Config::load(tmp.path()).unwrap();
        assert_eq!(again.group("web").unwrap().description, "web projects");
    }

    #[test]
    fn members_are_repos_or_workspaces() {
        let tmp = TempDir::new("members");
        fs::create_dir_all(tmp.path().join("ws/app/.git")).unwrap();
        fs::create_dir_all(tmp.path().join("ws/lib/.git")).unwrap();
        fs::create_dir_all(tmp.path().join("plain")).unwrap();
        let mut config = Config::load(&tmp.path().join("home")).unwrap();
        config.add_group("web", "").unwrap();
        assert!(
            config
                .add_member("web", &tmp.path().join("plain"), false, None)
                .is_err()
        );
        config
            .add_member("web", &tmp.path().join("ws"), true, None)
            .unwrap();
        config
            .add_member("web", &tmp.path().join("ws/app"), false, None)
            .unwrap();
        let repos = config.repos("web").unwrap();
        assert_eq!(repos.len(), 2, "a repository listed twice counts once");
    }

    #[test]
    fn removing_a_group_drops_imports_from_it() {
        let tmp = TempDir::new("imports");
        let mut config = Config::load(tmp.path()).unwrap();
        config.add_group("a", "").unwrap();
        config.add_group("b", "").unwrap();
        config
            .group_mut("b")
            .unwrap()
            .imports
            .insert("a".into(), vec!["commits".into()]);
        config.remove_group("a").unwrap();
        assert!(config.group("b").unwrap().imports.is_empty());
    }

    #[test]
    fn rejects_unportable_names() {
        assert!(check_name("group", "khara_ai-2").is_ok());
        for bad in ["", "Web", "-x", "a b", "a/b", &"x".repeat(41)] {
            assert!(check_name("group", bad).is_err(), "{bad:?}");
        }
        let msg = check_name("group", "Khara-AI").unwrap_err().to_string();
        assert!(msg.contains("lowercase only: try 'khara-ai'"), "{msg}");
        let msg = check_name("scope", "pure code!").unwrap_err().to_string();
        assert!(msg.contains("no spaces: try 'pure-code'"), "{msg}");
    }
}
