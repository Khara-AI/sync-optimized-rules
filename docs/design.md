# Design

## Concepts

- **Group**: a set of repositories that share rules. Members are repositories, or workspace folders whose repositories are discovered (default depth 2, nested checkouts belong to their parent).
- **Scope**: one coherent set of rules, e.g. `anonymity`, `commits`, `rust`. A scope has a kind: `rules` (safety, conventions) or `agents` (optimisation, installed only when the group sets `agents: true`).
- **Sharing**: a group lists the scopes it installs in every member. A repository can exclude scopes. A group can import single scopes from another group without the two groups merging.

A fresh install has no group and no scope. The interactive setup builds the user's own configuration: discover repositories, capture the rules a repository already has as scopes, or start from generic templates.

## On disk

Configuration home (`$SOR_HOME`, else `~/.config/sync-optimized-rules`, `%APPDATA%` on Windows):

```
config.json                     groups, members, scopes, exclusions, imports
groups/<group>/<scope>/
  scope.json                    description, kind, settings fragment
  rules/**                      -> .claude/rules/<group>/<scope>/
  files/**                      -> .claude/sor/<group>/<scope>/   (hook scripts, term lists)
  agents/**                     -> .claude/agents/<group>/<scope>/  (opt-in)
history/<group>/<scope>/<time>/ previous versions replaced by a promote
```

In each member repository:

```
.claude/rules/<group>/<scope>/  installed rules (namespaced: groups never collide)
.claude/sor/<group>/<scope>/    installed support files
.claude/agents/<group>/<scope>/ installed agents, when the group opts in
.claude/sor/state.json          what sor wrote: file hashes, owned settings entries
.claude/sor/trash/, backup/     files moved aside instead of deleted
.claude/settings.local.json     settings fragments merged here, entry by entry
.git/info/exclude               a marked block keeping all of the above out of git
```

Claude Code reads `.claude/rules/` and `.claude/agents/` recursively, so namespacing by folder costs nothing and lets one repository belong to several groups. Agents are identified by their `name` field, not their path: two groups shipping the same agent name is reported as a conflict.

## No overwrite

For each target file, comparing its current content with the group's version and with what sor wrote last time gives its state:

| State | Meaning | Sync does |
|---|---|---|
| missing | not there | install |
| ok | identical to the group's | nothing |
| outdated | still what sor wrote, group changed | update |
| modified | edited since sor wrote it | report a conflict |
| conflict | a file sor never wrote is in the way | report a conflict |
| kept | the user chose to keep a local version | nothing, until the group's version changes |

Conflicts are resolved per file: **keep** the local file, **take** the group's (the local one goes to `backup/`), or **promote** the local file to the group (the previous version goes to the store history; every member gets it at its next sync).

Settings fragments are flattened into entries: list items are added to the list if absent, scalars are only set when the key is absent (otherwise a clash is reported). sor records the entries it added and removes only those when a scope leaves; entries the user wrote are never touched.

## Interfaces

All of them are clients of the same library:

- `sor` CLI: commands for scripts, `--json` for machines, and an interactive mode (bare `sor`, `sor setup`) with keyboard menus.
- Claude Code plugin: a `/sor:setup` skill that drives the CLI and shows groups and drift as tables in the chat, and a `SessionStart` hook that warns when the current repository drifted from its groups.
- Editor extension (later, TypeScript): a view of groups and conflicts over `sor --json`.

## Code layout

```
src/main.rs        entry point
src/cli.rs         argument parsing and rendering
src/lib.rs         library root
src/config.rs      groups and members
src/discover.rs    repository discovery
src/paths.rs       config home
```

To come, as the features land: `store` (scopes on disk), `repo` (per-repository state and exclude block), `settings` (fragment merge), `engine` (status, sync, resolve) and `cli/interactive`.
