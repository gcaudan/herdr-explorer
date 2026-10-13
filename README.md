# herdr-explorer

A [herdr](https://github.com/herdrdev/herdr) plugin that adds a VS Code-style sidebar to every tab: the space's
file tree with live git status, a file preview, file and content search, and a source control panel. Plain Node.js,
no dependencies and no build step.

## Features

- **Explorer**, opened automatically on the left (or the right) of every tab, without stealing focus:
  - folders expanded with the mouse or the keyboard, remembered between sessions;
  - live git status: `M` modified, `U` untracked, `A` added, `R` renamed, `!` conflict, `●` on folders containing
    changes, git-ignored entries greyed out, space name and branch in the header;
  - filter as you type (`/`): the tree is limited to the names containing the text;
  - new file, new folder, rename, duplicate, move to the trash; stage, unstage or discard a file or folder;
  - another folder as the root (*Set as root*, `Backspace` for the parent folder);
  - optionally, follows the folder of the terminal next to it;
  - Nerd Font icons, adjustable width that is remembered;
  - reopened after a herdr restart, at the same folder, with the preview at the same file.
- **Preview**, a pane opened next to the terminal and reused for every file of the tab:
  - syntax highlighting for 27 languages and formats;
  - rendered Markdown: headings, lists, task lists, tables, code blocks, quotes, GitHub alerts, Mermaid flowcharts;
  - images (PNG, baseline JPEG, GIF, BMP, ICO) drawn with half blocks, a hex dump for other binary files;
  - reloads by itself when the file changes on disk, for instance written by an agent;
  - find in file, wrap, line numbers, mouse selection copied to the clipboard, full-tab zoom.
- **Search**, a popup over the tab:
  - *Go to file* (fuzzy, `path:line` to go to a line);
  - *Search in files* with case, whole word and regex toggles, include and exclude globs;
  - a preview of the selected result on wide screens.
- **Source control**, a full-tab pane:
  - staged changes, changes and conflicts, as a list or a tree; stage, unstage, discard;
  - branch, ahead and behind counts, and an ongoing merge, rebase, cherry-pick, revert or bisect in the header;
  - inline or side-by-side diffs with word-level highlights;
  - commit (amend too), with a message written by the `claude` CLI when available;
  - branches (switch, create, delete), history, stashes, tags, remotes, worktrees; fetch, pull, push.
- Opens files in your editor (a command set in the settings, else VS Code when installed, else `$EDITOR` in the
  terminal), in a terminal editor, or in the default application for documents and media.
  Executables and scripts are never run.
- Context menu: insert the path into the terminal, copy the path, reveal in the file manager, view git changes.

## Requirements

- herdr 0.9.3 or later
- Node.js 20 or later
- git, optional: without it, no git status, changes or source control
- A [Nerd Font](https://www.nerdfonts.com/) in the terminal running herdr, otherwise icons show as empty boxes.
- Optional: a graphical editor (VS Code by default), a clipboard tool and a trash tool (see
  [Installation](#installation) for each system; without a clipboard tool, copies go through OSC 52), and the
  [Claude Code](https://claude.com/claude-code) CLI (`claude`) for commit messages.

Tested in herdr 0.9.3 on Linux (Fedora, Wayland): the explorer with the mouse and the keyboard, the preview, the
search popup, source control, diff, editors, file operations, settings, `keys.json`, follow mode, right docking and
the `prefix` key bindings, and the explorer and preview coming back after a herdr server restart. Version 0.1 of the
explorer was also tested on Windows 11; the new panes are not verified on Windows or macOS, and opening in VS Code
was not tested (fallback to the terminal editor only).

## Installation

### Linux and macOS

```sh
git clone https://github.com/gcaudan/herdr-explorer.git
cd herdr-explorer
./install.sh
```

- Key bindings are added to `~/.config/herdr/config.toml`.
- Optional tools: VS Code with `code` in `PATH` (or another editor set in the settings); on Linux, `wl-copy`
  (Wayland) or `xclip` (X11) for the copy actions, and `gio` or `trash-put` for the trash. On macOS, `pbcopy` and
  the trash are built in.
- Install a Nerd Font (for instance `JetBrainsMono Nerd Font` from your package manager or
  [nerdfonts.com](https://www.nerdfonts.com/)) and select it in your terminal.

### Windows

```powershell
git clone https://github.com/gcaudan/herdr-explorer.git
cd herdr-explorer
.\install.ps1
```

- If PowerShell blocks the script, run `powershell -ExecutionPolicy Bypass -File .\install.ps1`.
- Key bindings are added to `%APPDATA%\herdr\config.toml`.
- The clipboard and the recycle bin go through PowerShell, which is built in; VS Code is found in `PATH` or in its
  default user install folder. Another editor must be an `.exe` (a `.cmd` launcher cannot run without a shell).
- Install a Nerd Font with `winget install DEVCOM.JetBrainsMonoNerdFont`, then select it in Windows Terminal.

### What the script does

It links the plugin to herdr from this folder (`herdr plugin link`), adds the key bindings below to `config.toml`
when missing, then reloads the configuration. It is safe to run again, for instance after moving the folder.

| Key | Action |
|---|---|
| `prefix+f` (`ctrl+b` then `f`) | open or close the explorer of the current tab |
| `prefix+a` | go to file |
| `prefix+d` | source control |

herdr keeps its own copy of the manifest: after pulling a new version, run the script again (or
`herdr plugin link .`) so that herdr picks up new hooks, such as the startup hook.

To add the explorer to already open tabs that have a single pane, run `node hook.js --all` in this folder from a
herdr pane.

### Uninstall

```sh
herdr plugin unlink gcaudan.explorer
```

Then remove the `[[keys.command]]` blocks of `gcaudan.explorer.*` from `config.toml` and run
`herdr server reload-config`. The settings and state stay in the plugin's state and config folders (see
[Configuration](#configuration)): delete them if you do not reinstall.

## Usage

### Explorer

| Mouse | Effect |
|---|---|
| Click a folder | expand or collapse it |
| Click a file | select it; with *Preview on single click*, show it in the preview |
| Double-click a file | open it: preview, or the editor depending on the settings; documents and media in their application |
| Right-click an entry | its action menu |
| Right-click below the last entry | the root's menu: new file, search, source control, up one level, settings… |
| Wheel | scroll the tree, or move in an open menu |
| Header icons | search, source control, settings, `«` to close the explorer |

Default keys; each one can be changed (see [Key bindings](#key-bindings)), under the action name given here.

| Key | Action | Effect |
|---|---|---|
| `↑` `↓`, `k` `j` | `up`, `down` | move |
| `PageUp` `PageDown` | `pageUp`, `pageDown` | move by one page |
| `Home` `End`, `g` `G` | `top`, `bottom` | go to the top or bottom |
| `Enter`, `Space` | `open` | open a file, expand or collapse a folder |
| `→`, `l` | `expand` | expand a folder, or go to its first entry when it is already expanded |
| `←`, `h` | `collapse` | collapse a folder, or go to the parent folder |
| `Esc`, `q` | `close` | close the filter or the menu |
| `/` | `filter` | filter; `Esc` closes it and shows the selected entry in the tree |
| `ctrl+p` | `goToFile` | go to file |
| `ctrl+f` | `searchInFiles` | search in files (in the selected folder) |
| `s` | `sourceControl` | source control |
| `p` | `preview` | preview the file and focus the preview |
| `e` | `edit` | edit in the terminal editor |
| `o` | `openInEditor` | open in the editor |
| `n`, `N` | `newFile`, `newFolder` | new file, new folder (`a/b/c.txt` creates the folders) |
| `R`, `F2` | `rename` | rename |
| `Delete` | `delete` | move to the trash (after confirmation) |
| `y`, `Y` | `copyPath`, `copyRelativePath` | copy the path, the relative path |
| `.`, `i` | `toggleHidden`, `toggleIgnored` | show or hide dot files, git-ignored files |
| `Backspace` | `parentRoot` | the parent folder becomes the root |
| `,` | `settings` | settings |
| `m`, `Shift+F10`, Menu key | `menu` | action menu |
| `r` | `refresh` | force a refresh (a full scan also runs every 30 s) |

### Action menu

| Item | Effect |
|---|---|
| Preview | shows the file in the preview pane |
| Open in editor | the editor command of the settings, else VS Code, else the terminal editor |
| Open in app | the default application of the system; documents, media and text files only (no executables or scripts) |
| Edit in terminal | the terminal editor of the settings, else `$VISUAL`, `$EDITOR`, else nano, nvim, vim or vi, in a full-tab pane |
| Insert into terminal | types the path, relative to the folder of the terminal, and focuses it; prefixed with `@` when an agent such as Claude Code runs there (quoted instead if the name contains spaces) |
| New file, New folder, Rename, Duplicate, Delete | Delete moves to the trash, and asks before deleting permanently when there is no trash |
| Search in folder, Set as root | on a folder |
| Reveal in Explorer / Open folder, Copy path, Copy relative path | |
| View changes | full-tab git diff against the last commit, untracked files shown in full |
| Stage, Unstage, Discard changes | discarding restores tracked files from the last commit and moves untracked ones to the trash |

The menu moves with `up` and `down` or the wheel, runs an item with `open`, `expand` or a click, and closes with
`Esc`. Name prompts take the usual line-editing keys (`ctrl+a`, `ctrl+e`, `ctrl+u`, `ctrl+k`, `ctrl+w`…);
confirmations take `y`, any other key cancels.

### Preview

| Key or mouse | Effect |
|---|---|
| `↑` `↓`, `j` `k`, `Enter`, wheel, `PageUp` `PageDown`, `Space` `b`, `ctrl+d` `ctrl+u`, `g` `G` | scroll |
| `←` `→`, `h` `l` | scroll sideways (without wrap) |
| `/` or `ctrl+f`, then `Enter`; `n` `N` | find in the file (case-insensitive unless the text has a capital letter) |
| `w`, `L`, `m` | wrap, line numbers (both saved for every preview), rendered or raw Markdown |
| Drag with the left button | selects lines, copied to the clipboard when released |
| Header chips | rendered or raw Markdown, wrap or no wrap, `✕` close |
| `e`, `o`, `O` | edit in the terminal editor; open (documents in their application, the rest in the editor); open in the default app |
| `y`, `Y` | copy the path, the relative path |
| `ctrl+p` | go to file |
| `z` | zoom the preview to the full tab, and back |
| `r`, `q` or `Esc` or `ctrl+c` or `✕` | reload; close the find bar, else the pane |

### Search

- *Go to file* scores paths like fzf and VS Code (start of segments and words, file name, consecutive characters);
  `path:line` or `path:line:column` opens the file at that line.
- *Search in files* takes plain text or a regular expression; the include and exclude fields take comma-separated
  globs (`*`, `?`, `**`, `{a,b}`, `[abc]`): a pattern without a leading `/` or `./` matches at any depth, and a
  folder matches its contents. Results are grouped by file.
- The files searched are those git tracks or does not ignore; outside a repository, or with *hidden* on, the folder
  is walked. From 100 columns, the selected result is previewed on the right. The last queries, globs and options
  come back at the next opening, selected: typing replaces them.

| Key | Effect |
|---|---|
| `ctrl+p`, `ctrl+f` | go to file, search in files (the header tabs too) |
| `alt+c`, `alt+w`, `alt+r`, `alt+h` | case, whole word, regex, hidden and ignored files (the chips too) |
| `Tab`, `Shift+Tab` | query, include globs, exclude globs (`*.js, src/**`) |
| `↑` `↓` (or `ctrl+k`, `ctrl+j` `ctrl+n`), `PageUp` `PageDown`, `ctrl+Home` `ctrl+End`, click | select; `alt+←` `alt+→` fold a file's results |
| `Enter`, double-click | open in the preview at the line |
| `ctrl+o` (or `alt+Enter`, which Windows Terminal keeps for full screen), `ctrl+t`, `ctrl+y` | open in the editor, insert the path into the terminal, copy the relative path |
| `Esc`, `ctrl+c` | close |

### Source control

A full-tab pane on the repository of the tab's folder (`prefix+d`, `s` in the explorer, or its header icon). Its
header shows the branch (click to switch), the ahead and behind counts or *not published*, an ongoing operation, and
the Fetch, Pull, Push, refresh and close buttons. The selected row shows `+` stage, `−` unstage and `↶` discard
buttons. Enter on a file opens its diff on the right from 100 columns, over the list below; diffs are side by side
from 120 columns. Committing with nothing staged offers to stage everything first. Outside a repository, `i` runs
`git init` after confirmation.

| Key | Effect |
|---|---|
| `1` `2` `3` `4` | changes, history (last 500 commits), stashes, refs (branches, tags, remotes, worktrees) |
| `←` `→`, `h` `l` | collapse, expand a section or folder |
| `s` or `Space`, `u`, `a`, `A` | stage, unstage, stage all, unstage all |
| `d` | discard (asks first); untracked files are **deleted for good**, not moved to the trash |
| `Enter`, double-click | diff of a file, commit, stash or tag; in Refs, checkout the branch; `Tab` switches between the list and the diff |
| `n` `N`, `s` | in the diff: next and previous hunk, inline or side by side |
| `c`, `Enter`, `Esc` | type the commit message, commit, leave the field; `alt+a` amend |
| `ctrl+g` or `✧` | commit message written by Claude, or from the file names without it |
| `c` `Enter` or `✓ Continue`, `X` or `✕ Abort` | during a rebase, cherry-pick or revert: continue once the conflicts are staged (git's message kept; refused when it would run `exec` lines or a merge strategy that is not git's), or abort (asks first); a merge ends with a commit, its message optional |
| `b` or click on the branch | branches: filter, `Enter` switch, `n` new, `D` delete |
| `f`, `P`, `p` | fetch, pull (fast-forward only), push (sets the upstream on the first push) |
| Stashes | `n` stash (untracked files included), `a` apply, `o` pop, `d` drop |
| Refs | `D` delete the local branch (asks again with `-D` when it is not fully merged) |
| `t`, `r`, `q` | list or tree, refresh, close |

## Configuration

- **Settings** (`,` or the gear icon), saved in `settings.json` and shared by every explorer and preview:

  | Setting | Menu item | Default | Effect |
  |---|---|---|---|
  | `hidden` | Show hidden files | `true` | show dot files |
  | `ignored` | Show git-ignored files | `true` | show git-ignored files, greyed out |
  | `open` | Open in the preview | `"preview"` | Enter and double-click on a text file: `"preview"` or `"editor"` |
  | `editor` | Editor command… | `""` | external editor command; empty: VS Code when found, else the terminal editor |
  | `singleClick` | Preview on single click | `false` | a single click on a file shows it in the preview |
  | `follow` | Follow the terminal folder | `false` | reveal the folder of the terminal in the tree, within the space folder |
  | `side` | Dock on the right | `"left"` | side of the explorer, applied to the tabs opened afterwards |
  | `wrap` | (preview `w`) | `false` | preview: wrap long lines |
  | `lineNumbers` | (preview `L`) | `true` | preview: line number gutter |

  The editor command (`zed`, `subl -w`, `code -r`…) runs without a shell, with the path as last argument. A terminal
  editor (`vi`, `vim`, `nvim`, `nano`, `micro`, `hx`, `helix`, `kak`, `emacs`, `ne`, `joe`, `mcedit`, `edit`) runs
  in the full-tab editor pane instead, and is also used by *Edit in terminal*. *Edit key bindings…* opens `keys.json`
  (see below).
- **Width**: 32 columns by default. When resized by dragging the border, the width is kept for the next openings
  and restored when the window is resized.
- **State**: the settings (`settings.json`), the width (`width`), the expanded folders of each space
  (`expanded.json`), the last searches (`search.json`) and what each explorer and preview pane shows (`panes/`,
  read back after a herdr restart) are stored in the plugin state folder:
  `~/.local/state/herdr/plugins/gcaudan.explorer` on Linux, `%LOCALAPPDATA%\herdr\plugins\gcaudan.explorer` on
  Windows. Deleting `width` resets the width to 32 columns.

### Key bindings

The explorer's keys are read from `keys.json` in the plugin config folder (`herdr plugin config-dir gcaudan.explorer`
prints it). *Settings › Edit key bindings…* creates the file with every action and its current keys, and opens it in
the terminal editor; changes apply within a second, without restarting the explorer.

```json
{
  "explorer": {
    "newFile": ["a"],
    "delete": ["delete", "ctrl+d"],
    "down": ["down", "j"]
  }
}
```

- An action left out keeps its default keys; listing keys for an action replaces all of its default keys.
- A key given to an action is taken from the action that had it by default; a key listed for two actions goes to
  the last one.
- Key names: `up`, `down`, `left`, `right`, `home`, `end`, `pageup`, `pagedown`, `enter`, `space`, `tab`,
  `shift+tab`, `backspace`, `insert`, `delete`, `escape`, `menu`, `shift+f10`, `f1` to `f5`, `ctrl+<letter>`,
  `ctrl+up` and the other arrows, `shift+up`, `shift+down`, `alt+up`, `alt+down`, `alt+<key>`, or the character
  itself (`/`, `n`, `N`).
- Only the explorer's keys can be changed: the preview, search and source control keys are fixed.
- The menu uses `up`, `down`, `open` and `expand` (run the item), `close` and `collapse` (close it). While filtering,
  characters go to the filter and only the keys that are not characters (arrows, `Enter`, `ctrl+…`) act on the
  list. `Esc` always closes the menu and the filter; text fields (names, filter) keep their editing keys.

## How it works

- **Placement**: herdr only splits panes to the right or downwards. The hook therefore opens the explorer on the
  right, sets its width, swaps it with the terminal, then gives focus back to the terminal. Docked on the right,
  there is no swap. If the tab was created in the background, the display briefly switches to it, then returns to
  the tab being viewed.
- **Preview**: opened by splitting the terminal (explorer | terminal | preview). The next files of the tab are sent
  to the same pane through a request file of the state folder, `preview-<tab>.json`.
- **herdr restart**: herdr restores the plugin's panes as plain shells, its session keeping only their label and
  folder. Each explorer and preview therefore saves what it shows in `panes/<pane id>.json` (space folder and tree
  root; file and line). A startup hook (`node hook.js --restore`, run once herdr has restored the session) closes
  the panes of the plugin that run no `node` process, then reopens the explorer and the preview from those files,
  and deletes the files of panes that no longer exist. herdr keeps pane ids across a restart.
- **Refresh**: the explorer watches its root (recursively, except on your home folder, one of its parents or `/`)
  and the repository's `index` and `HEAD`, refreshing at most every 250 ms, with a full scan every 30 s as a safety
  net. Source control polls the status every 5 s and watches the git folder and `refs/`.
- **State files** are written to a temporary file then renamed, so that another pane never reads them half-written.
- **Closing**: when the tab's terminal exits, the explorer closes too, and so does the tab.
- **Right-click**: herdr keeps right-clicks for its own menu unless the pane asks for them; the explorer does so at
  startup (`herdr pane input --right-click pane`).

## Security

The explorer opens by itself on any folder, including freshly cloned untrusted repositories. Therefore:

- `git`, `herdr` and the other programs are run from their absolute path, found in the absolute folders of `PATH`:
  on Windows, a bare program name is first looked up in the working directory, and elsewhere an empty or `.` entry
  of `PATH` means that folder too, where a repository could plant a `git`.
- Every git command runs with these disabled, so that a repository's config cannot run commands through them:
  - hooks: hook files (`core.hooksPath` set to a folder that never exists: `/dev/null`, or on Windows `.no-hooks`
    in the plugin folder), and the `hook.<name>.command` hooks the repository defines in its config;
    commits therefore skip the repository's hooks (pre-commit, commit-msg…);
  - `core.fsmonitor`;
  - the repository's own filters (`filter.<driver>.clean`, `smudge`, `process`, from `.git/config`,
    `config.worktree` or included files); filters from the global and system config, such as git-lfs, are kept;
  - submodules: ignored, and never recursed into (switch, restore, fetch, pull, push), since their own config is out
    of reach;
  - external diff tools and textconv filters, in every diff;
  - `log.showSignature`, which would run the repository's `gpg.program` on signed commits;
  - lazy fetching (`GIT_NO_LAZY_FETCH`): in a partial clone with a missing object, even `git status` would fetch it,
    running the promisor remote's `uploadpack` or ssh command.
- A `core.worktree` set by the repository is ignored: the working tree stays the folder holding `.git`, so that
  switching branch, pulling or discarding cannot write elsewhere (in your home folder, for instance).
- File names are given to git as literal paths: a file named `*` or `[id]` never matches other files. A revision
  given to the diff pane that starts with `-` or contains spaces or control characters is refused, the others pass
  after `--end-of-options`; branch and remote names starting with `-` are refused.
- Source control also:
  - replaces the repository's own `core.sshCommand`, `core.askPass`, `core.gitProxy`, `gpg.*.program`,
    credential helpers and merge drivers with the global values, and overrides `uploadpack` and `receivepack`;
  - only allows the `https`, `http`, `ssh` and `git` transports for fetch, pull and push: no `ext::` command, no
    local remote (it would run its own hooks), no remote helper (`git-remote-<name>`);
  - refuses fetch, pull and push when the repository's config sets `http.*`, `url.*.insteadOf` or
    `remote.*.proxy`, which could send your credentials elsewhere, `core.alternateRefsCommand`, a command git
    runs during a fetch, or `remote.*.vcs`, a helper program that can live in the repository: run them in the
    terminal after checking;
  - never prompts for a password in the pane (`GIT_TERMINAL_PROMPT=0`), and stops network commands after 120 s;
    network commands and commits run detached from the terminal, so that ssh or gpg cannot take over the pane;
  - deletes untracked files on discard only when they are inside the repository.
- Control characters in file names, file contents and diffs are never sent to the terminal: the preview, the search
  and the diffs colour the text themselves, so a file cannot hide content with escape sequences.
- Commit messages from Claude: the diff goes to `claude -p` on its standard input, capped at 60,000 characters,
  with a prompt telling it to treat the diff as data; with no tools, no MCP servers, no session saved, and from the
  temporary folder so that the repository's `CLAUDE.md`, settings and hooks are not loaded. The answer is reduced
  to one line without control characters, and only fills the message field: you still commit.
- *Insert into terminal* refuses names containing control characters (a newline would run the command or submit
  the prompt), prefixes names starting with `-` with `./`, and quotes the others for the shell (single quotes,
  for PowerShell and POSIX shells).
- Double-click and *Open in app* never start executables, scripts or shortcuts. Names typed for new or
  renamed files cannot leave their folder. *Duplicate* copies symbolic links as links.
- The preview only reads regular files (a named pipe or a device would block it), at most 8 MB of them; it
  highlights the first 1 MB and hex-dumps the first 64 KB. Images are decoded in JavaScript, with their size
  checked (50 megapixels) before anything is allocated.
- The search skips files that are not regular, binary files and files over 2 MB, and cuts symbolic link loops.
- On Windows, the clipboard and trash calls pass paths to PowerShell through environment variables, never inside
  the command, and VS Code is started through `Code.exe` rather than `cmd`.

## Limitations

- With `$HOME`, one of its parents or `/` as the root, or when a folder of the tree cannot be read, only the top
  level is watched: changes deeper down show up within 30 s.
- Source control cannot fetch, pull or push with a remote that is a local folder or that uses a remote helper
  (`hg::`, `codecommit::`…) (see [Security](#security)): use the terminal for those. A partial clone cannot fetch
  its missing objects from the explorer or the source control either.
- herdr enforces a minimum of 10 % of the width per pane: in a narrow window, the explorer is wider than requested.
  This is also why it closes instead of collapsing into a narrow strip.
- When a tab opens, the split shows half-and-half for a few tens of milliseconds before taking its width:
  `herdr plugin pane open` cannot set the size at creation.
- In a tab with several panes, `prefix+f` places the explorer next to the top-left (or top-right) pane, which may
  not span the full height.
- Submodules: their changes are not shown, and switching branch, pulling or discarding does not update them (run
  `git submodule update` in the terminal): their own config could run commands.
- After a herdr server restart, only the explorer and the preview come back: source control, diffs, the editor
  and the search popup stay closed. An explorer with nothing saved opens at the tab terminal's folder; a preview
  whose file is gone stays closed.
- *Insert into terminal* quotes for PowerShell and POSIX shells, not for `cmd.exe`: there, a name containing
  `&`, `|`, `<`, `>`, `^` or `%` could still run a command once Enter is pressed.
- *Open in app* opens HTML and SVG files in the browser, and Office documents in Office, which asks
  before enabling macros.
- Preview: files over 8 MB show their start only, so larger images are not drawn; highlighting is approximate (a
  tokenizer, not a parser); progressive JPEG, WebP and SVG images show their size only, GIF its first frame; image
  quality is limited to two pixels per cell.
- The explorer's filter reads at most 20,000 entries and does not search git-ignored folders.
- Search: the content search reads files line by line (no multi-line regex), skips binary files and files over 2 MB,
  and stops at 5000 results; without git, the walk stops at 50,000 files.
- Source control: conflicts show as a diff with their markers, not a 3-way merge; history stops at 500 commits;
  edits made outside show up within 5 s; fetch, pull and push cannot ask for a password (use an SSH agent or a
  credential helper).

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
  (`herdr server stop`, then `herdr`). `herdr plugin log` shows whether the hook ran and its errors.

Messages of the plugin:

| Message | Cause and fix |
|---|---|
| `git not found in PATH` | git is looked up in the absolute folders of `PATH` only |
| `No terminal editor found: set $EDITOR.` | set `$EDITOR` or `$VISUAL`, or install nano or vim |
| `<command> not found` when opening in the editor | fix *Editor command…* in the settings; on Windows, give an `.exe` |
| `Clipboard unavailable`, or `(OSC 52)` after a copy | install `wl-copy` or `xclip`; OSC 52 works only if herdr and the terminal pass it on |
| `No trash: delete … for good?` | install `gio` or `trash-put` to keep deletions recoverable |
| `Fetching refused: the repository sets …` | the repository's own config sets network settings: check them, then fetch in the terminal |
| `Claude not available` | install the `claude` CLI and sign in; the message is made from the file names meanwhile |

## Project structure

| File | Role |
|---|---|
| `herdr-plugin.toml` | manifest: startup hook, `tab.created` event, panes, `toggle`, `search` and `scm` actions |
| `hook.js` | opens the explorer next to a tab, or closes it; reopens the panes after a restart; search and source control actions |
| `explorer.js` | the explorer: tree, git status, filter, file operations, menu, settings, width |
| `preview.js` | file preview: text, Markdown, images, hex dump |
| `highlight.js`, `markdown.js`, `image.js` | syntax highlighting, Markdown rendering, image decoding |
| `search.js` | go to file and search in files popup |
| `scm.js` | source control pane |
| `diff.js` | git diff viewer, inline or side by side; also used by `scm.js` |
| `edit.js` | terminal editor pane |
| `actions.js` | file actions: open, preview, insert, copy, reveal, create, rename, trash, stage, diff |
| `ui.js` | terminal helpers shared by the panes: text width, lines, input, text field |
| `exec.js` | runs external programs from their absolute path, safe git options, settings, saved pane state |
| `install.sh`, `install.ps1` | installation: `herdr plugin link` and key bindings |

## License

[MIT](LICENSE)
