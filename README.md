# herdr-explorer

A [herdr](https://github.com/herdrdev/herdr) plugin that adds a VS Code-style file explorer to the left of every
tab: the space's file tree, live git status, and file actions.

## Features

- Opens automatically when a space or tab is created, without stealing focus from the terminal.
- Folders expand and collapse with the mouse or the keyboard, and are remembered between sessions.
- Live git status (file changes, commits, staging, branch switches):
  - letter next to the file: `M` modified, `U` untracked, `A` added, `R` renamed, `!` conflict;
  - `●` on folders containing changes, coloured after the most significant one (red for a deleted file, which
    is no longer listed);
  - git-ignored files and folders greyed out;
  - space name and current branch in the header.
- Opens files in VS Code, or in the default application for images, PDFs, Office documents, archives and media.
  Executables and scripts are never run, even when VS Code is missing.
- Context menu: insert the path into the terminal, copy the path, reveal in the file manager, view git changes.
- Nerd Font icons, adjustable width that is remembered.

## Requirements

- herdr 0.9.3 or later
- Node.js 20 or later
- git, optional: without it, no git status or changes view
- A [Nerd Font](https://www.nerdfonts.com/) in the terminal running herdr, otherwise icons show as empty boxes.
  On Windows: `winget install DEVCOM.JetBrainsMonoNerdFont`, then select it in Windows Terminal.
- Optional: VS Code (`code` in `PATH` on Linux and macOS); on Linux, `wl-copy` or `xclip` for the copy actions.

Tested on Windows 11. Linux and macOS are supported in the code but not yet verified.

## Installation

```sh
git clone https://github.com/gcaudan/herdr-explorer.git
cd herdr-explorer
./install.sh        # Linux, macOS
.\install.ps1       # Windows (PowerShell)
```

If PowerShell blocks the script, run `powershell -ExecutionPolicy Bypass -File .\install.ps1`.

The script links the plugin to herdr from this folder (`herdr plugin link`), adds the `prefix+f` key binding to
`config.toml` if missing, then reloads the configuration. It is safe to run again, for instance after moving the
folder.

To add the explorer to already open tabs that have a single pane, run `node hook.js --all` in this folder from a
herdr pane.

### Uninstall

```sh
herdr plugin unlink gcaudan.explorer
```

Then remove the `[[keys.command]]` block for `gcaudan.explorer.toggle` from `config.toml`.

## Usage

### Mouse

| Action | Effect |
|---|---|
| Click a folder | expand or collapse it |
| Double-click a file | open it |
| Right-click | action menu |
| Wheel | scroll |
| Click `«` (top right) | close the explorer |

### Keyboard

When the explorer has focus:

| Key | Effect |
|---|---|
| `↑` `↓`, `j` `k` | move |
| `→`, `l` | expand a folder |
| `←`, `h` | collapse a folder, or go to the parent folder |
| `Enter`, `Space` | open a file, expand or collapse a folder |
| `Home` `End`, `g` `G` | go to the top or bottom |
| `PageUp` `PageDown` | scroll by one page |
| `m`, `Shift+F10`, Menu key | action menu (`Esc` or `q` to close it) |
| `r` | force a refresh (a full scan also runs every 30 s) |

Anywhere in herdr: `prefix+f` (`ctrl+b` then `f`) opens or closes the explorer of the current tab.

### Action menu

| Item | Effect |
|---|---|
| Insert into terminal | types the path, relative to the folder of the terminal on the right, and focuses it; prefixed with `@` when an agent such as Claude Code runs there (quoted instead if the name contains spaces) |
| Open in VS Code | |
| Open with default app | documents, media and text files only (no executables or scripts) |
| Reveal in Explorer | opens the parent folder with the item selected; on Linux and macOS, *Open folder* opens it in the file manager |
| Copy path | absolute path |
| Copy relative path | relative to the space folder |
| View changes | on a changed item: full-tab git diff against the last commit, untracked files shown in full; arrows, `PageUp` `PageDown`, `Space` or wheel to scroll, `q` or `Esc` to go back |

## Configuration

- **Width**: 32 columns by default. When resized by dragging the border, the width is kept for the next openings
  and restored when the window is resized.
- **State**: the width (`width` file) and the expanded folders of each space (`expanded.json`) are stored
  in the plugin state folder, `%LOCALAPPDATA%\herdr\plugins\gcaudan.explorer` on Windows. Deleting `width` resets
  the width to 32 columns.

## How it works

- **Placement**: herdr only splits panes to the right or downwards. The hook therefore opens the explorer on the
  right, sets its width, swaps it with the terminal, then gives focus back to the terminal. If the tab was created
  in the background, the display briefly switches to it, then returns to the tab being viewed.
- **Closing**: when the tab's terminal exits, the explorer closes too, and so does the tab.
- **Right-click**: herdr keeps right-clicks for its own menu unless the pane asks for them; the explorer does so at
  startup (`herdr pane input --right-click pane`).

## Security

The explorer opens by itself on any folder, including freshly cloned untrusted repositories. Therefore:

- `git` and `herdr` are run from their absolute path in `PATH`: on Windows, a bare program name is first looked up
  in the working directory, where a repository could plant a `git.exe`.
- git runs with hooks, `core.fsmonitor` and the repository's own filters (`filter.<driver>.clean`, `smudge`,
  `process`, from `.git/config`, `config.worktree` or included files) disabled, ignores submodules, and the changes
  view runs without external diff tools or textconv filters: a repository's config cannot run commands through
  them. Filters from the global and system config, such as git-lfs, are kept.
- Control characters in file names and diff contents are never sent to the terminal; the changes view colours
  the diff itself, so a file cannot hide its changes with escape sequences.
- *Insert into terminal* refuses names containing control characters (a newline would run the command or submit
  the prompt), prefixes names starting with `-` with `./`, and quotes the others for the shell (single quotes,
  for PowerShell and POSIX shells).
- Double-click and *Open with default app* never start executables, scripts or shortcuts.

## Limitations

- herdr enforces a minimum of 10 % of the width per pane: in a narrow window, the explorer is wider than requested.
  This is also why it closes instead of collapsing into a narrow strip.
- When a tab opens, the split shows half-and-half for a few tens of milliseconds before taking its width:
  `herdr plugin pane open` cannot set the size at creation.
- In a tab with several panes, `prefix+f` places the explorer to the left of the top-left pane, which may not span
  the full height.
- Changes inside submodules are not shown: git ignores submodules, whose own config could run commands.
- After a herdr server restart, the explorer should be relaunched with the session; not verified.
- *Insert into terminal* quotes for PowerShell and POSIX shells, not for `cmd.exe`: there, a name containing
  `&`, `|`, `<`, `>`, `^` or `%` could still run a command once Enter is pressed.
- *Open with default app* opens HTML and SVG files in the browser, and Office documents in Office, which asks
  before enabling macros.

## Project structure

| File | Role |
|---|---|
| `herdr-plugin.toml` | manifest: `tab.created` event, `tree` and `diff` panes, `toggle` action |
| `hook.js` | opens the explorer to the left of a tab, or closes it (`toggle` action) |
| `explorer.js` | the explorer: tree, git status, mouse, keyboard, menu, width |
| `actions.js` | file actions: open, insert, copy, reveal, diff |
| `diff.js` | git changes viewer for a file or folder |
| `exec.js` | runs external programs from their absolute path, safe git options |
| `install.sh`, `install.ps1` | installation: `herdr plugin link` and `prefix+f` key binding |

## License

[MIT](LICENSE)
