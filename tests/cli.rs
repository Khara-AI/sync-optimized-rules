//! End-to-end: the `sor` binary against a synthetic config home and workspace.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs, process};

struct Sandbox(PathBuf);

impl Sandbox {
    fn new(label: &str) -> Self {
        let dir = env::temp_dir().join(format!("sor-e2e-{label}-{}", process::id()));
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
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn path(&self, rel: &str) -> String {
        self.0.join(rel).to_string_lossy().into_owned()
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn mkrepo(root: &Path, rel: &str) {
    fs::create_dir_all(root.join(rel).join(".git")).unwrap();
}

#[test]
fn fresh_install_has_no_groups() {
    let sb = Sandbox::new("fresh");
    assert!(sb.ok(&["group", "list"]).contains("No group yet"));
    assert_eq!(sb.ok(&["group", "list", "--json"]).trim(), "[]");
}

#[test]
fn group_with_workspace_member_lists_its_repos() {
    let sb = Sandbox::new("workspace");
    mkrepo(&sb.0, "ws/example-app");
    mkrepo(&sb.0, "ws/libs/web-shop");
    sb.ok(&["group", "add", "web", "--description", "web projects"]);
    sb.ok(&["member", "add", "web", &sb.path("ws"), "--workspace"]);

    let repos: Vec<String> = serde_json::from_str(&sb.ok(&["repos", "web", "--json"])).unwrap();
    assert_eq!(
        repos,
        [sb.path("ws/example-app"), sb.path("ws/libs/web-shop")]
    );

    let groups: serde_json::Value =
        serde_json::from_str(&sb.ok(&["group", "list", "--json"])).unwrap();
    assert_eq!(groups[0]["name"], "web");
    assert_eq!(groups[0]["agents"], false, "agents are opt-in");
}

#[test]
fn errors_are_reported_without_touching_the_config() {
    let sb = Sandbox::new("errors");
    fs::create_dir_all(sb.0.join("plain")).unwrap();
    sb.ok(&["group", "add", "web"]);
    let out = sb.sor(&["member", "add", "web", &sb.path("plain")]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a git repository"));
    assert!(!sb.sor(&["group", "add", "Bad Name"]).status.success());
    assert_eq!(sb.ok(&["repos", "web", "--json"]).trim(), "[]");
}
