//! Hook entries of `.claude/settings.json` that run captured files.

use serde_json::Value;

use crate::store::Area;

/// Hook entries whose command runs one of `files` (path under `.claude/`,
/// name in the scope), as (event, entry, entry with the path rewritten to the
/// scope's installed folder).
pub(super) fn hook_entries(project: &Value, files: &[(&str, &str)]) -> Vec<(String, Value, Value)> {
    let Some(hooks) = project["hooks"].as_object() else {
        return Vec::new();
    };
    let files: Vec<&(&str, &str)> = files
        .iter()
        .filter(|(f, _)| Area::of(f).0 == Area::Files)
        .collect();
    let mut out = Vec::new();
    for (event, list) in hooks {
        for entry in list.as_array().into_iter().flatten() {
            let text = entry.to_string();
            let mut rewritten = text.clone();
            for (f, name) in &files {
                for var in ["$CLAUDE_PROJECT_DIR", "${CLAUDE_PROJECT_DIR}"] {
                    rewritten = rewritten.replace(
                        &format!("{var}/.claude/{f}"),
                        &format!("{{{{dir}}}}/{name}"),
                    );
                }
            }
            if rewritten != text {
                if let Ok(v) = serde_json::from_str(&rewritten) {
                    out.push((event.clone(), entry.clone(), v));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hook_paths_are_rewritten_to_the_scope_folder() {
        let project = json!({ "hooks": { "PreToolUse": [
            { "matcher": "Bash", "hooks": [{ "type": "command",
              "command": "python3 \"$CLAUDE_PROJECT_DIR/.claude/hooks/guard.py\"" }] },
            { "matcher": "Edit", "hooks": [{ "type": "command", "command": "other" }] }
        ] } });
        let found = hook_entries(&project, &[("hooks/guard.py", "hooks/guard.py")]);
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].2["hooks"][0]["command"],
            "python3 \"{{dir}}/hooks/guard.py\""
        );
    }
}
