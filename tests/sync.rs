//! End-to-end: one test per invariant of status, sync, resolve and tidy, on
//! synthetic git repositories.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs, process};

use serde_json::{Value, json};

struct Sandbox(PathBuf);

impl Sandbox {
    fn new(label: &str) -> Self {
        let dir = env::temp_dir().join(format!("sor-sync-{label}-{}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(fs::canonicalize(dir).unwrap())
    }

    fn sor(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sor"))
            .args(args)
            .env("SOR_HOME", self.0.join("home"))
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.sor(args);
        assert!(
            out.status.success(),
            "{args:?}: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn code(&self, args: &[&str]) -> i32 {
        self.sor(args).status.code().unwrap()
    }

    fn status(&self, repo: &str) -> Value {
        let out = self.sor(&["status", &self.path(repo), "--json"]);
        serde_json::from_slice::<Value>(&out.stdout).unwrap()[0].clone()
    }

    fn path(&self, rel: &str) -> String {
        self.0.join(rel).to_string_lossy().into_owned()
    }

    fn write(&self, rel: &str, text: &str) {
        let p = self.0.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.0.join(rel)).unwrap()
    }

    fn exists(&self, rel: &str) -> bool {
        self.0.join(rel).exists()
    }

    /// A git repository whose `.claude/` is ignored, like most members.
    fn repo(&self, rel: &str) {
        let dir = self.0.join(rel);
        fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
    }

    /// Group `g` with members `repos`, and scope `rules` captured from `src`.
    fn group(&self, src: &str, repos: &[&str]) {
        self.repo(src);
        self.write(&format!("{src}/.claude/rules/git.md"), "commit with care\n");
        self.write(&format!("{src}/.claude/rules/local.md"), "only here\n");
        self.write(&format!("{src}/.claude/hooks/guard.py"), "print()\n");
        let settings = json!({
            "permissions": { "allow": ["Bash(git status)"] },
            "hooks": { "PreToolUse": [{ "matcher": "Bash", "hooks": [{
                "type": "command",
                "command": "python3 \"$CLAUDE_PROJECT_DIR/.claude/hooks/guard.py\"" }] }] }
        });
        self.write(
            &format!("{src}/.claude/settings.json"),
            &settings.to_string(),
        );
        self.ok(&["group", "add", "g"]);
        for r in repos {
            self.repo(r);
            self.ok(&["member", "add", "g", &self.path(r)]);
        }
        self.ok(&[
            "scope",
            "capture",
            "g",
            "rules",
            ".claude/rules/git.md",
            ".claude/hooks",
            "--from",
            &self.path(src),
            "--permissions",
        ]);
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

const RULE: &str = "a/.claude/rules/g/rules/git.md";

#[test]
fn sync_installs_only_the_captured_scope_and_hides_it_from_git() {
    let sb = Sandbox::new("install");
    sb.group("src", &["a"]);
    assert_eq!(sb.code(&["status", &sb.path("a")]), 2);
    sb.ok(&["sync", &sb.path("a")]);

    assert_eq!(sb.read(RULE), "commit with care\n");
    assert!(sb.exists("a/.claude/sor/g/rules/hooks/guard.py"));
    assert!(
        !sb.exists("a/.claude/rules/g/rules/local.md"),
        "files not captured stay in their repository"
    );
    let local: Value = serde_json::from_str(&sb.read("a/.claude/settings.local.json")).unwrap();
    assert_eq!(local["permissions"]["allow"], json!(["Bash(git status)"]));
    assert_eq!(
        local["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "python3 \"$CLAUDE_PROJECT_DIR/.claude/sor/g/rules/hooks/guard.py\""
    );
    let exclude = sb.read("a/.git/info/exclude");
    assert!(exclude.contains("/.claude/rules/g/") && exclude.contains("/.claude/sor/"));

    assert_eq!(sb.status("a")["clean"], true);
    assert_eq!(sb.code(&["status", &sb.path("a")]), 0);
}

#[test]
fn local_edits_are_never_overwritten() {
    let sb = Sandbox::new("edits");
    sb.group("src", &["a", "b"]);
    sb.ok(&["sync"]);
    sb.write(RULE, "my version\n");
    assert_eq!(sb.status("a")["files"][0]["state"], "modified");

    assert_eq!(sb.code(&["sync", &sb.path("a")]), 2);
    assert_eq!(sb.read(RULE), "my version\n");

    sb.ok(&[
        "resolve",
        &sb.path("a"),
        ".claude/rules/g/rules/git.md",
        "promote",
    ]);
    assert_eq!(sb.status("b")["files"][0]["state"], "outdated");
    sb.ok(&["sync", &sb.path("b")]);
    assert_eq!(sb.read("b/.claude/rules/g/rules/git.md"), "my version\n");
    let history = fs::read_dir(sb.0.join("home/history/g/rules")).unwrap();
    assert_eq!(history.count(), 1, "the replaced version is kept");
}

#[test]
fn a_file_sor_did_not_write_is_a_conflict_until_resolved() {
    let sb = Sandbox::new("conflict");
    sb.group("src", &["a"]);
    sb.write(RULE, "already there\n");
    assert_eq!(sb.status("a")["files"][0]["state"], "conflict");
    assert_eq!(sb.code(&["sync", &sb.path("a")]), 2);
    assert_eq!(sb.read(RULE), "already there\n");

    sb.ok(&[
        "resolve",
        &sb.path("a"),
        RULE.trim_start_matches("a/"),
        "keep",
    ]);
    assert_eq!(sb.status("a")["clean"], true);

    sb.ok(&[
        "resolve",
        &sb.path("a"),
        RULE.trim_start_matches("a/"),
        "take",
    ]);
    assert_eq!(sb.read(RULE), "commit with care\n");
    let backup = sb.0.join("a/.claude/sor/backup");
    assert_eq!(fs::read_dir(backup).unwrap().count(), 1);
}

#[test]
fn unsharing_removes_only_what_sor_added() {
    let sb = Sandbox::new("unshare");
    sb.group("src", &["a"]);
    sb.write(
        "a/.claude/settings.local.json",
        r#"{ "permissions": { "allow": ["Bash(ls)", "Bash(git status)"] } }"#,
    );
    sb.ok(&["sync", &sb.path("a")]);
    sb.ok(&["group", "unshare", "g", "rules"]);
    assert_eq!(sb.status("a")["files"][0]["state"], "stale");
    sb.ok(&["sync", &sb.path("a")]);

    assert!(!sb.exists(RULE));
    assert_eq!(
        fs::read_dir(sb.0.join("a/.claude/sor/trash"))
            .unwrap()
            .count(),
        1
    );
    let local: Value = serde_json::from_str(&sb.read("a/.claude/settings.local.json")).unwrap();
    assert_eq!(
        local["permissions"]["allow"],
        json!(["Bash(ls)", "Bash(git status)"]),
        "entries the user had before sor stay"
    );
    assert!(local.get("hooks").is_none());
}

#[test]
fn agents_need_the_group_to_opt_in_and_repos_can_opt_out() {
    let sb = Sandbox::new("agents");
    sb.group("src", &["a", "b"]);
    sb.write(
        "src/.claude/agents/reviewer.md",
        "---\nname: reviewer\n---\n",
    );
    sb.ok(&[
        "scope",
        "capture",
        "g",
        "review",
        ".claude/agents",
        "--from",
        &sb.path("src"),
    ]);
    sb.ok(&["sync"]);
    assert!(!sb.exists("a/.claude/agents/g/review/reviewer.md"));

    sb.ok(&["group", "agents", "g", "on"]);
    sb.ok(&["group", "exclude", "g", &sb.path("b"), "review"]);
    sb.ok(&["sync"]);
    assert!(sb.exists("a/.claude/agents/g/review/reviewer.md"));
    assert!(!sb.exists("b/.claude/agents/g/review/reviewer.md"));
}

#[test]
fn tidy_moves_originals_aside_but_never_tracked_files() {
    let sb = Sandbox::new("tidy");
    sb.group("src", &["a"]);
    let src = sb.path("src");
    sb.ok(&["member", "add", "g", &src]);
    sb.ok(&["sync", &src]);
    assert_eq!(sb.code(&["sync", &src]), 0);

    sb.ok(&[
        "tidy",
        &src,
        ".claude/rules/git.md",
        ".claude/hooks",
        "--dry-run",
    ]);
    assert!(sb.exists("src/.claude/hooks/guard.py"));
    sb.ok(&["tidy", &src, ".claude/rules/git.md", ".claude/hooks"]);
    assert!(!sb.exists("src/.claude/hooks"));
    assert!(sb.exists("src/.claude/rules/local.md"));
    let project: Value = serde_json::from_str(&sb.read("src/.claude/settings.json")).unwrap();
    assert!(project.get("hooks").is_none());
    assert_eq!(project["permissions"]["allow"], json!(["Bash(git status)"]));

    git(&sb.0.join("src"), &["add", "-f", ".claude/rules/local.md"]);
    let out = sb.sor(&["tidy", &src, ".claude/rules/local.md"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("tracked by git"));
    assert!(sb.exists("src/.claude/rules/local.md"));
}
