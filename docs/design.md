# Design

## Concepts

- **Group**: a set of repositories that share rules. Members are repositories, or workspace folders whose repositories are discovered (default depth 2, nested checkouts belong to their parent).
- **Scope**: one coherent set of rules, e.g. `anonymity`, `commits`, `rust`: rule files, support files (hook scripts, term lists), a settings fragment, and optionally agents. Agents are optimisation rather than safety, so they are installed only in groups that set `agents: true`.
- **Sharing**: a group lists the scopes it installs in every member. A repository can exclude scopes. A group can import single scopes from another group without the two groups merging.

A fresh install has no group and no scope. The user builds their own: `sor scope capture` turns files a repository already has under `.claude/` into a scope (the hooks of `.claude/settings.json` that run those files follow, rewritten to the installed path; `--permissions` also takes its allow/deny/ask lists). The interactive setup drives the same library: it can gather files from several repositories at once (identical copies count once; each different version of a path is kept as `<folder>/<repository>-<file>`, its hooks rewritten to that name) and split them across several scopes. Versions can then be merged by `claude -p` with no tools; the merged file takes the plain name, the versions go to `history/` and their hook entries point at the merged file.

## On disk

Configuration home (`$SOR_HOME`, else `~/.config/sync-optimized-rules`, `%APPDATA%` on Windows):

```
config.json                     groups, members, scopes, exclusions, imports
groups/<group>/<scope>/
  scope.json                    description, settings fragment
  rules/**                      -> .claude/rules/<group>/<scope>/
  files/**                      -> .claude/sor/<group>/<scope>/   (hook scripts, term lists)
  agents/**                     -> .claude/agents/<group>/<scope>/  (opt-in)
history/<group>/<scope>/<time>/ previous versions replaced by a promote or a new capture
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

Claude Code reads `.claude/rules/` and `.claude/agents/` recursively, so namespacing by folder costs nothing and lets one repository belong to several groups. Agents are identified by their `name` field, not their path: give agents of different groups different names (detecting duplicates is planned).

In settings fragments, `{{dir}}` stands for the scope's installed support folder, `$CLAUDE_PROJECT_DIR/.claude/sor/<group>/<scope>`.

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
| stale | sor wrote it, no scope installs it any more | move it to `trash/` |

Conflicts are resolved per file: **keep** the local file, **take** the group's (the local one goes to `backup/`), or **promote** the local file to the group (the previous version goes to the store history; every member gets it at its next sync).

Settings fragments are flattened into entries: list items are added to the list if absent, scalars are only set when the key is absent (otherwise a clash is reported). sor records the entries it added and removes only those when a scope leaves; entries the user wrote are never touched.

`sor status` runs the same comparison without applying anything and exits with 2 when anything differs; `sor sync` exits with 2 when conflicts or clashes remain.

## Tidying originals

After a capture, the repository it came from has its original files next to the copies sor installs. `sor tidy <repo> <path>...` (or `capture --move`) moves the originals to `.claude/sor/backup/` and removes from `.claude/settings.json` the hook entries that ran them, after copying it to the same backup. It is the one command that edits `settings.json`, only on request, and it refuses files git tracks: those change through a commit.

## Interfaces

All of them are clients of the same library:

- `sor` CLI: commands for scripts, `--json` for machines, and an interactive mode (bare `sor`, `sor setup`) with keyboard menus.
- Claude Code plugin: `bin/sor` (the `sor` on the user's `PATH`, else a release downloaded on first use) and a `/sor:sor` skill that asks the menus' questions with Claude's question tool and drives the CLI. Long lists are written out with their paths and answered in free text. `sor find`, `sor scan`, `sor scope capture` with several `--from`, `sor scope merge --file` and `sor diff` give it everything the menus see. The terminal install stays the main way; the plugin is for those who start from Claude, and both share one configuration. `sor update` (and the plugin's `/sor:update`) installs the latest release where `sor` runs from, through the release's own install script, then updates every installed copy of the plugin with `claude plugin update`. Claude Code only updates a plugin whose version changed, so `plugin.json` follows the version of the crate (a test checks it). A `SessionStart` hook that warns when the current repository drifted from its groups is still to come.
- Editor extension (later, TypeScript): a view of groups and conflicts over `sor --json`.

## Code layout

```
src/main.rs          entry point
src/cli.rs           argument parsing and dispatch
src/cli/render.rs    human and JSON output
src/cli/tui.rs       terminal: raw keys, widgets drawn in place
src/cli/tui/widget/  lists, text fields, yes/no (pure state, tested without a terminal)
src/cli/interactive/ menus: groups, members, capture, scopes, sync and conflicts
src/lib.rs           library root
src/config.rs        groups and members
src/config/sharing.rs  shared scopes, exclusions, imports, plan per repository
src/store.rs         scopes on disk
src/capture.rs       capture, and what a repository could capture
src/capture/gather.rs files of several repositories, versions named apart
src/capture/merge.rs merging versions with Claude Code
src/capture/hooks.rs hook entries that run captured files
src/capture/tidy.rs  moving originals aside
src/engine.rs        status and sync
src/engine/resolve.rs  keep, take, promote
src/repo.rs          per-repository state and exclude block
src/settings.rs      settings fragment merge
src/fsutil.rs        hashing, atomic writes, moving aside
src/discover.rs      repository discovery
src/paths.rs         config home
```

To come: the plugin's `SessionStart` hook.
