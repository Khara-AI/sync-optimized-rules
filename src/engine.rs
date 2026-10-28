//! Status, sync and conflict resolution for one repository.
//!
//! `status` is a sync that applies nothing: both walk the same comparison.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::config::{Config, Planned};
use crate::error::Result;
use crate::fsutil;
use crate::repo::{self, State};
use crate::settings::{self, Item};
use crate::store::{Area, Store};

mod resolve;

pub use resolve::{Resolution, resolve};

pub const TRASH: &str = ".claude/sor/trash";
pub const BACKUP: &str = ".claude/sor/backup";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileState {
    Missing,
    Ok,
    Outdated,
    Kept,
    Modified,
    Conflict,
    Stale,
}

impl FileState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Ok => "ok",
            Self::Outdated => "outdated",
            Self::Kept => "kept",
            Self::Modified => "modified",
            Self::Conflict => "conflict",
            Self::Stale => "stale",
        }
    }

    pub fn is_conflict(self) -> bool {
        matches!(self, Self::Modified | Self::Conflict)
    }
}

#[derive(Debug, Serialize)]
pub struct FileReport {
    pub path: String,
    pub state: FileState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct SettingsReport {
    pub missing: Vec<String>,
    pub removed: Vec<String>,
    pub clash: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub repo: PathBuf,
    pub scopes: Vec<String>,
    pub files: Vec<FileReport>,
    pub settings: SettingsReport,
    /// The exclude block needs (or needed) an update.
    pub exclude: bool,
    pub applied: bool,
    /// Nothing differed from the groups before this run.
    pub clean: bool,
}

impl Report {
    /// Conflicts and clashes: what a sync cannot settle on its own.
    pub fn pending(&self) -> bool {
        self.files.iter().any(|f| f.state.is_conflict()) || !self.settings.clash.is_empty()
    }
}

struct Target {
    path: String,
    src: PathBuf,
    want: String,
    group: String,
    scope: String,
    area: Area,
    rel: String,
}

impl Target {
    fn label(&self) -> String {
        format!("{}/{}", self.group, self.scope)
    }
}

fn targets(store: &Store, plan: &[Planned]) -> Vec<Target> {
    let mut out: Vec<Target> = Vec::new();
    for p in plan {
        for f in store.files(&p.group, &p.scope) {
            if f.area == Area::Agents && !p.agents {
                continue;
            }
            let Some(want) = fsutil::hash_file(&f.path) else {
                continue;
            };
            out.push(Target {
                path: format!("{}/{}", f.area.install_dir(&p.group, &p.scope), f.rel),
                src: f.path,
                want,
                group: p.group.clone(),
                scope: p.scope.clone(),
                area: f.area,
                rel: f.rel,
            });
        }
    }
    out
}

fn fragments(store: &Store, plan: &[Planned]) -> Result<Vec<Item>> {
    let mut out: Vec<Item> = Vec::new();
    for p in plan {
        let meta = store.meta(&p.group, &p.scope)?;
        let dir = format!(
            "$CLAUDE_PROJECT_DIR/{}",
            Area::Files.install_dir(&p.group, &p.scope)
        );
        let Value::Object(map) = substitute(Value::Object(meta.settings), &dir) else {
            continue;
        };
        for item in settings::flatten(&map) {
            if !out.contains(&item) {
                out.push(item);
            }
        }
    }
    Ok(out)
}

fn substitute(v: Value, dir: &str) -> Value {
    match v {
        Value::String(s) => Value::String(s.replace("{{dir}}", dir)),
        Value::Array(list) => Value::Array(list.into_iter().map(|e| substitute(e, dir)).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, e)| (k, substitute(e, dir)))
                .collect(),
        ),
        other => other,
    }
}

fn classify(t: &Target, cur: Option<&str>, state: &State) -> FileState {
    let recorded = state.files.get(&t.path).map(String::as_str);
    match cur {
        None => FileState::Missing,
        Some(c) if c == t.want => FileState::Ok,
        Some(_) if state.kept.get(&t.path) == Some(&t.want) => FileState::Kept,
        Some(c) if recorded == Some(c) => FileState::Outdated,
        Some(_) if recorded.is_some() => FileState::Modified,
        Some(_) => FileState::Conflict,
    }
}

