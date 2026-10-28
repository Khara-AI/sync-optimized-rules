//! `sor update`: the binary to the latest release, and the Claude Code plugin
//! (with its skills) when it is installed. `sor install-cli`: the plugin's copy
//! of sor into the user's own terminal.

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde::Serialize;
use serde_json::{Value, json};
use sync_optimized_rules::capture;
use sync_optimized_rules::error::Result;

const REPO: &str = "Khara-AI/sync-optimized-rules";
const PLUGIN: &str = "sor@";

#[derive(Serialize)]
struct Step {
    what: String,
    done: bool,
    message: String,
}

pub(super) fn run(json: bool, dry_run: bool) -> Result<()> {
    let mut steps = vec![binary(dry_run)];
    steps.extend(plugin(dry_run));
    show(json, &steps);
    Ok(())
}

/// The plugin's launcher sets this when it runs the copy it downloaded.
const PLUGIN_COPY: &str = "SOR_PLUGIN_COPY";

pub(super) fn install_cli(json: bool, dry_run: bool) -> Result<()> {
    show(json, &[terminal(dry_run)]);
    Ok(())
}

fn terminal(dry_run: bool) -> Step {
    let what = "terminal";
    let version = env!("CARGO_PKG_VERSION");
    if env::var_os(PLUGIN_COPY).is_none() {
        let at = env::current_exe().map_or_else(|_| "sor".into(), |p| p.display().to_string());
        return step(
            what,
            false,
            format!("sor {version} is already installed: {at}"),
        );
    }
    if dry_run {
        return step(what, false, format!("would install sor {version}"));
    }
    match install(None, version) {
        Ok(said) => step(what, true, said),
        Err(e) => step(what, false, format!("install of sor {version} failed: {e}")),
    }
}

fn show(json: bool, steps: &[Step]) {
    if json {
        println!("{}", json!(steps));
        return;
    }
    for s in steps {
        let mark = if s.done { "✔" } else { "·" };
        println!("{mark} {}: {}", s.what, s.message);
    }
    if steps.iter().any(|s| s.done && s.what.starts_with("plugin")) {
        println!("Restart Claude Code, or run /reload-plugins, to load the new plugin.");
    }
}

fn step(what: &str, done: bool, message: impl Into<String>) -> Step {
    Step {
        what: what.to_owned(),
        done,
        message: message.into(),
    }
}

fn binary(dry_run: bool) -> Step {
    let current = env!("CARGO_PKG_VERSION");
    let what = "sor";
    let latest = match latest_release() {
        Some(v) => v,
        None => {
            return step(
                what,
                false,
                format!("{current}, no published release found (or offline)"),
            );
        }
    };
    if !newer(&latest, current) {
        return step(what, false, format!("{current}, up to date"));
    }
    let Some(dir) = env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
        .and_then(|p| p.parent().map(Path::to_path_buf))
    else {
        return step(what, false, "cannot tell where sor is installed");
    };
    if dry_run {
        return step(
            what,
            false,
            format!("{current} → {latest} in {}", dir.display()),
        );
    }
    // A running .exe cannot be overwritten, but it can be renamed.
    let (exe, old) = (dir.join("sor.exe"), dir.join("sor.old.exe"));
    if cfg!(windows)
        && let Err(e) = std::fs::rename(&exe, &old)
    {
        return step(what, false, format!("update to {latest} failed: {e}"));
    }
    match install(Some(&dir), &latest) {
        Ok(_) => step(what, true, format!("{current} → {latest}")),
        Err(e) => {
            if cfg!(windows) {
                let _ = std::fs::rename(&old, &exe);
            }
            step(what, false, format!("update to {latest} failed: {e}"))
        }
    }
}

/// The version the latest release tag names, from the redirect GitHub answers.
fn latest_release() -> Option<String> {
    let url = format!("https://github.com/{REPO}/releases/latest");
    let out = Command::new("curl")
        .args(["-fsSIL", "-o", null(), "-w", "%{url_effective}", &url])
        .output()
        .ok()?;
    out.status.success().then_some(())?;
    tag_version(&String::from_utf8_lossy(&out.stdout))
}

fn tag_version(url: &str) -> Option<String> {
    let tag = url.trim().rsplit_once("/tag/")?.1;
    let v = tag.strip_prefix('v').unwrap_or(tag);
    parse(v).map(|_| v.to_owned())
}

