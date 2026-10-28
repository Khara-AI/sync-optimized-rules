# sync-optimized-rules

`sor` keeps the Claude Code rules of many repositories in sync. You gather repositories into groups, choose which scopes each group shares (anonymity, commit hygiene, language conventions…), and `sor` installs them in every member, locally and without overwriting anything you wrote by hand.

> Early stage: groups and members work; scopes, sync and the interactive setup are being built. See [docs/design.md](docs/design.md).

## Install

Linux and macOS:

```sh
curl -fsSL https://github.com/Khara-AI/sync-optimized-rules/releases/latest/download/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://github.com/Khara-AI/sync-optimized-rules/releases/latest/download/install.ps1 | iex
```

As a Claude Code plugin (downloads the binary on first use):

```
/plugin marketplace add Khara-AI/sync-optimized-rules
/plugin install sor@sync-optimized-rules
```

From source: `cargo install --git https://github.com/Khara-AI/sync-optimized-rules`.

### Update

Run the install command again: it always fetches the latest release. For the plugin, `/plugin marketplace update sync-optimized-rules`.

### Uninstall

```sh
curl -fsSL https://github.com/Khara-AI/sync-optimized-rules/releases/latest/download/install.sh | sh -s -- --uninstall
```

On Windows, `$env:SOR_UNINSTALL = 1` before the install command. For the plugin, `/plugin uninstall sor@sync-optimized-rules`. Your configuration in `~/.config/sync-optimized-rules` is left in place.

## Usage

```sh
sor group add web --description "web projects"
sor member add web ~/code/example-app            # one repository
sor member add web ~/code/clients --workspace    # every repository inside
sor repos web
sor group list --json
```

A fresh install has no groups: your configuration lives in `~/.config/sync-optimized-rules` (or `$SOR_HOME`) and is yours alone.

## Principles

- **Local.** Nothing leaves your machine; installed files are kept out of git through `.git/info/exclude`.
- **No overwrite.** Each group installs into its own namespace. A file you edited becomes a conflict you resolve (keep yours, take the group's, or promote yours to the group), never a silent replacement. Retired files are moved aside, not deleted.
- **Scopes, not everything.** A group shares the scopes you pick; a repository can opt out of one; groups can lend each other single scopes.
- **Agents are opt-in.** Rules carry safety and conventions; agents are optimisation and are only shared when a group asks for them.

## License

Copyright 2026 Khara-AI. Licensed under the [Apache License 2.0](LICENSE).
