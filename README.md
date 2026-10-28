# sync-optimized-rules

`sor` keeps the Claude Code rules of many repositories in sync. You gather repositories into groups, choose which scopes each group shares (anonymity, commit hygiene, language conventions…), and `sor` installs them in every member, locally and without overwriting anything you wrote by hand.

> Early stage: the format of the configuration may still change. See [docs/design.md](docs/design.md).

## Install

Pick the way you install, then unfold it. Every way ends with the same `sor` command.

<details>
<summary><b>Linux and macOS</b> (one-line script)</summary>

```sh
curl -fsSL https://github.com/Khara-AI/sync-optimized-rules/releases/latest/download/install.sh | sh
```

The script downloads the latest release for your system into `~/.local/bin`. If that folder is not on your `PATH`, it tells you to add it.

- **Update:** run the same command again.
- **Uninstall:** `curl -fsSL https://github.com/Khara-AI/sync-optimized-rules/releases/latest/download/install.sh | sh -s -- --uninstall`

Check it works: `sor --version`.
</details>

<details>
<summary><b>Windows</b> (PowerShell)</summary>

```powershell
irm https://github.com/Khara-AI/sync-optimized-rules/releases/latest/download/install.ps1 | iex
```

- **Update:** run the same command again.
- **Uninstall:** `$env:SOR_UNINSTALL = 1` then the same command.

Check it works in a new terminal: `sor --version`.
</details>

<details>
<summary><b>Claude Code plugin</b> (Claude runs <code>sor</code> for you)</summary>

In Claude Code:

```
/plugin marketplace add Khara-AI/sync-optimized-rules
/plugin install sor@sync-optimized-rules
```

The plugin puts a `sor` command at Claude's disposal. It uses the `sor` on your `PATH` when there is one, else downloads the latest release on first use. You never build anything.

