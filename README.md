# herdr-explorer

[![Release](https://img.shields.io/github/v/release/gcaudan/herdr-explorer)](https://github.com/gcaudan/herdr-explorer/releases/latest)
[![License: MIT](https://img.shields.io/github/license/gcaudan/herdr-explorer)](LICENSE)

> A VS Code-style file explorer, preview, search and source control for every [herdr](https://github.com/herdrdev/herdr) tab.

herdr gives you tabs of terminals and agents; this plugin adds the side bar you miss from an editor. It docks a file
tree with live git status next to the terminal of each new tab, so you can follow what an agent changes, read the
file it just wrote, drop a path into the shell or into a Claude Code prompt, then stage and commit, without leaving
herdr. One binary, written in Rust with no dependencies, downloaded from the release of the version you install.

```
┌──────────────┬───────────────────────────┬──────────────────────┐
│ Explorer     │ terminal / agent          │ Preview              │
│ (tree, git)  │                           │ (code, Markdown,     │
│              │                           │  images, hex)        │
└──────────────┴───────────────────────────┴──────────────────────┘
```

Plugin id `gcaudan.explorer` · herdr 0.9.4 or later · Linux, macOS and Windows.

## Features

- **Explorer**: a file tree docked on the left (or right) of every new tab, without taking the focus: VS Code git
  decorations, filter as you type, file actions (new, rename, duplicate, trash, stage, discard), *Set as root*, and
  an optional mode that follows the terminal's folder. Its keys can be rebound.
- **Review**: the changed files only, `]` and `[` to step through them in the preview, and review notes on lines of
  the preview or the diff, typed together into the agent's prompt (`src/app.js:12-14 (fix): …`) without submitting
  it.
- **Show a file from an agent**: `bin/herdr-explorer peek file:line`, and a Claude Code skill that calls it, show a
  file at a line in the tab's preview, so that an agent points you at the code it talks about. `ctrl`+click on a
  `file://` link in the terminal shows that file the same way.
- **Insert into terminal**: types a file's path, relative and quoted, into the terminal next to the explorer;
  prefixed with `@` when an agent such as Claude Code runs there.
- **Preview**: one pane per tab, with VS Code-style tabs (temporary or kept): syntax highlighting for 27 languages and formats, rendered
  Markdown (GFM tables, task lists with the `[?]`, `[-]` and `[/]` statuses, alerts, simple Mermaid flowcharts, local images, sharp where possible), images in real pixels where the
  terminal has the Kitty graphics protocol (else half blocks), WebP and TIFF included, a hex dump for binary files.
  Reloads as soon as the file changes on disk. Find, wrap, zoom, and mouse selection copied to the clipboard.
  Quick edits in place (`i`, text files up to 2 MB): undo, find and replace, changed lines in the gutter, line endings kept. A **Diff**
  view shows the file's git changes.
- **Search**: fuzzy *Go to file* (`path:line` supported) and *Search in files* (case, whole word, regex, globs),
  with a preview of the selected result on wide screens.
- **Source control**: stage, unstage, discard, commit or amend (with an optional message written by Claude), fetch,
  pull, push (also forced with lease), merge, rebase, cherry-pick, tags, undo the last commit, reset, branches,
  stashes and refs; a history with a branch graph, also for a single file, branch comparisons;
  continue or abort a merge, rebase, cherry-pick or revert. Inline or side-by-side diffs with word-level highlights,
  compared with HEAD or with where the branch started, also in a *Changes* pane for a file or a folder.
- **Editors**: your graphical editor (VS Code by default), a terminal editor in a full-tab pane, or the system's
  application for documents and media. Executables and scripts are never run.
- **Icons**: Nerd Font icons, or the colours of any VS Code icon theme you have installed; emoji, Unicode or ASCII
  icons without a Nerd Font.
- **Restored after a herdr restart**, at the same folder and file, and **updated** from GitHub releases in place,
  the binary checked against the release's `SHA256SUMS`.
- **Safe on untrusted repositories**: no shell, hardened git calls and sanitized terminal output (see
  [SECURITY.md](SECURITY.md)).

## Requirements

herdr 0.9.4 or later, `curl` to download the binary, and a
[Nerd Font](https://www.nerdfonts.com/) (v3) in the terminal running herdr, or emoji, Unicode or ASCII icons. git, a graphical editor, a clipboard and
a trash tool, and the [Claude Code](https://claude.com/claude-code) CLI are optional: see
[Requirements](docs/installation.md#requirements) for what each one adds.

Release binaries exist for Linux and macOS (x86_64, arm64) and Windows (x86_64, arm64 in emulation); elsewhere,
build one with Rust (see [Building the binary](docs/installation.md#building-the-binary)).

Tested with herdr 0.9.3 on Linux (Fedora, Wayland), every pane included. Version 0.1 of the explorer was also tested
on Windows 11; the other panes are not verified on Windows or macOS, and opening in VS Code was not tested.

## Installation

### From the herdr plugin index

```sh
herdr plugin install gcaudan/herdr-explorer
```

herdr downloads the binary (the manifest's build step) and manages this checkout and its updates (`herdr plugin
update gcaudan.explorer`). On its first start, the explorer offers to add the key bindings and, when Claude Code is
installed, link the `herdr-preview` skill; or run `herdr-explorer setup` from the plugin folder's `bin/` yourself
(see [Setup](docs/installation.md#setup)).

### From a git clone

Clone the latest release's tag (`v<X.Y.Z>`, from the
[releases page](https://github.com/gcaudan/herdr-explorer/releases/latest)), download its binary, link the clone to
herdr, then add the key bindings:

```sh
git clone --branch v<X.Y.Z> https://github.com/gcaudan/herdr-explorer.git
cd herdr-explorer
sh scripts/fetch-bin.sh     # Windows (PowerShell): powershell -ExecutionPolicy Bypass -File scripts\fetch-bin.ps1
herdr plugin link .
bin/herdr-explorer setup    # or accept the explorer's offer on its first start
```

Keep the clone where it is: herdr runs the plugin from that folder. You update it yourself (`git pull` or another
tag, then `sh scripts/fetch-bin.sh`). See [docs/installation.md](docs/installation.md) for setup, updates and
uninstalling.

## Quick start

| Key | Action |
|---|---|
| `prefix+f` (`ctrl+b`, then `f`) | show or hide the explorer in the current tab |
| `prefix+a` | go to file, in the explorer's root (else the focused terminal's folder) |
| `prefix+d` | source control, for the same folder, in a review tab of its own |

1. Open a new tab: the explorer opens on the left, at the tab's folder.
2. Move with `↑` `↓` (or `j` `k`). `Enter` folds or unfolds a folder; on a file, `Enter` or a double-click shows
   it in the preview (documents and media open in their app).
3. Right-click a row (or press `m`) for the action menu: open in editor, insert into terminal, new file, rename,
   delete, view changes, stage…
4. `/` filters the tree, `ctrl+p` goes to a file and `ctrl+f` searches in files.
5. `s` opens source control: `c` to type a message, `Enter` to commit.
6. `,` opens the settings, `?` (or `F1`) lists every key of the pane, and `q` closes the explorer.

See [docs/usage.md](docs/usage.md) for every pane, key, mouse action and menu item.

## Documentation

- [Installation and updates](docs/installation.md): requirements, install, updates, uninstall.
- [Usage](docs/usage.md): every key, mouse action and menu item of each pane.
- [Configuration](docs/configuration.md): the settings (`,` in the explorer), key bindings, state files, environment.
- [Limitations and troubleshooting](docs/troubleshooting.md): limits, common problems, every message and its fix.
- [How it works](docs/architecture.md): placement, pane inputs, preview requests, restore, refresh, widths,
  self-restart.
- [Changelog](CHANGELOG.md)

## Contributing

Install Rust, clone, link it with `herdr plugin link .`, then `scripts/dev-build.sh` after each change and reopen the pane; the
checks are `sh scripts/check.sh`. Commits follow
[Conventional Commits](https://www.conventionalcommits.org/), and releases are cut by release-please. See
[CONTRIBUTING.md](CONTRIBUTING.md).

## Security

The explorer opens by itself on any folder, including repositories nobody has reviewed. [SECURITY.md](SECURITY.md)
describes how it keeps a hostile repository from running code, leaking credentials or taking over the terminal.

## License

[MIT](LICENSE) © gcaudan
