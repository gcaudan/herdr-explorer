# herdr-explorer

[![Release](https://img.shields.io/github/v/release/gcaudan/herdr-explorer)](https://github.com/gcaudan/herdr-explorer/releases/latest)
[![License: MIT](https://img.shields.io/github/license/gcaudan/herdr-explorer)](LICENSE)

> A VS Code-style file explorer, preview, search and source control for every [herdr](https://github.com/herdrdev/herdr) tab.

herdr gives you tabs of terminals and agents; this plugin adds the side bar you miss from an editor. It docks a file
tree with live git status next to the terminal of each new tab, so you can follow what an agent changes, read the
file it just wrote, drop a path into the shell or into a Claude Code prompt, then stage and commit, without leaving
herdr. Plain Node.js, with no dependencies and no build step.

```
┌──────────────┬───────────────────────────┬──────────────────────┐
│ Explorer     │ terminal / agent          │ Preview              │
│ (tree, git)  │                           │ (code, Markdown,     │
│              │                           │  images, hex)        │
└──────────────┴───────────────────────────┴──────────────────────┘
```

Plugin id `gcaudan.explorer` · herdr 0.9.3 or later · Linux, macOS and Windows.

## Features

- **Explorer**: a file tree docked on the left (or right) of every new tab, without taking the focus. VS Code git
  decorations, filter as you type, file actions (new, rename, duplicate, trash, stage, discard), *Set as root*, and
  an optional mode that follows the terminal's folder.
- **Insert into terminal**: types a file's path, relative and quoted for the shell, into the terminal next to the
  explorer; prefixed with `@` when an agent such as Claude Code runs there.
- **Preview**: one pane per tab, reused for every file. Syntax highlighting for 27 languages and formats, rendered
  Markdown (GFM tables, task lists, alerts, simple Mermaid flowcharts), images drawn with half blocks, a hex dump for
  binary files. Reloads as soon as the file changes on disk, for instance when an agent writes it. Find, wrap, zoom,
  and mouse selection copied to the clipboard.
- **Search**: a popup with fuzzy *Go to file* (`path:line` supported) and *Search in files* (case, whole word, regex,
  include and exclude globs), with a preview of the selected result on wide screens.
- **Source control**: stage, unstage, discard, commit or amend (with an optional message written by Claude), fetch,
  pull, push, branches, history, stashes and refs; continue or abort a merge, rebase, cherry-pick or revert. Inline
  or side-by-side diffs with word-level highlights.
- **Editors**: your graphical editor (VS Code by default), a terminal editor in a full-tab pane, or, for documents
  and media, the system's default application. Executables and scripts are never run.
- **Icons**: Nerd Font icons, or the colours of any VS Code icon theme you have installed.
- **Restored after a herdr restart**, at the same folder and file, and **updated** from GitHub releases in place.
- **Safe on untrusted repositories**: no shell, hardened git calls and sanitized terminal output (see
  [SECURITY.md](SECURITY.md)).

## Requirements

- herdr 0.9.3 or later.
- Node.js 20 or later, with `node` on the `PATH` that herdr gives plugins.
- A [Nerd Font](https://www.nerdfonts.com/) (v3) in the terminal running herdr; without one, icons show as empty
  boxes.
- Optional:
  - git: without it there is no git status, no changes view, no source control and no updates;
  - a graphical editor (VS Code by default);
  - a clipboard tool and a trash tool (see [Platform tools](#platform-tools));
  - the [Claude Code](https://claude.com/claude-code) CLI (`claude`), for commit messages.

Tested with herdr 0.9.3 on Linux (Fedora, Wayland), every pane included. Version 0.1 of the explorer was also tested
on Windows 11; the other panes are not verified on Windows or macOS, and opening in VS Code was not tested.

### Platform tools

Each feature degrades gracefully without its tool.

| Feature | Linux | macOS | Windows |
|---|---|---|---|
| Clipboard | `wl-copy`, else `xclip`, else OSC 52 | `pbcopy` | PowerShell `Set-Clipboard` |
| Trash | `gio trash`, else `trash-put` | `/usr/bin/trash` (macOS 14), else Finder | Recycle Bin (PowerShell) |
| Open in app | `xdg-open` | `open` | `explorer.exe` |
| Reveal | `xdg-open` on the parent folder | `open -R` (selects the file) | `explorer.exe /select` |

Without a trash, deleting asks a second time before removing for good.

## Installation

### From the herdr plugin index

```sh
herdr plugin install gcaudan/herdr-explorer
```

herdr manages this checkout and its updates, but does not add key bindings. Append these blocks to herdr's
`config.toml` (`~/.config/herdr/config.toml` on Linux and macOS, `%APPDATA%\herdr\config.toml` on Windows), then run
`herdr server reload-config`:

```toml
[[keys.command]]
key = "prefix+f"
type = "plugin_action"
command = "gcaudan.explorer.toggle"
description = "file explorer"

[[keys.command]]
key = "prefix+a"
type = "plugin_action"
command = "gcaudan.explorer.search"
description = "go to file"

[[keys.command]]
key = "prefix+d"
type = "plugin_action"
command = "gcaudan.explorer.scm"
description = "source control"
```

To update, run `herdr plugin update gcaudan.explorer` (clicking `↑` in the explorer copies this command): herdr
installs the new version into a new folder, so the open explorers and previews keep the old code until you reopen
them (`prefix+f` twice). To uninstall: `herdr plugin uninstall gcaudan.explorer`, then remove the key bindings.

### From a git clone

The install script links the clone to herdr and adds the key bindings above; the explorer can then update itself.

```sh
git clone https://github.com/gcaudan/herdr-explorer.git
cd herdr-explorer
./install.sh
```

On Windows (PowerShell):

```powershell
git clone https://github.com/gcaudan/herdr-explorer.git
cd herdr-explorer
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

Keep the clone where it is, on `main`: herdr runs the plugin from that folder, and updates pull into it. See
[docs/installation.md](docs/installation.md) for what the script does, updates and uninstalling.

## Quick start

| Key | Action |
|---|---|
| `prefix+f` (`ctrl+b`, then `f`) | show or hide the explorer in the current tab |
| `prefix+a` | go to file, in the explorer's root (else the focused terminal's folder) |
| `prefix+d` | source control, for the same folder |

1. Open a new tab: the explorer opens on the left, at the tab's folder.
2. Move with `↑` `↓` (or `j` `k`). `Enter` on a folder expands or collapses it; `Enter` or a double-click on a
   file shows it in the preview next to the terminal (documents and media open in their app).
3. Right-click a row (or press `m`) for the action menu: open in editor, insert into terminal, new file, rename,
   delete, view changes, stage…
4. `ctrl+p` goes to a file and `ctrl+f` searches in files.
5. `s` opens source control: `c` to type a message, `Enter` to commit.
6. `,` opens the settings.

See [docs/usage.md](docs/usage.md) for every pane, key, mouse action and menu item.

## Configuration

Settings (hidden files, docking side, editor command, icon theme, update check…) are changed from the explorer's
settings menu and stored in `settings.json`. The explorer's keys can be rebound in `keys.json`. See
[docs/configuration.md](docs/configuration.md).

## Documentation

- [Installation and updates](docs/installation.md): the install script, the update check and steps, uninstalling.
- [Usage](docs/usage.md): every key, mouse action and menu item of each pane.
- [Configuration](docs/configuration.md): settings, state files, key bindings.
- [Limitations and troubleshooting](docs/troubleshooting.md): limits, common problems, every message and its fix.
- [How it works](docs/architecture.md): placement, preview requests, restore after a restart, refresh, self-restart.
- [Changelog](CHANGELOG.md)

## Contributing

There is no build step: clone, run the install script, edit, and reopen the pane. Commits follow
[Conventional Commits](https://www.conventionalcommits.org/), and releases are cut by release-please. See
[CONTRIBUTING.md](CONTRIBUTING.md).

## Security

The explorer opens by itself on any folder, including repositories nobody has reviewed. [SECURITY.md](SECURITY.md)
describes how it keeps a hostile repository from running code, leaking credentials or taking over the terminal.

## License

[MIT](LICENSE) © gcaudan
