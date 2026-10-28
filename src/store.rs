//! Scopes on disk: the reference copy each group installs in its members.
//!
//! `<config home>/groups/<group>/<scope>/` holds `scope.json` (description and
//! settings fragment) and up to three areas: `rules/`, `files/`, `agents/`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{Error, IoContext, Result};
use crate::fsutil;

const META: &str = "scope.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Area {
    Rules,
    Files,
    Agents,
}

impl Area {
    pub const ALL: [Area; 3] = [Area::Rules, Area::Files, Area::Agents];

    pub fn dir(self) -> &'static str {
        match self {
            Area::Rules => "rules",
            Area::Files => "files",
            Area::Agents => "agents",
        }
    }

    /// Where the area lands in a repository, namespaced by group and scope.
    pub fn install_dir(self, group: &str, scope: &str) -> String {
        match self {
            Area::Rules => format!(".claude/rules/{group}/{scope}"),
            Area::Files => format!(".claude/sor/{group}/{scope}"),
            Area::Agents => format!(".claude/agents/{group}/{scope}"),
        }
    }

    /// Area and inner path of a file under a repository's `.claude/`.
    pub fn of(rel_in_claude: &str) -> (Area, &str) {
        if let Some(rest) = rel_in_claude.strip_prefix("rules/") {
            (Area::Rules, rest)
        } else if let Some(rest) = rel_in_claude.strip_prefix("agents/") {
            (Area::Agents, rest)
        } else {
            (Area::Files, rel_in_claude)
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Meta {
    pub description: String,
    /// Merged into `.claude/settings.local.json`; `{{dir}}` stands for the
    /// scope's installed `files/` folder.
    pub settings: Map<String, Value>,
}

pub struct StoreFile {
    pub area: Area,
    pub rel: String,
    pub path: PathBuf,
}

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(config_home: &Path) -> Self {
        Self {
            root: config_home.join("groups"),
        }
    }

    pub fn dir(&self, group: &str, scope: &str) -> PathBuf {
        self.root.join(group).join(scope)
    }

    pub fn exists(&self, group: &str, scope: &str) -> bool {
        self.dir(group, scope).join(META).is_file()
    }

    pub fn scopes(&self, group: &str) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.root.join(group)) else {
            return Vec::new();
        };
        let mut out: Vec<String> = entries
            .flatten()
            .filter(|e| e.path().join(META).is_file())
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .collect();
        out.sort();
        out
    }

    pub fn meta(&self, group: &str, scope: &str) -> Result<Meta> {
        let file = self.dir(group, scope).join(META);
        match fsutil::read_json(&file)? {
            Some(v) => serde_json::from_value(v).map_err(|e| Error::Json(file, e)),
            None => Err(Error::invalid(format!(
                "scope '{scope}' does not exist in group '{group}'"
            ))),
        }
    }

    pub fn save_meta(&self, group: &str, scope: &str, meta: &Meta) -> Result<()> {
        fsutil::write_json(&self.dir(group, scope).join(META), meta)
    }

    pub fn files(&self, group: &str, scope: &str) -> Vec<StoreFile> {
        let dir = self.dir(group, scope);
        Area::ALL
            .iter()
            .flat_map(|&area| {
                let base = dir.join(area.dir());
                fsutil::files_under(&base)
                    .into_iter()
                    .map(move |rel| StoreFile {
                        area,
                        path: base.join(&rel),
                        rel,
                    })
            })
            .collect()
    }

    /// Put `src` in the store; an existing different version moves to `history/`.
    pub fn put(&self, group: &str, scope: &str, area: Area, rel: &str, src: &Path) -> Result<()> {
        let dest = self.dir(group, scope).join(area.dir()).join(rel);
        if dest.is_file() && fsutil::hash_file(&dest) != fsutil::hash_file(src) {
            self.archive(group, scope, &format!("{}/{rel}", area.dir()))?;
        }
        fsutil::copy_atomic(src, &dest)
    }

    /// Move a store file to `<config home>/history/<group>/<scope>/<time>/`.
    pub fn archive(&self, group: &str, scope: &str, rel: &str) -> Result<()> {
        let home = self.root.parent().unwrap_or(&self.root);
        let dest = home
            .join("history")
            .join(group)
            .join(scope)
            .join(fsutil::stamp())
            .join(rel);
        if let Some(dir) = dest.parent() {
            fs::create_dir_all(dir).at(dir)?;
        }
        let src = self.dir(group, scope).join(rel);
        fs::copy(&src, &dest).at(&dest)?;
        Ok(())
    }
}
