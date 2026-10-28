---
name: update
description: Update sor and its Claude Code plugin to the latest release.
disable-model-invocation: true
allowed-tools: Bash(sor *)
---

# Update sor

1. Run `sor update --json`. It updates, each only when a newer release exists:
   - the `sor` command: the one in the user's terminal when it is installed there, else the copy the plugin downloaded;
   - every installed copy of this plugin, with its skills, through `claude plugin update`.
2. Report one line per step: what moved from which version to which, or that it was already up to date. If a step failed, give its message and the manual way:
   - `sor`: run the install command again (`curl -fsSL https://github.com/Khara-AI/sync-optimized-rules/releases/latest/download/install.sh | sh`, or `irm https://github.com/Khara-AI/sync-optimized-rules/releases/latest/download/install.ps1 | iex` on Windows);
   - the plugin: `/plugin marketplace update sync-optimized-rules`, then `/plugin update sor@sync-optimized-rules`.
3. When the plugin was updated, tell the user to run `/reload-plugins` (or restart Claude Code) so the new skills load.

Speak the user's language.
