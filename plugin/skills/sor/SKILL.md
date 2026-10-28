---
name: sor
description: Set up and run sor (sync-optimized-rules), which keeps the Claude Code rules of many repositories in sync by groups and scopes. Use when the user wants to share rules, hooks or permissions between repositories, create or change a sor group, capture rules into a scope, sync, or settle a sor conflict.
argument-hint: "[what you want to do, or nothing for the menu]"
allowed-tools: Bash(sor *) Bash(type -a sor)
---

# sor, guided by Claude

You walk the user through what the `sor` keyboard menus do, asking the same questions with your own question tool. Every change goes through `sor` commands: never edit the store (`~/.config/sync-optimized-rules`) or the files sor installed by hand.

Always pass `--json` when you read state, and read `sor --help` once if a command fails. Speak the user's language.

## Asking

- **Up to 4 choices**: ask with AskUserQuestion. One option per choice: the label is the short name (repository folder, file name, scope), the description the full path or a hint. Use `multiSelect` when several can be picked.
- **More than 4 choices**: first write the numbered list in your message, one line each with the name and the full path, for example `3. web-shop  ~/code/libs/web-shop  (5 files in .claude/)`. Then ask with AskUserQuestion: options `All (N)` and `None`, and say in the question to type numbers or names in **Other** (`1-3, 5`, `all but web-shop`). Put the same list in the preview of the `All` option so it shows in the dialog.
- Repeat back what you understood when the answer was typed, then go on.
- Esc or "cancel" leaves the current step without changing anything.

## Start

1. `sor --version` must work: the plugin provides `sor`, downloading it on first use.
2. `sor group list --json`. If the user said what they want in `$ARGUMENTS`, go straight to it. Otherwise ask: each group (description: its repositories and shared scopes), **New group**, **Status**, **Sync**.

## New group

1. Ask the name and an optional description. Run `sor group add <name> --description "<text>"`. If sor refuses the name, it says why and suggests a fix: offer that fix.
2. Repositories: ask which folder to look in (suggest the parent of the current project). `sor find <folder> --json` lists the git repositories inside, how many `.claude/` files each has, and their groups. Ask which to add, then `sor member add <group> <path>` for each. Offer another folder until the user is done.
3. Offer to capture rules now (next section), then to sync.

## Capture rules into scopes

1. Ask which repositories to take rules from: the group's members (`sor repos <group> --json`) that have files, plus "another repository" by path.
2. `sor scan <repo>... --json`. It lists the files each `.claude/` could share:
   - `name` is the file's name in the scope, `path` its place in the repositories, `repos` where it comes from, `hooked` that a hook of `.claude/settings.json` runs it and will follow it.
   - Identical copies count once. Different versions of one path appear as `<folder>/<repository>-<file>`: tell the user, they can capture one version or all of them and merge.
3. Ask which files to capture. Show each by `name`, with its repositories and a mark when its hook follows.
4. Ask: **one scope** for all of them, or **several scopes**? For several, ask the first scope's name and its files, then the next for the files left, until none is left. Suggest names from the files (`anonymity`, `conventions`, `git`).
5. Permissions: `scan` gives the `allow`, `deny` and `ask` rules of each repository's `.claude/settings.json`. If there are any, explain in two lines:
   - `allow`: commands Claude runs without asking; `deny`: commands it never runs; `ask`: commands it always asks about first.
   - Shared, they go to `settings.local.json` of every repository of the group.

   Then ask "Share them too? (9 allow, 14 deny)". Recommend **yes** when there are only `deny` and `ask` rules (they only restrict Claude), and leave the choice open when there are `allow` rules: they grant the same rights in every repository, so check they fit them all. With several scopes, ask which scope takes them.
6. For each scope: `sor scope capture <group> <scope> <name>... --from <repo> [--from <repo>]... [--permissions] [--description "<text>"]`. Pass file names as `scan` gave them. A path takes all its versions, a version name only that one.
7. Versions to merge (next section), then offer to sync.

## Merging versions

When a scope holds several versions of one file (`rules/app-git.md`, `rules/lib-git.md`):

1. `sor scope list <group> --json` gives each scope's `dir`. Read the versions there: rules under `rules/`, agents under `agents/`, other files under `files/`.
2. Merge them yourself: keep every rule and behaviour of each version, say each thing once, keep the language, tone and format. For a script, keep it working and keep what each version does.
3. Show the merged file and what you combined, and ask to confirm.
4. Write it to a temporary file, then `sor scope merge <group> <scope> <path> <version>... --file <tmp>`. sor writes it under the plain name, archives the versions in the store history, and points their hooks at the merged file.

If the user prefers to keep the versions side by side, leave them: each repository then gets both.

## Sync

1. `sor status --json` (exit code 2 means something differs). Give one line per repository: files to install, update, remove or settle, settings, git exclude. Offer the details file by file.
2. Ask before applying, then `sor sync`.
3. After a capture, offer to tidy the source repositories: `sor tidy <repo> <path>...` moves their own copies (paths as `.claude/<path>`) to `.claude/sor/backup/` and takes their hooks out of `.claude/settings.json`, so each rule exists once. Do it only after the sync installed the scopes there.
4. Conflicts (`modified`: the user edited sor's copy; `conflict`: a file was there before sor): show `sor diff <repo> <file>`, then ask:
   - **Keep mine here**, until the group's version changes again (`sor resolve <repo> <file> keep`);
   - **Take the group's version**, mine goes to `.claude/sor/backup/` (`take`);
   - **Promote mine to the group**, every member gets it (`promote`, then sync);
   - **Decide later**.
5. A setting kept (`clash`): the repository has its own value. Say so; it changes it there to take the group's.
6. Finish with `sor status`: say whether every repository is up to date, and what is left otherwise.

## A group's menu

For an existing group, offer what applies:

- **Repositories**: add (as in New group) or remove (`sor member remove <group> <path>`, then `sor sync <path>` to take its files away).
- **Capture rules**.
- **Shared scopes**: stop sharing one with `sor group unshare <group> <scope>`, or share it again.
- **Borrowed scopes**: take single scopes of another group with `sor group import <group> <from> <scope>...`.
- **Exceptions**: a repository skips a scope with `sor group exclude <group> <repo> <scope>...`, or takes it back with `include`.
- **Agents**: off by default; `sor group agents <group> on|off` also shares the agents of its scopes.
- **Check and sync**.
- **Forget this group**: `sor group remove <group>`, then `sor sync` on its repositories. Its scopes stay in the store.

Changes to groups take effect at the next `sor sync`: offer it before you finish.

## The terminal version

The `sor` keyboard menus in the user's own terminal are the main way to use sor: faster, with lists of any length. The plugin's `sor` only exists for Claude. Once per session, at the end, if `type -a sor` finds no `sor` outside the plugin, tell the user they can install it in their terminal too. Give the line for their system:

- Linux and macOS: `curl -fsSL https://github.com/Khara-AI/sync-optimized-rules/releases/latest/download/install.sh | sh`
- Windows: `irm https://github.com/Khara-AI/sync-optimized-rules/releases/latest/download/install.ps1 | iex`

Both share the same configuration, so what they set up here is already there.
