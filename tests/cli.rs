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

#[test]
fn the_capture_menus_work_from_commands_too() {
    let sb = Sandbox::new("commands");
    for (repo, rel, text) in [
        ("ws/app", "rules/git.md", "commit small"),
        ("ws/lib", "rules/git.md", "commit with tests"),
        ("ws/app", "rules/style.md", "short lines"),
        ("ws/lib", "rules/style.md", "short lines"),
        (
            "ws/app",
            "settings.json",
            r#"{"permissions":{"deny":["Bash(git push *)"]}}"#,
        ),
    ] {
        mkrepo(&sb.0, repo);
        let path = sb.0.join(repo).join(".claude").join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    sb.ok(&["group", "add", "web"]);

    let found: serde_json::Value =
        serde_json::from_str(&sb.ok(&["find", &sb.path("ws"), "--json"])).unwrap();
    assert_eq!(found[0]["name"], "app");
    assert_eq!(
        found[0]["files"], 2,
        "settings.json is not a file to capture"
    );

    let (app, lib) = (sb.path("ws/app"), sb.path("ws/lib"));
    let scan: serde_json::Value =
        serde_json::from_str(&sb.ok(&["scan", &app, &lib, "--json"])).unwrap();
    let names: Vec<&str> = scan["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["rules/app-git.md", "rules/lib-git.md", "rules/style.md"]
    );
    assert_eq!(scan["permissions"][0]["deny"], 1);

    for repo in [&app, &lib] {
        sb.ok(&["member", "add", "web", repo]);
    }
    let from = ["--from", app.as_str(), "--from", lib.as_str()];
    let mut args = vec!["scope", "capture", "web", "conventions", "rules"];
    args.extend(from);
    args.push("--permissions");
    sb.ok(&args);

    let merged = sb.0.join("merged.md");
    fs::write(&merged, "commit small, with tests").unwrap();
    sb.ok(&[
        "scope",
        "merge",
        "web",
        "conventions",
        "rules/git.md",
        "rules/app-git.md",
        "rules/lib-git.md",
        "--file",
        &merged.to_string_lossy(),
    ]);
    sb.ok(&["sync"]);
    let installed = sb.0.join("ws/lib/.claude/rules/web/conventions");
    assert_eq!(
        fs::read_to_string(installed.join("git.md")).unwrap(),
        "commit small, with tests\n"
    );
    assert!(installed.join("style.md").is_file());
    assert!(!installed.join("app-git.md").exists());
    let local = fs::read_to_string(sb.0.join("ws/lib/.claude/settings.local.json")).unwrap();
    assert!(local.contains("git push"), "permissions follow: {local}");
}
