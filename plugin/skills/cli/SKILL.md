---
name: cli
description: Install sor in the user's own terminal, next to this Claude Code plugin.
disable-model-invocation: true
allowed-tools: Bash(sor *)
---

# Install sor in your terminal

The plugin's `sor` only exists for Claude. This puts the same version in the user's own terminal, for the keyboard menus (`sor` with no arguments).

1. Run `sor install-cli --json`.
   - `done: true`: sor is installed. Give the `message` line: where it went, and whether its folder must be added to `PATH` (on Windows, a new terminal picks it up).
   - `already installed`: the user's terminal already has sor, nothing to do. `/sor:update` keeps it current.
   - A failure: give its message and the manual way, for their system:
     - Linux and macOS: `curl -fsSL https://github.com/Khara-AI/sync-optimized-rules/releases/latest/download/install.sh | sh`
     - Windows: `irm https://github.com/Khara-AI/sync-optimized-rules/releases/latest/download/install.ps1 | iex`
2. Tell the user both share the same configuration: what they set up with `/sor:sor` is already in the terminal menus.

Speak the user's language.