Then ask Claude in plain words, for example *"with sor, make a group web with ~/code/app and ~/code/api, capture the git rules of app into a scope commits, and sync"*. Claude drives the commands of [Use it from commands](#use-it-from-commands): the keyboard menus need a real terminal, so inside Claude Code everything goes through commands.

- **Update:** `/plugin marketplace update sync-optimized-rules`
- **Uninstall:** `/plugin uninstall sor@sync-optimized-rules`

To try a local checkout instead of the published plugin: `/plugin marketplace add /path/to/sync-optimized-rules`.
</details>

<details>
<summary><b>From source</b> (Rust toolchain)</summary>

```sh
cargo install --git https://github.com/Khara-AI/sync-optimized-rules --locked
```

- **Update:** the same command with `--force`.
- **Uninstall:** `cargo uninstall sync-optimized-rules`
</details>

Uninstalling never touches your configuration in `~/.config/sync-optimized-rules` nor what `sor` installed in your repositories: run `sor group remove` and `sor sync` first if you want them gone.

## Use it with the menus

Run `sor` in a terminal. Arrows move, space ticks, enter confirms, esc goes back, tab completes a folder.

<details>
<summary><b>First setup, step by step</b></summary>

1. **+ New group**: a name (lowercase, digits, `-` and `_`; if it is wrong, `sor` says why and suggests a fix) and an optional description.
2. **Repositories**: pick a folder, tick the repositories found inside, and add more folders as needed.
3. **Capture rules**: tick the repositories to take rules from. Their `.claude/` files are listed; tick the ones to share.
   - Identical files count once. When repositories hold **different versions** of a file, each version is kept under its repository's name (`rules/app-git.md`, `rules/api-git.md`), and `sor` offers to merge them with Claude Code if `claude` is installed.
   - Share the files in **one scope** or split them into **several scopes** (for instance `anonymity` and `conventions`).
   - Hooks of `.claude/settings.json` that run a captured file follow it.
   - `sor` then asks whether to share the **permissions** (`allow`, `deny`, `ask` of `.claude/settings.json`). They go to every repository of the group: `deny` and `ask` only restrict Claude, so they are offered by default; `allow` grants rights everywhere, so it is your call.
4. **Install them and move the originals aside?** Yes installs the scopes in the source repositories and moves their own copies to `.claude/sor/backup/`, so each rule exists once.
5. **Sync**: a line per repository says what will change; *Show the details* lists it file by file. Apply, and every member gets the scopes.
</details>

<details>
<summary><b>Day to day</b></summary>

- **Edit a shared rule**: edit it in any repository, then *Check and sync*: `sor` sees the change and offers to *promote* it to the group. Or edit it in the store, `~/.config/sync-optimized-rules/groups/<group>/<scope>/`.
- **Conflicts**: a file you edited, or one that was there before `sor`, is never overwritten. *Check and sync* shows the difference and lets you keep yours, take the group's (yours goes to backup) or promote yours.
- **Exceptions**: a repository can skip a scope of its group.
- **Borrowed scopes**: a group can use one scope of another group without sharing everything.
- **Agents**: off by default; turn them on per group to share the agents of its scopes too.
- **Forget this group**: what it installed goes to `.claude/sor/trash/` at the next sync; its scopes stay in the store.
</details>

## Use it from commands

Every step of the menus exists as a command, for scripts and for Claude. Every command accepts `--json`; `sor --help` lists them all.

<details>
<summary><b>The same first setup, from commands</b></summary>

```sh
# 1. A group and its repositories
sor group add web --description "web projects"
sor member add web ~/code/app                     # one repository
sor member add web ~/code/api
sor member add web ~/code/clients --workspace     # every repository inside a folder

# 2. Capture rules of one repository into scopes (paths are relative to it)
sor scope capture web anonymity .claude/rules/anonymity.md .claude/hooks/guard.py --from ~/code/app
sor scope capture web conventions .claude/rules/git.md .claude/rules/style.md --from ~/code/app --permissions

# 3. Look, then install everywhere
sor status            # exit code 2 when something differs
sor sync --dry-run
sor sync

# 4. Move the originals of the source repository aside (copies in .claude/sor/backup/)
sor tidy ~/code/app .claude/rules/anonymity.md .claude/hooks/guard.py .claude/rules/git.md .claude/rules/style.md
```

`--permissions` also shares the `allow`, `deny` and `ask` rules of the source repository. `--move` on `scope capture` does the tidying right away; tidying after the sync, as above, means Claude never goes without the rules.

Capturing from several repositories at once, with the versions merged by Claude, is only in the menus for now.
</details>

<details>
<summary><b>Day to day, from commands</b></summary>

```sh
sor resolve ~/code/app .claude/rules/web/conventions/git.md keep|take|promote
sor group exclude web ~/code/api anonymity     # this repository skips a scope
sor group include web ~/code/api anonymity
sor group import web shared-group licensing    # borrow one scope of another group
sor group agents web on
sor group unshare web conventions              # stop installing a scope
sor member remove web ~/code/api
sor group remove web                           # then sor sync to take its files away
```
</details>

Your configuration lives in `~/.config/sync-optimized-rules` (or `$SOR_HOME`) and is yours alone. See [docs/design.md](docs/design.md) for how files are compared and merged.

## Principles

- **Local.** Nothing leaves your machine; installed files are kept out of git through `.git/info/exclude`.
- **No overwrite.** Each group installs into its own namespace. A file you edited becomes a conflict you resolve (keep yours, take the group's, or promote yours to the group), never a silent replacement. Retired files are moved aside, not deleted.
- **Scopes, not everything.** A group shares the scopes you pick; a repository can opt out of one; groups can lend each other single scopes.
- **Agents are opt-in.** Rules carry safety and conventions; agents are optimisation and are only shared when a group asks for them.

## License

Copyright 2026 Khara-AI. Licensed under the [Apache License 2.0](LICENSE).
