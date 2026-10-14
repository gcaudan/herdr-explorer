# herdr-explorer

A [herdr](https://github.com/herdrdev/herdr) plugin that adds a VS Code-style sidebar to every tab: the space's file
tree with live git status, plus a file preview, a search popup, a source control pane and an editor pane. Plain
Node.js, with no dependencies and no build step.

## Features

- **Explorer**: opens by itself on the left (or the right) of every new tab, without taking the focus.
  - Folders expand with the mouse or the keyboard; the expanded ones are remembered per root folder between
    sessions.
  - Live git status: `M` modified, `U` untracked, `A` added, `R` renamed, `D` deleted, `!` conflict, `●` on a folder holding
    changes; git-ignored entries are greyed out. The header shows the space name and the branch.
  - Filter as you type (`/`): the tree keeps the names containing the text.
  - New file, new folder, rename, duplicate, move to the trash; stage, unstage or discard a file or a folder.
  - Any folder can become the root (*Set as root*; `Backspace` goes to the parent folder).
  - Can follow the folder of the terminal next to it.
  - Nerd Font icons, or the colours of a VS Code icon theme (Material Icon Theme…).
  - Width set by dragging its border, and remembered.
  - Comes back after a herdr restart, at the same folder, with the preview on the same file.
- **Preview**: one pane per tab, opened next to the terminal and reused for every file.
  - Syntax highlighting for 27 languages and formats.
  - Rendered Markdown: headings, lists, task lists, tables, code blocks, quotes, GitHub alerts, Mermaid flowcharts.
  - Images (PNG, baseline JPEG, GIF, BMP, ICO, CUR) drawn with half blocks; a hex dump for other binary files.
  - Reloads as soon as the file changes on disk, for instance when an agent writes it.
  - Find in file, wrap, line numbers, full-tab zoom.
  - Mouse selection by character, word or line, copied to the clipboard.
- **Search**: a popup over the tab.
  - *Go to file*: fuzzy match on paths; `path:line` goes to a line.
  - *Search in files*: case, whole word and regex toggles; include and exclude globs.
  - A preview of the selected result on wide screens.
- **Source control**: a full-tab pane.
  - Conflicts, staged changes and changes, as a list or a tree; stage, unstage, discard.
  - The branch, the ahead and behind counts, and any merge, rebase, cherry-pick, revert, `git am` or bisect in
    progress.
  - Inline or side-by-side diffs, with word-level highlights.
  - Commit or amend, with a message written by the `claude` CLI when it is installed.
  - Branches (switch, create, delete), history, stashes, tags, remotes, worktrees; fetch, pull, push.
- **Opening files**: in your editor (the command set in the settings, else VS Code when installed, else a terminal
  editor), in a terminal editor in a full-tab pane, or, for documents and media, in the system's default
  application. Executables and scripts are never run.
- **Context menu**: insert the path into the terminal, copy the path, reveal in the file manager, view git changes.
- **Updates**: a daily check for a newer release; `↑` in the explorer header updates the plugin, and the open panes
  restart on the new code.

## Requirements

- herdr 0.9.3 or later.
- Node.js 20 or later.
- git, optional: without it there is no git status, no changes view and no source control.
- A [Nerd Font](https://www.nerdfonts.com/) in the terminal running herdr; without one, icons show as empty boxes.
- Optional:
  - a graphical editor (VS Code by default);
  - a clipboard tool and a trash tool (see [Installation](#installation) for each system); without a clipboard
    tool, copies go through OSC 52;
  - the [Claude Code](https://claude.com/claude-code) CLI (`claude`), for commit messages.

Tested with herdr 0.9.3 on Linux (Fedora, Wayland): the explorer with the mouse and the keyboard, the preview, the
search popup, source control, diffs, editors, file operations, settings, `keys.json`, follow mode, right docking,
the `prefix` key bindings, and the explorer and preview coming back after a herdr server restart. Version 0.1 of the
explorer was also tested on Windows 11. The other panes are not verified on Windows or macOS, and opening in VS Code
was not tested (only the fallback to the terminal editor).

## Installation

The plugin runs from its git clone, which updates pull into: keep the clone in a permanent folder, on `main`.

### Linux and macOS

```sh
git clone https://github.com/gcaudan/herdr-explorer.git
cd herdr-explorer
./install.sh
```

- The key bindings are added to `~/.config/herdr/config.toml`.
- Optional tools:
  - VS Code with `code` in `PATH`, or another editor set in the settings;
  - on Linux, `wl-copy` (Wayland) or `xclip` (X11) for the copy actions, and `gio` or `trash-put` for the trash;
  - on macOS, nothing: `pbcopy` and the trash are built in.
- Install a Nerd Font (for instance `JetBrainsMono Nerd Font`, from your package manager or
  [nerdfonts.com](https://www.nerdfonts.com/)) and select it in your terminal.

### Windows

```powershell
git clone https://github.com/gcaudan/herdr-explorer.git
cd herdr-explorer
.\install.ps1
```

- If PowerShell blocks the script: `powershell -ExecutionPolicy Bypass -File .\install.ps1`.
- The key bindings are added to `%APPDATA%\herdr\config.toml`.
- The clipboard and the recycle bin go through PowerShell, which is built in. VS Code is found in `PATH` or in its
  default user install folder. Any other editor must be an `.exe`: a `.cmd` launcher cannot run without a shell.
- Install a Nerd Font with `winget install DEVCOM.JetBrainsMonoNerdFont`, then select it in Windows Terminal.

### What the script does

It links the plugin to herdr from this folder (`herdr plugin link`, after unlinking any previous link), adds the key
bindings below to `config.toml` when they are missing, then reloads the configuration. It is safe to run again, for
instance after moving the folder.

| Key | Action |
|---|---|
| `prefix+f` (`ctrl+b` then `f`) | open or close the explorer of the current tab |
| `prefix+a` | go to file, in the current tab's folder |
| `prefix+d` | source control of the current tab's folder |

herdr keeps its own copy of the manifest: after pulling a new version, run the script again (or
`herdr plugin link .`) so that herdr picks up new hooks, such as the startup hook.

The explorer only opens by itself in new tabs. To add it to the open tabs that have a single pane, run
`node hook.js --all` in this folder from a herdr pane.

### Update

**Check.** The startup hook (`hook.js --restore`) and the `tab.created` hook (`hook.js`) ask the GitHub API for the
latest release of `gcaudan/herdr-explorer` — never the panes. At most once every 24 h (a failed check also waits a
day), 3 s timeout, skipped when `updateCheck` is `false`. Only a version made of digits and dots (leading `v`
stripped) is kept, in `update.json` (`checkedAt`, `latest`).

**Signal.** When `latest` is newer than the version in the clone's `herdr-plugin.toml`, the explorer header shows a
green `↑` and the settings menu ends with *Update to X…*. A clone ahead of the release shows nothing. The `↑` is
hidden while an update runs, while a header message is shown, and when *Check for updates* is off.

**Update.** Clicking `↑` or *Update to X…* asks `Update to X? y/N`, then shows `Updating…` until done. Steps
(`update.js`):

| Step | Refusal message |
|---|---|
| Take `update.lock` in the state folder (one update at a time across all explorers; a lock older than 2 min is taken over) | `Update in progress` |
| `git rev-parse --show-toplevel` must be the plugin folder itself (not a copy inside another repository, e.g. dotfiles) | `Not a git clone` |
| Current branch must be `main` | `Not on main` |
| No merge, rebase, cherry-pick, revert, `git am` or bisect in progress | `A git operation is in progress` |
| `git status --porcelain --untracked-files=no` must be empty (untracked files are allowed) | `Local changes in the plugin folder` |
| `git pull --ff-only --no-rebase` (60 s timeout) | see below |

Every git call runs with `GIT_TERMINAL_PROMPT=0` and `GCM_INTERACTIVE=never` (no Git Credential Manager window), and
outside Windows in a detached session with no terminal, so nothing can prompt in the pane. The pull uses
`GIT_SSH_COMMAND="ssh -o BatchMode=yes"` unless you set `GIT_SSH_COMMAND`, `GIT_SSH` or `core.sshCommand` yourself.

Results:

- `Already up to date` or `Updated to X`.
- The pull itself failed (network, passphrase, diverged history): the git error is shown and
  `git -C "<clone>" pull --ff-only --no-rebase` is copied to the clipboard (`…: update command copied`) to run in a terminal.
  A refusal from the table above copies nothing.
- `Updated to X: rerun the installer`: the update changed `install.sh`, `install.ps1` or the manifest beyond its
  `version` line. `finish: X` is stored in `update.json`, so every explorer shows a yellow `!` instead of `↑`, even
  after restarting. Clicking `!` or *Finish the update…* copies the installer command
  (`"<clone>/install.sh"`, or `powershell -ExecutionPolicy Bypass -File "<clone>\install.ps1"`); once copied, the
  flag is removed and the `!` disappears everywhere. The update itself never runs the installer.

**Restart.** Open explorers and previews watch `herdr-plugin.toml` (polled every 2 s). When its version differs from
the one they started with — after an update or a manual `git pull` / `git checkout` — they wait 2 s, then restart as
soon as they are idle (explorer: no update running, no menu, prompt, confirmation or filter being typed; preview: no
find being typed, no selection being dragged). The new process takes over the same pane with the same root, file
and line. The old `node` process stays alive, waiting for the new one and forwarding signals to it: each restart adds
one waiting process until the pane is closed. Search, Source Control, Changes and Editor panes do not restart; they
run the old code until closed.

### Uninstall

```sh
herdr plugin unlink gcaudan.explorer
```

Then remove the `[[keys.command]]` blocks of `gcaudan.explorer.*` from `config.toml` and run
`herdr server reload-config`, and delete the clone. The settings and the saved state remain in the plugin's state and
config folders (see [Configuration](#configuration)): delete them if you do not reinstall.

## Usage

### Explorer

| Mouse | Effect |
|---|---|
| Click a folder | expand or collapse it |
| Click a file | select it; with *Preview on single click*, also show it in the preview |
| Double-click a file | open it, like `Enter` (see below) |
| Right-click an entry | its action menu |
| Right-click below the last entry | the root's menu: new file or folder, go to file, search in files, source control (in a repository), reveal, copy path, up one level, back to the space folder (when the root is elsewhere), refresh, settings; `m` opens it too when the tree is empty |
| Wheel | scroll the tree, or move in an open menu |
| Header icons | `↑` to update when a newer release exists, `!` to finish an update (see [Update](#update)), go to file, source control, settings, `«` to close the explorer |

Opening a file (`Enter`, double-click): text files and images go to the preview; documents and media (PDF, Office,
archives, sound, video) go to their application. With *Open in the preview* off, text files go to the editor and
images to their application.

Default keys, each one changeable under its action name (see [Key bindings](#key-bindings)):

| Key | Action | Effect |
|---|---|---|
| `↑` `↓`, `k` `j` | `up`, `down` | move |
| `PageUp` `PageDown` | `pageUp`, `pageDown` | move by one page |
| `Home` `End`, `g` `G` | `top`, `bottom` | go to the top, the bottom |
| `Enter`, `Space` | `open` | open a file; expand or collapse a folder |
| `→`, `l` | `expand` | expand a folder, or go to its first entry when already expanded |
| `←`, `h` | `collapse` | collapse a folder, or go to the parent folder |
| `Esc`, `q` | `close` | close the filter or the menu |
| `/` | `filter` | filter the tree (see below) |
| `ctrl+p` | `goToFile` | go to file |
| `ctrl+f` | `searchInFiles` | search in files, in the selected folder (else in the root) |
| `s` | `sourceControl` | source control |
| `p` | `preview` | show the file in the preview and focus it |
| `e` | `edit` | edit in the terminal editor |
| `o` | `openInEditor` | open in the editor |
| `n`, `N` | `newFile`, `newFolder` | new file, new folder; `a/b/c.txt` creates the missing folders |
| `R`, `F2` | `rename` | rename |
| `Delete` | `delete` | move to the trash, after confirmation |
| `y`, `Y` | `copyPath`, `copyRelativePath` | copy the path, the relative path |
| `.`, `i` | `toggleHidden`, `toggleIgnored` | show or hide dot files, git-ignored files (saved in the settings) |
| `Backspace` | `parentRoot` | make the parent folder the root |
| `,` | `settings` | settings |
| `m`, `Shift+F10`, Menu key | `menu` | action menu |
| `r` | `refresh` | refresh now (a full scan also runs every 30 s) |

While filtering, typed characters go to the filter and the other bound keys (arrows, `Enter`, `ctrl+…`) act on the
list; `Tab` leaves the field and a click on it returns to it. `Esc` closes the filter and shows the selected entry in
the full tree. The contents of git-ignored folders are not searched.

### Action menu

| Item | Effect |
|---|---|
| Preview | show the file in the preview pane (files the preview can show) |
| Open in editor | the editor command of the settings, else VS Code, else the terminal editor (the latter for files only) |
| Open in app | the system's default application; documents, media and a few text formats only (`txt`, `md`, `csv`, `log`, `json`, `xml`, `html`, `htm`), never executables or scripts |
| Edit in terminal | the terminal editor in a full-tab pane: the one set in the settings, else `$VISUAL`, `$EDITOR`, else the first found of nano, nvim, vim, vi (on Windows: edit, nvim, vim, nano) |
| Insert into terminal | type the path into the terminal, relative to its folder, and focus it; prefixed with `@` when an agent such as Claude Code runs there (quoted instead when the name contains spaces) |
| New file, New folder, Rename, Duplicate, Delete | Delete moves to the trash, and asks before deleting for good when there is no trash |
| Search in folder, Set as root | on a folder |
| Reveal in Explorer (Windows) or Open folder, Copy path, Copy relative path | |
| View changes | full-tab git diff against the last commit, untracked files in full (`s` inline or side by side, `n` `N` hunks, `q` close) |
| Stage, Unstage, Discard changes | discard restores tracked files from the last commit; untracked files and files added since that commit go to the trash (kept when there is no trash) |

Preview, Open in app and Edit in terminal are offered on files only. The git items appear in a repository, on an
entry that is not git-ignored: Unstage always; View changes, Stage and Discard changes only when the entry has
changes.

In the menu, `up` and `down` or the wheel move, `open`, `expand` or a click run the item, and `Esc` closes it.
Name prompts take the usual line-editing keys (`ctrl+a`, `ctrl+e`, `ctrl+u`, `ctrl+k`, `ctrl+w`…); confirmations
take `y`, and any other key cancels.

### Preview

| Key or mouse | Effect |
|---|---|
| `↑` `↓`, `j` `k`, `Enter`, wheel, `PageUp` `PageDown`, `Space` `b`, `ctrl+d` `ctrl+u`, `g` `G` | scroll |
| `←` `→`, `h` `l` | scroll sideways (without wrap) |
| `/` or `ctrl+f`, then `Enter`; `n` `N` | find in the file (case-insensitive unless the text has a capital letter) |
| `w`, `L`, `m` | wrap, line numbers (both saved in the settings), rendered or raw Markdown |
| Drag with the left button | select text by character, copied to the clipboard on release |
| Double click, triple click | select and copy a word, the line |
| Header chips | rendered or raw Markdown, wrap or no wrap, `✕` close |
| `e`, `o`, `O` | edit in the terminal editor; open (documents, media and images in their application, the rest in the editor); open in the default app |
| `y`, `Y` | copy the path, the relative path |
| `ctrl+p` | go to file |
| `z` | zoom the preview to the full tab, and back |
| `r` | reload |
| `q`, `Esc`, `ctrl+c`, `✕` | close the find bar, else the pane; `Esc` first clears a selection |

Mouse selection works everywhere but in images:

- A double click selects a word (or a run of spaces, or one punctuation character); a triple click selects the whole
  line, all its wrapped rows included.
- The selection stays shown until the next click or `Esc`; the wheel scrolls without clearing it. One more cell is
  highlighted when it takes a line break.
- Dragging onto the top or bottom row scrolls; in text without wrap, dragging onto the left or right edge scrolls
  sideways. Over the header, the selection reaches the start of the top row; over the footer, the end of the bottom
  row.
- A wrapped line is copied without the breaks of the wrap; rendered Markdown without its left margin and trailing
  spaces.
- When the button is released outside the pane, the selection is copied at the next click or when the pane loses
  focus.

### Search

- *Go to file* scores paths like fzf and VS Code: matches at the start of a segment or a word, in the file name, or
  on consecutive characters score higher. `path:line` or `path:line:column` opens the file at that line.
- *Search in files* takes plain text or a regular expression. The include and exclude fields take comma-separated
  globs (`*`, `?`, `**`, `{a,b}`, `[abc]`): a pattern without a leading `/` or `./` matches at any depth, and a
  folder matches its contents. Results are grouped by file; a click on a file folds its results.
- The files searched are those git tracks or does not ignore. Outside a repository, when git lists no files, or with
  *hidden* on, the folder is walked instead (without `.git`, and without `node_modules` and dot folders unless
  *hidden* is on).
- From 100 columns, the selected result is previewed on the right.
- The last queries and globs, and the case, whole word and regex options, come back at the next opening, selected:
  typing replaces them.

| Key | Effect |
|---|---|
| `ctrl+p`, `ctrl+f` | go to file, search in files (or the header tabs) |
| `alt+c`, `alt+w`, `alt+r`, `alt+h` | case, whole word, regex, hidden and ignored files (or the chips) |
| `Tab`, `Shift+Tab` | move between the query, the include globs and the exclude globs (`*.js, src/**`) |
| `↑` `↓` (or `ctrl+k`, `ctrl+j` `ctrl+n`), `PageUp` `PageDown`, `ctrl+Home` `ctrl+End`, click, wheel | select, scroll; `alt+←` `alt+→` fold and unfold a file's results |
| `Enter`, double-click | open in the preview at the line |
| `ctrl+o` (or `alt+Enter`, which Windows Terminal keeps for full screen), `ctrl+t`, `ctrl+y` | open in the editor, insert the path into the terminal, copy the relative path |
| `Esc`, `ctrl+c` | close |

### Source control

A full-tab pane on the repository of the tab's folder, opened with `prefix+d`, with `s` in the explorer or with its
header icon.

- **Header**: the branch (click it to switch), the ahead and behind counts or *not published*, the operation in
  progress, and the Fetch, Pull, Push, refresh and close buttons (icons only below 80 columns).
- **Rows**: the selected row shows its buttons: `↶` discard and `+` stage in *Changes*, `−` unstage in *Staged
  Changes*, `+` stage on a conflict.
- **Diffs**: `Enter` or a double click on a file opens its diff, on the right of the list from 100 columns (where it
  then follows the selection), over the list below that. A diff is side by side when it has 120 columns or more.
- **Commit**: a message is needed, except for a merge (git's prepared message is used) or an amend. With nothing
  staged and no operation in progress, committing offers to stage everything first.
- Outside a repository, `i` runs `git init` after confirmation, and `r` refreshes.

| Key | Effect |
|---|---|
| `1` `2` `3` `4` | changes, history (last 500 commits), stashes, refs (branches, tags, remotes, worktrees) |
| `↑` `↓`, `k` `j`, `PageUp` `PageDown`, `Home` `End`, `g` `G`, wheel | move in the list |
| `←` `→`, `h` `l` | collapse, expand a section or folder (a click or `Enter` on one toggles it) |
| `s` or `Space`, `u`, `a`, `A` | stage, unstage, stage all, unstage all |
| `d` | discard, after confirmation. In *Changes*, untracked files are **deleted for good**, not moved to the trash, with the folders left empty; in *Staged Changes*, the index and the file go back to the last commit, and files added since are deleted; conflicts cannot be discarded |
| `Enter`, double-click | diff of a file, commit, stash or tag; in Refs, check out the branch |
| `Tab` | switch between the list and the diff (a click on the diff focuses it) |
| `n` `N`, `s`, `q` or `Esc` | in the diff: next and previous hunk, inline or side by side, close; it scrolls with the list keys, `Enter`, `Space` `b`, and `Shift+Tab` goes back to the list |
| `c`, `Enter`, `Esc` or `Tab` | type the commit message (from any view), commit, leave the field; `alt+a` amend (starts from the last message) |
| `ctrl+g` or `✧` | commit message written by Claude, or made from the file names when Claude is not available |
| `c` `Enter` or `✓ Continue`, `X` or `✕ Abort` | during a rebase, cherry-pick or revert: continue once the conflicts are staged, or abort after confirmation. A rebase always continues with git's messages; a cherry-pick or revert continues with git's message when the field is empty, and commits with the typed message otherwise. Continuing is refused when it would run `exec` lines or a merge strategy other than git's own (ort, recursive, resolve, octopus, ours, subtree), or when those cannot be checked. A merge ends with a commit, its message optional. A `git am` is continued or aborted in the terminal |
| `b` or click on the branch | branches: `/` or typing filters, `Enter` switches (a remote branch through its local one, created if needed), `n` new, `D` delete, `Esc` clears the filter then closes, `q` closes |
| `f`, `P`, `p` | fetch, pull (fast-forward only), push (the first push publishes the branch on `origin` or the only remote and sets its upstream) |
| Stashes | `n` stash (untracked files included, message optional), `a` apply, `o` pop, `d` drop |
| Refs | `D` delete the local branch (asks again, with `-D`, when it is not fully merged) |
| `t`, `r`, `q` or `Esc`, `ctrl+c` | list or tree, refresh, close (`Esc` closes the diff first) |

Confirmations take `y`; any other key cancels.

## Configuration

- **Settings** (`,` or the gear icon) are saved in `settings.json` and shared by every explorer and preview. The pane
  where a setting changes applies it at once; the last column says when the other open panes do:

  | Setting | Menu item | Default | Effect | Other open panes |
  |---|---|---|---|---|
  | `hidden` | Show hidden files | `true` | show dot files | at their next opening |
  | `ignored` | Show git-ignored files | `true` | show git-ignored files, greyed out | at their next opening |
  | `open` | Open in the preview | `"preview"` | `Enter` and double-click on a text file: `"preview"` or `"editor"` | at their next opening |
  | `editor` | Editor command… | `""` | external editor command; empty: VS Code when found, else the terminal editor | at once (read at each use) |
  | `singleClick` | Preview on single click | `false` | a single click on a file shows it in the preview | at their next opening |
  | `follow` | Follow the terminal folder | `false` | reveal the folder of the terminal in the tree, within the space folder | at once |
  | `side` | Dock on the right | `"left"` | side of the explorer | new tabs only |
  | `icons` | Icon theme… | `""` | a VS Code icon theme, `{ id, label, file }`; empty: Nerd Font icons. *Search…* lists the themes installed for VS Code, VS Code Insiders, VSCodium, Cursor, Windsurf or VS Code Server, and those built into these editors; the tree previews each one as the selection moves, and `Esc` restores the saved one | explorers at once; source control at its next opening |
  | `wrap` | (preview `w`) | `false` | preview: wrap long lines | other previews at their next opening |
  | `lineNumbers` | (preview `L`) | `true` | preview: line number gutter | other previews at their next opening |
  | `updateCheck` | Check for updates | `true` | one request a day to the GitHub API; `↑` in the header when a newer release exists | explorers at once |

  The editor command (`zed`, `subl -w`, `code -r`…) runs without a shell, with the path as its last argument. A
  terminal editor (`vi`, `vim`, `nvim`, `nano`, `micro`, `hx`, `helix`, `kak`, `emacs`, `ne`, `joe`, `mcedit`,
  `edit`) runs in the full-tab editor pane instead, and is also the one *Edit in terminal* uses. *Edit key
  bindings…* opens `keys.json` (see [Key bindings](#key-bindings)).
- **Width**: 32 columns by default (10 at least). A width set by dragging the border is kept for the next openings
  and restored when the window is resized.
- **State** is kept in the plugin state folder: `~/.local/state/herdr/plugins/gcaudan.explorer` on Linux,
  `%LOCALAPPDATA%\herdr\plugins\gcaudan.explorer` on Windows.

  | File | Content |
  |---|---|
  | `settings.json` | the settings |
  | `width` | the explorer width; delete it to go back to 32 columns |
  | `expanded.json` | the expanded folders of each root folder |
  | `search.json` | the last searches |
  | `update.json` | the time of the last update check, the latest release found, and an update left to finish |
  | `update.lock` | present while an update runs |
  | `panes/` | what each explorer and preview pane shows, read back after a herdr restart |
  | `preview-<tab>.json` | the file requested for a tab's preview (see [How it works](#how-it-works)) |

### Key bindings

The explorer's keys are read from `keys.json` in the plugin config folder (`herdr plugin config-dir gcaudan.explorer`
prints it). *Settings › Edit key bindings…* creates the file with every action and its current keys, then opens it
in the terminal editor. Changes apply within a second, without restarting the explorer.

```json
{
  "explorer": {
    "newFile": ["a"],
    "delete": ["delete", "ctrl+d"],
    "down": ["down", "j"]
  }
}
```

- An action left out keeps its default keys; listing keys for an action replaces all its defaults.
- A key given to an action is taken from the action that had it by default. A key listed for two actions goes to the
  last one.
- Key names: `up`, `down`, `left`, `right`, `home`, `end`, `pageup`, `pagedown`, `enter`, `space`, `tab`,
  `shift+tab`, `backspace`, `insert`, `delete`, `escape`, `menu`, `shift+f10`, `f1` to `f5`, `ctrl+<letter>`,
  `ctrl+up` and the other arrows, `ctrl+home`, `ctrl+end`, `shift+up`, `shift+down`, `alt+up` and the other arrows,
  `alt+<key>`, or the character itself (`/`, `n`, `N`). Terminals send `ctrl+h`, `ctrl+i`, `ctrl+j` and `ctrl+m` as
  `backspace`, `tab` and `enter`.
- Only the explorer's keys can be changed: the preview, search and source control keys are fixed.
- The menu uses `up`, `down`, `open` and `expand` (run the item), `close`, `collapse`, `menu` and `settings` (close
  it). While filtering, characters go to the filter and only the keys that are not characters (arrows, `Enter`,
  `ctrl+…`) act on the list. `Esc` always closes the menu and the filter; text fields (names, filter) keep their
  editing keys.

## How it works

- **Placement**: herdr only splits panes to the right or downwards. On a new tab, the hook therefore splits the
  top-left pane, sets the width, swaps the explorer with the terminal, then gives the focus back to the terminal.
  Docked on the right, it splits the top-right pane, with no swap. If the tab was created in the background, the
  display briefly switches to it, then returns to the tab being viewed.
- **Preview**: opened by splitting the terminal (explorer | terminal | preview). The tab's next files go to the same
  pane through a request file in the state folder, `preview-<tab>.json`. The preview watches the file's folder rather
  than the file, since editors and agents often replace a file by renaming a new one over it.
- **herdr restart**: herdr restores the plugin's panes as plain shells, its session keeping only their label and
  folder. Each explorer and preview therefore saves what it shows in `panes/<pane id>.json` (space folder and tree
  root; file and line). A startup hook (`node hook.js --restore`, run once herdr has restored the session) closes
  the plugin's panes that run no `node` process, reopens the explorer and the preview from those files, and deletes
  the files of panes that no longer exist. herdr keeps pane ids across a restart.
- **Refresh**: the explorer watches its root (recursively, except on your home folder, one of its parents or `/`)
  and the repository's `index` and `HEAD`, refreshing at most every 250 ms, with a full scan every 30 s as a safety
  net. Follow mode checks the terminal's folder every 2 s. Source control polls the status every 5 s and watches
  the git folder and `refs/`.
- **State files** are written to a temporary file then renamed, so that another pane never reads them half-written.
- **Closing**: when the tab's terminal exits, the explorer closes too, and the tab with it.
- **Self-restart after an update**: the explorer and the preview poll `herdr-plugin.toml`; when its version differs
  from the one they started with, they hand their terminal to a new `node` process running the new code, in the same
  pane, and wait for it to exit.
- **Right-click**: herdr keeps right-clicks for its own menu unless a pane asks for them; the explorer does so at
  startup (`herdr pane input --right-click pane`).

## Security

The explorer opens by itself on any folder, including freshly cloned untrusted repositories. Therefore:

- **Programs** (`git`, `herdr` and the others) run from their absolute path, looked up in the absolute folders of
  `PATH` only. On Windows, a bare program name is first looked up in the working directory, and elsewhere an empty
  or `.` entry of `PATH` means that folder too: a repository could plant a `git` there.
- **Every git command** runs with the following disabled, so that the repository's config cannot run commands
  through them:
  - hooks: hook files (`core.hooksPath` set to a folder that never exists: `/dev/null`, or `.no-hooks` in the
    plugin folder on Windows), and the `hook.<name>.command` hooks the repository defines in its config; no hook
    file runs, those of a global hooks folder included, so commits skip pre-commit, commit-msg and the others;
  - `core.fsmonitor`;
  - the repository's own filters (`filter.<driver>.clean`, `smudge`, `process`, from `.git/config`,
    `config.worktree` or an included file); filters from the global and system config, such as git-lfs, are kept;
  - submodules: ignored, and never recursed into (switch, restore, fetch, pull, push), since their own config is out
    of reach;
  - external diff tools and textconv filters, in every diff;
  - `log.showSignature`, which would run the repository's `gpg.program` on signed commits;
  - lazy fetching (`GIT_NO_LAZY_FETCH`): in a partial clone with a missing object, even `git status` would fetch it,
    running the promisor remote's `uploadpack` or ssh command.
- **Working tree**: a `core.worktree` set by the repository is ignored. The working tree stays the folder holding
  `.git`, so that switching branch, pulling or discarding cannot write elsewhere (in your home folder, for instance).
- **Arguments**: file names are given to git as literal paths, so a file named `*` or `[id]` never matches other
  files. A revision given to the diff pane that starts with `-` or contains spaces or control characters is
  refused; the others pass after `--end-of-options`. Branch and remote names starting with `-` are refused.
- **Source control** also:
  - for the commands that change something (stage, commit, switch, stash, fetch, pull, push…), replaces the
    repository's own `core.sshCommand`, `core.askPass`, `gpg.*.program`, credential helpers and merge drivers with
    the global values (or git's defaults), overrides `uploadpack` and `receivepack`, and ignores `core.gitProxy`
    (`GIT_PROXY_COMMAND` emptied);
  - allows only the `https`, `http`, `ssh` and `git` transports for fetch, pull and push: no `ext::` command, no
    local remote (it would run its own hooks), no remote helper (`git-remote-<name>`);
  - refuses fetch, pull and push when the repository's config sets `http.*`, `url.*.insteadOf`,
    `url.*.pushInsteadOf` or `remote.*.proxy` (which could send your credentials elsewhere),
    `core.alternateRefsCommand` (a command git runs during a fetch) or `remote.*.vcs` (a helper program that can live
    in the repository): run them in the terminal after checking;
  - never prompts for a password in the pane (`GIT_TERMINAL_PROMPT=0`) and stops network commands after 120 s; on
    Linux and macOS, network commands, commits and *Continue* run detached from the terminal, so that ssh or gpg
    cannot take over the pane;
  - deletes untracked files on discard only when they are inside the repository.
- **Terminal output**: control characters in file names, file contents and diffs are never sent to the terminal.
  The preview, the search and the diffs colour the text themselves, so a file cannot hide content with escape
  sequences.
- **Commit messages from Claude**: the prompt, the file list and the diff go to `claude -p` on its standard input,
  the diff capped at 60,000 characters (with nothing staged: the unstaged changes and up to 20 untracked files). The
  prompt tells Claude to treat the diff as data. Claude runs with no tools, no MCP servers, no saved session, user
  settings only, and from the temporary folder, so that the repository's `CLAUDE.md`, settings and hooks are not
  loaded; it is stopped after 90 s. The answer is reduced to one line of at most 200 characters, without control
  characters or backticks, and only fills the message field: you still commit. On Windows, a `claude.cmd` installed
  by npm runs through `cmd`, with fixed arguments.
- ***Insert into terminal*** refuses names containing control characters (a newline would run the command or submit
  the prompt), prefixes names starting with `-` with `./`, and puts the others in single quotes for the shell when
  they hold anything but plain characters (quoting valid in PowerShell and POSIX shells).
- **Opening**: double-click and *Open in app* never start executables, scripts or shortcuts. Names typed for new or
  renamed files cannot leave their folder. *Duplicate* copies symbolic links as links.
- **Preview**: only regular files are read (a named pipe or a device would block it), at most 8 MB of them.
  Highlighting is skipped for files over 1 MB and for lines over 2,000 characters; the hex dump covers the first
  64 KB. Images are decoded in JavaScript, with their size checked (50 megapixels, 256 MB of inflated PNG data) before anything is allocated.
- **Search**: files that are not regular, binary files and files over 2 MB are skipped, and symbolic link loops are
  cut.
- **Updates**: the release check only reads a version number (digits and dots) from the GitHub API; nothing it
  returns reaches a command. The update only runs `git pull --ff-only` in the plugin's own clone, with no terminal
  prompt, and never runs the installer.
- **Windows**: the clipboard and trash calls pass paths to PowerShell through environment variables, never inside
  the command, and VS Code is started through `Code.exe` rather than `cmd`.

## Limitations

- With `$HOME`, one of its parents or `/` as the root, or when a folder of the tree cannot be read, only the top
  level is watched: changes deeper down show up within 30 s.
- Source control cannot fetch, pull or push with a remote that is a local folder or that uses a remote helper
  (`hg::`, `codecommit::`…) (see [Security](#security)): use the terminal for those. A partial clone cannot fetch
  its missing objects from the explorer or source control either.
- herdr enforces a minimum of 10 % of the width per pane: in a narrow window, the explorer is wider than requested.
  This is also why it closes instead of collapsing into a narrow strip.
- When a tab opens, the split shows half-and-half for a few tens of milliseconds before taking its width:
  `herdr plugin pane open` cannot set the size at creation.
- In a tab with several panes, `prefix+f` places the explorer next to the top-left (or top-right) pane, which may
  not span the full height.
- Submodules: their changes are not shown, and switching branch, pulling or discarding does not update them (run
  `git submodule update` in the terminal), since their own config could run commands.
- After a herdr server restart, only the explorer and the preview come back: source control, diffs, the editor and
  the search popup stay closed. An explorer with nothing saved opens at the tab terminal's folder; a preview whose
  file is gone stays closed.
- *Insert into terminal* quotes for PowerShell and POSIX shells, not for `cmd.exe`: there, a name containing `&`,
  `|`, `<`, `>`, `^` or `%` could still run a command once `Enter` is pressed.
- *Open in app* opens HTML and SVG files in the browser, and Office documents in Office, which asks before enabling
  macros.
- Preview: files over 8 MB show their start only, so larger images are not drawn; highlighting is approximate (a
  tokenizer, not a parser). Progressive, lossless, hierarchical or arithmetic-coded JPEG, WebP and SVG images show
  their size only; GIF and APNG show their first frame; image quality is limited to two pixels per cell. Mermaid
  diagrams are limited to flowcharts (`flowchart`, `graph`), drawn as boxes in their order of appearance, without
  subgraphs or styles. A selection is cleared when the file reloads, the pane is resized, or wrap, line numbers or
  the Markdown view are toggled.
- The explorer's filter reads at most 20,000 entries and does not search git-ignored folders.
- Search: the content search matches line by line (no multi-line regex), skips binary files and files over 2 MB,
  and stops at 5,000 matches (lines over 1,000 characters are cut around the first match); *Go to file*
  keeps the best 1,000; a folder walk stops at 50,000 files.
- Source control: conflicts show as a diff with their markers, not a 3-way merge; history stops at 500 commits;
  edits made outside show up within 5 s; fetch, pull and push cannot ask for a password (use an SSH agent or a
  credential helper). A diff stops at 200,000 rows and 200 untracked files.
- Updates need the plugin folder to be a git clone on `main`, without changes to tracked files.
- A pane restarted on new code keeps its previous `node` process, which only waits for the new one: after N version
  changes (updates, or a `git checkout` between versions), an explorer or preview holds N+1 `node` processes until
  it is closed. Close and reopen it (`prefix+f` twice) to start from one.

## Troubleshooting

- **Agents not resumed after a herdr restart** (`claude --resume <id>` shown at the top of the pane, but a plain
  prompt below): herdr types the resume command into a new shell, and a program run at shell startup that reads
  the terminal swallows it. On Bazzite and other Universal Blue systems, the login banner (`ublue-motd`, drawn by
  `glow`) does. Skip it in herdr panes only, at the top of `~/.bashrc`, before `/etc/bashrc` is sourced:

  ```bash
  [ "${HERDR_ENV:-}" = 1 ] && USERMOTDSOURCED=Y
  ```

  Or turn it off everywhere with `touch ~/.config/no-show-user-motd`.
- **Explorer panes showing a shell after a restart**: herdr did not run the startup hook, usually because the plugin
  was linked before the hook existed. Run `herdr plugin link .` in this folder, then restart herdr
  (`herdr server stop`, then `herdr`). `herdr plugin log` shows whether the hook ran, and its errors.

- **Nothing opens at all**: `node` is not on the `PATH` that herdr sees.
- **Icons shown as boxes**: the terminal font is not a Nerd Font (v3); a VS Code icon theme still needs one for the
  chevrons and header icons.
- **`prefix+f`, `prefix+a` or `prefix+d` do nothing**: check the `[[keys.command]]` blocks in `config.toml` and that
  `herdr plugin list` shows `gcaudan.explorer`, or run the script again.
- **No `↑` although a release is out**: `updateCheck` is off, the check already ran today, or the clone is ahead of
  the release. Delete `update.json` to check again at the next tab creation.

Messages of the plugin:

| Message | Cause and fix |
|---|---|
| `git not found in PATH` | git is looked up in the absolute folders of `PATH` only |
| `No terminal editor found: set $EDITOR.` | set `$EDITOR` or `$VISUAL`, or install nano or vim |
| `<command> not found` when opening in the editor | fix *Editor command…* in the settings; on Windows, give an `.exe` |
| `Clipboard unavailable`, or `(OSC 52)` after a copy | install `wl-copy` or `xclip`; OSC 52 works only if herdr and the terminal pass it on |
| `No trash: delete … for good?` | install `gio` or `trash-put` to keep deletions recoverable |
| `Fetching refused: the repository sets …` (or Pulling, Pushing, Publishing) | the repository's own config sets network settings: check them, then run the command in the terminal |
| `Not a git clone`, `Not on main`, `Local changes in the plugin folder`… on update | switch the clone to `main`, or commit or stash your changes, then update again |
| `… : update command copied` | the pull failed (network, diverged history, ssh passphrase): run the copied command in a terminal |
| `Updated to X: rerun the installer`, or `!` in the header | the manifest or the installer changed: click the `!` to copy the installer command, then run it in a terminal |
| `Update in progress` | another explorer is updating; a lock left by a killed update is taken over after 2 minutes |
| `Not a git repository` | the folder is not in a repository; `i` in source control runs `git init` |
| `No terminal next to the explorer` | *Insert into terminal* found no terminal pane beside the explorer |
| `No editor: set one in the settings` | opening a folder needs an editor command or VS Code |
| `Not opened: file type not allowed` | *Open in app* only takes documents, media and a few text formats |
| `Continue refused: …` | the rebase or cherry-pick would run commands or a merge strategy of its own: check it, then continue in the terminal |
| `Claude not available` | install the `claude` CLI and sign in; meanwhile, the message is made from the file names |

## Project structure

| File | Role |
|---|---|
| `herdr-plugin.toml` | manifest: startup hook, `tab.created` event, panes, `toggle`, `search` and `scm` actions |
| `hook.js` | opens the explorer next to a tab, or closes it; reopens the panes after a restart; search and source control actions |
| `explorer.js` | the explorer: tree, git status, filter, file operations, menu, settings, width |
| `preview.js` | file preview: text, Markdown, images, hex dump, find, mouse selection |
| `highlight.js`, `markdown.js`, `image.js` | syntax highlighting, Markdown rendering, image decoding |
| `search.js` | go to file and search in files popup |
| `scm.js` | source control pane |
| `diff.js` | git diff viewer, inline or side by side; also used by `scm.js` |
| `edit.js` | terminal editor pane |
| `actions.js` | file actions: open, preview, insert, copy, reveal, create, rename, trash, stage, diff |
| `icons.js` | Nerd Font icons, and VS Code icon themes |
| `update.js` | daily check for a newer release, update by `git pull --ff-only`, restart of the panes on the new code |
| `ui.js` | terminal helpers shared by the panes: text width, lines, input, text field |
| `exec.js` | runs external programs from their absolute path, safe git options, settings, saved pane state, key bindings |
| `install.sh`, `install.ps1` | installation: `herdr plugin link` and key bindings |

## License

[MIT](LICENSE)