fn parse(v: &str) -> Option<Vec<u64>> {
    v.split('.').map(|n| n.parse().ok()).collect()
}

fn newer(latest: &str, current: &str) -> bool {
    matches!((parse(latest), parse(current)), (Some(l), Some(c)) if l > c)
}

/// Run the release's own install script, into `dir` or its default folder.
/// Returns what the script said last (where sor went, how to reach it).
fn install(dir: Option<&Path>, version: &str) -> std::result::Result<String, String> {
    let base = format!("https://github.com/{REPO}/releases/download/v{version}");
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("powershell");
        c.args([
            "-NoProfile",
            "-Command",
            &format!("irm {base}/install.ps1 | iex"),
        ]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", &format!("curl -fsSL {base}/install.sh | sh")]);
        c
    };
    if let Some(dir) = dir {
        cmd.env("SOR_INSTALL_DIR", dir);
    }
    let out = cmd
        .env("SOR_VERSION", version)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(last_line(&out));
    }
    let said = String::from_utf8_lossy(&out.stdout);
    Ok(said
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("; "))
}

/// Every installed copy of the sor plugin, through the `claude` command.
fn plugin(dry_run: bool) -> Vec<Step> {
    let Some(claude) = capture::claude() else {
        return vec![step("plugin", false, "claude not found, nothing to update")];
    };
    let listed = Command::new(&claude)
        .args(["plugin", "list", "--json"])
        .output();
    let installed: Vec<Value> = match listed {
        Ok(out) if out.status.success() => serde_json::from_slice::<Vec<Value>>(&out.stdout)
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p["id"].as_str().is_some_and(|id| id.starts_with(PLUGIN)))
            .collect(),
        _ => return vec![step("plugin", false, "claude plugin list failed")],
    };
    if installed.is_empty() {
        return vec![step("plugin", false, "not installed")];
    }
    installed
        .iter()
        .map(|p| {
            let id = p["id"].as_str().unwrap_or_default();
            let scope = p["scope"].as_str().unwrap_or("user");
            let what = format!("plugin {id}");
            let from = p["version"].as_str().unwrap_or("?");
            if dry_run {
                return step(
                    &what,
                    false,
                    format!("{from}, would refresh its marketplace and update"),
                );
            }
            match update_plugin(&claude, id, scope) {
                Ok((done, message)) => step(&what, done, message),
                Err(e) => step(&what, false, e),
            }
        })
        .collect()
}

fn update_plugin(
    claude: &PathBuf,
    id: &str,
    scope: &str,
) -> std::result::Result<(bool, String), String> {
    let market = id.split_once('@').map_or("", |(_, m)| m);
    let refresh = Command::new(claude)
        .args(["plugin", "marketplace", "update", market])
        .output()
        .map_err(|e| e.to_string())?;
    if !refresh.status.success() {
        return Err(format!("marketplace {market}: {}", last_line(&refresh)));
    }
    let out = Command::new(claude)
        .args(["plugin", "update", id, "--scope", scope, "--json"])
        .output()
        .map_err(|e| e.to_string())?;
    let answer: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    let (old, new) = (
        answer["oldVersion"].as_str().unwrap_or("?"),
        answer["newVersion"].as_str().unwrap_or("?"),
    );
    match answer["updateOutcome"].as_str() {
        Some("updated") => Ok((true, format!("{old} → {new}"))),
        Some("up_to_date") => Ok((false, format!("{new}, up to date"))),
        _ if out.status.success() => Ok((false, last_line(&out))),
        _ => Err(answer["message"]
            .as_str()
            .map_or_else(|| last_line(&out), str::to_owned)),
    }
}

fn last_line(out: &Output) -> String {
    let text = [&out.stdout, &out.stderr]
        .iter()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    text.lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("no output")
        .trim()
        .to_owned()
}

fn null() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_come_from_the_release_tag_and_compare_by_number() {
        let url = "https://github.com/Khara-AI/sync-optimized-rules/releases/tag/v0.10.2\n";
        assert_eq!(tag_version(url).as_deref(), Some("0.10.2"));
        assert_eq!(tag_version("https://github.com/x/releases"), None);
        assert!(newer("0.10.2", "0.9.9"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1.0", "0.2.0"), "never a downgrade");
    }
}