fn describe(item: &Item) -> String {
    format!("{} {}", item.label(), item.value)
}

/// The block in `.git/info/exclude`: sor's own folder, the merged settings
/// and the namespaced folders of every group installed here.
fn exclude_entries(plan: &[Planned]) -> Vec<String> {
    let mut groups = BTreeSet::new();
    for p in plan {
        groups.insert(format!("/.claude/rules/{}/", p.group));
        if p.agents {
            groups.insert(format!("/.claude/agents/{}/", p.group));
        }
    }
    ["/.claude/sor/".to_owned(), format!("/{}", repo::SETTINGS)]
        .into_iter()
        .chain(groups)
        .collect()
}

pub fn run(config: &Config, repo: &Path, apply: bool) -> Result<Report> {
    let store = config.store();
    let plan = config.plan(repo)?;
    let mut state = State::load(repo)?;
    let had_state = !state.is_empty();
    let targets = targets(&store, &plan);
    let stamp = fsutil::stamp();
    let mut files = Vec::new();

    for t in &targets {
        let dest = repo.join(&t.path);
        let cur = fsutil::hash_file(&dest);
        let st = classify(t, cur.as_deref(), &state);
        if apply && matches!(st, FileState::Missing | FileState::Outdated | FileState::Ok) {
            if st != FileState::Ok {
                fsutil::copy_atomic(&t.src, &dest)?;
            }
            state.files.insert(t.path.clone(), t.want.clone());
            state.kept.remove(&t.path);
        }
        files.push(FileReport {
            path: t.path.clone(),
            state: st,
            scope: Some(t.label()),
        });
    }

    let wanted: BTreeSet<&str> = targets.iter().map(|t| t.path.as_str()).collect();
    let stale: Vec<String> = state
        .files
        .keys()
        .filter(|p| !wanted.contains(p.as_str()))
        .cloned()
        .collect();
    for path in stale {
        let present = repo.join(&path).is_file();
        if present {
            files.push(FileReport {
                path: path.clone(),
                state: FileState::Stale,
                scope: None,
            });
        }
        if apply {
            if present {
                fsutil::relocate(repo, &path, TRASH, &stamp)?;
                fsutil::prune_empty(&repo.join(".claude"), &repo.join(&path));
            }
            state.files.remove(&path);
        }
    }
    if apply {
        state.kept.retain(|p, _| wanted.contains(p.as_str()));
    }

    let desired = fragments(&store, &plan)?;
    let settings_file = repo.join(repo::SETTINGS);
    let mut doc = fsutil::read_json(&settings_file)?.unwrap_or_else(|| Value::Object(Map::new()));
    let before = doc.clone();
    let mut report = SettingsReport::default();
    let mut owned = Vec::new();
    for item in state.settings.iter().filter(|i| !desired.contains(i)) {
        if settings::present(&doc, item) {
            report.removed.push(describe(item));
            settings::remove(&mut doc, item);
        }
    }
    for item in &desired {
        if settings::present(&doc, item) {
            if state.settings.contains(item) {
                owned.push(item.clone());
            }
        } else if settings::add(&mut doc, item) {
            report.missing.push(describe(item));
            owned.push(item.clone());
        } else {
            report.clash.push(describe(item));
        }
    }
    if apply {
        if doc != before {
            fsutil::write_json(&settings_file, &doc)?;
        }
        state.settings = owned;
    }

    let exclude = if plan.is_empty() && !had_state {
        false
    } else {
        repo::update_exclude(repo, &exclude_entries(&plan), apply)?
    };
    if apply && (had_state || !state.is_empty()) {
        state.save(repo)?;
    }

    let clean = files
        .iter()
        .all(|f| matches!(f.state, FileState::Ok | FileState::Kept))
        && report.missing.is_empty()
        && report.removed.is_empty()
        && report.clash.is_empty()
        && !exclude;
    Ok(Report {
        repo: repo.to_path_buf(),
        scopes: plan
            .iter()
            .map(|p| format!("{}/{}", p.group, p.scope))
            .collect(),
        files,
        settings: report,
        exclude,
        applied: apply,
        clean,
    })
}
