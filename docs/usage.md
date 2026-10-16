# Usage

The plugin has six panes:

| Pane            | Opens as                     | Opened by                                                     |
| --------------- | ---------------------------- | ------------------------------------------------------------- |
| Explorer        | split, left or right of tab  | new tab, `prefix+f`                                           |
| Preview         | split, right of the terminal | `Enter`/double-click on a file, `p`, search results           |
| Search          | popup, 80% × 80%             | `ctrl+p`, `ctrl+f`, `prefix+a`                                |
| Source Control  | full tab (zoomed)            | `s`, `prefix+d`                                               |
| Changes         | full tab (zoomed)            | menu **View changes**                                         |
| Editor          | full tab (zoomed)            | `e`, **Edit in terminal**, `o` with a terminal editor         |

Key names below are those of [configuration.md](configuration.md#key-names). Only the explorer's
keys can be rebound.

## Explorer

### Header

The header shows the root folder's name in bold, or in bold italic when **Set as root** moved the
tree away from the tab's folder. A short message replaces it after an action: `✓` when it is done
(2 s), `✕` in red when it failed (4 s), `⋯` while it runs. The preview and source control show their
messages the same way, in their bottom line. The icons on the right are clickable:

| Icon      | Click                                                                  |
| --------- | ---------------------------------------------------------------------- |
| `↑`       | update to the newer release (green, only when one exists)              |
| `!`       | copy the installer command that finishes an update (yellow)            |
| gear      | Settings                                                               |

### Tree

Folders come first, then files, sorted by name with numbers in natural order. The `.git` folder is
never shown. Git decorations follow VS Code:

| Badge | Meaning                                                            |
| ----- | ------------------------------------------------------------------ |
| `U`   | untracked (also every file inside an untracked folder)             |
| `A`   | added                                                              |
| `R`   | renamed                                                            |
| `M`   | modified                                                           |
| `D`   | deleted in git but still on disk (`git rm --cached`); a file gone from disk has no row: the `●` of its folder shows it, and source control lists it |
| `!`   | merge conflict                                                     |
| `●`   | on a folder: something inside changed (coloured by the most severe) |
| grey  | git-ignored                                                        |

### Keys (defaults)

| Key                      | Action                                                                         |
| ------------------------ | ------------------------------------------------------------------------------ |
| `up` `k` / `down` `j`    | move                                                                           |
| `pageup` / `pagedown`    | move one page                                                                  |
| `ctrl+u` / `ctrl+d`      | move half a page                                                               |
| `home` `g` / `end` `G`   | first / last row                                                               |
| `?` `f1`                 | the key help page, see below                                                   |
| `enter` `space`          | folder: fold or unfold. File: open (see [Opening files](#opening-files))       |
| `right` `l`              | unfold a folder, or go to its first child when already open                    |
| `left` `h`               | fold a folder, or go to the parent folder                                      |
| `m` `menu` `shift+f10`   | action menu of the selected row                                                |
| `escape` `q`             | close the menu or the filter                                                   |
| `r`                      | refresh                                                                        |
| `<` / `>`                | 4 columns wider / narrower (hold to repeat), kept for the next explorers       |
| `shift+left` `shift+right` | move the inner border 4 columns that way (hold to repeat)                    |
| `/`                      | filter the tree                                                                |
| `ctrl+p`                 | Go to file                                                                     |
| `ctrl+f`                 | Search in files (in the selected folder, else the root)                        |
| `s`                      | Source control, on the selected file (on a folder: its first changed file)     |
| `,`                      | Settings                                                                       |
| `.`                      | show / hide dot files                                                          |
| `i`                      | show / hide git-ignored files                                                  |
| `n` / `N`                | new file / new folder, in the selected folder (or the selected file's folder)  |
| `R` `f2`                 | rename                                                                         |
| `delete`                 | move to the trash                                                              |
| `e`                      | edit the file in the terminal editor                                           |
| `o`                      | open in the external editor                                                    |
| `O`                      | open the file with its system application (documents, media, text formats)     |
| `p`                      | preview the file and focus the preview                                         |
| `y` / `Y`                | copy the absolute / relative path                                              |
| `backspace`              | make the parent folder the tree root                                           |

### Mouse

- **Click** a folder to fold or unfold it. Click a file to select it.
- **Double-click** a file (within 400 ms) to open it.
- **Single click** a file to preview it, when **Preview on single click** is on.
- **Right-click** a row for its action menu. Right-click below the last row for the root menu. The
  plugin routes right-clicks in this pane to the explorer instead of herdr's menu.
- **Wheel** scrolls 3 rows at a time. In a menu, it moves the selection.
- **Header icons**: see [Header](#header).
- **Scrollbar**: see [Scrollbars](#scrollbars).

### Filter

Press `/` to open a filter field at the bottom. The tree then shows only the entries whose name
contains the text (case-insensitive), with their parent folders dimmed, the match in bold blue, and a
count of matches. Git-ignored folders are not searched.

- Typed characters go to the field. Bound keys that are not characters (arrows, `enter`, `ctrl+…`)
  act on the list.
- `tab` leaves the field and keeps the filter, so the single-letter keys work again. The bottom line
  then shows the filter in blue and `esc clear`. Click it to type again.
- `escape` closes the filter and reveals the selected entry in the full tree.

### Prompts

**Rename** turns the row into a field, the cursor before the extension. **New file** and **New folder**
open the folder and add a field at the top of it, whose icon follows the name typed. The bottom line
shows the keys (`⏎ rename`, `⏎ create in src`, `esc cancel`). The editor command is typed on the
bottom line. `enter` accepts and `escape` cancels. The field supports `left`/`right`, `home`/`end`, `ctrl+a` `ctrl+e` `ctrl+b` `ctrl+f`
`ctrl+d`, `ctrl+u` (delete to start), `ctrl+k` (delete to end), `ctrl+w` / `alt+backspace` (delete
word) and paste. A name may contain subfolders (`a/b.txt`, created as needed). A name ending with `/`
creates a folder.

### Confirmations

Moving to the trash, deleting for good, discarding changes and updating ask first, in a card over the
pane: the question, the full name and its folder, a note when the action cannot be undone, then two
buttons. `left`/`right` (or `tab`) move between the buttons, Cancel being selected at first, and
`enter` presses the selected one. `y`, or a click on the action's button, confirms. Any other key or
click cancels. Source control confirmations accept only `y`.

### Opening files

`enter`, a double-click or the menu's first item decides as follows:

1. With **Open in the preview** on (the default), a file the preview can show goes to the preview.
   That is every file except documents and media: Office files, PDF, archives, audio and video.
   Images are shown.
2. Otherwise, documents and media (`png jpg jpeg gif webp bmp ico svg pdf doc docx xls xlsx ppt pptx
   odt ods odp zip 7z rar gz tgz mp3 mp4 wav mkv avi mov`) open with the system application.
3. Everything else goes to the editor. A double-click never runs an executable or a script.

### Action menu (on a file or folder)

| Item                     | Shown for                         | Action                                                       |
| ------------------------ | --------------------------------- | ------------------------------------------------------------ |
| Preview                  | files the preview can show        | show in the tab's preview                                    |
| Open in editor           | files and folders                 | external editor (see [configuration](configuration.md))      |
| Open in app              | `txt md csv log json xml html htm` and the media types above | system application |
| Edit in terminal         | files                             | Editor pane                                                  |
| Insert into terminal     | all                               | type the path into the terminal next to the explorer         |
| New file… / New folder…  | all                               | in the folder (or the file's folder)                         |
| Search in folder…        | folders                           | Search in files limited to that folder                       |
| Set as root              | folders                           | make the folder the tree root                                |
| Rename…                  | all                               |                                                              |
| Duplicate                | all                               | copy as `name copy.ext`, `name copy 2.ext`…                  |
| Delete…                  | all                               | move to the trash, after confirmation                        |
| Open folder / Reveal in Explorer (Windows) | all             | Windows and macOS select the entry in File Explorer or Finder; on Linux, the file manager opens the folder holding it |
| Copy path                | all                               | absolute path                                                |
| Copy relative path       | all                               | relative to the tree root                                    |
| View changes             | changed entries in a repository   | Changes pane                                                 |
| Stage                    | changed entries                   | `git add -A` on the path                                     |
| Unstage                  | entries in a repository, not ignored | restore the index from HEAD                               |
| Discard changes…         | changed entries                   | after confirmation, see below                                |

**Insert into terminal** finds the terminal next to the explorer, skipping the plugin's own panes. It
types the path relative to the terminal's folder followed by a space, then focuses the terminal.
When an agent runs there, the path is prefixed with `@` (and quoted when it contains spaces).
Otherwise it is quoted for the shell when needed.

**Delete…** moves the entry to the trash: `gio trash` or `trash-put` on Linux, `trash` or Finder on
macOS, the recycle bin on Windows. Without a trash, a second confirmation offers to delete it for good.

**Discard changes…** puts tracked files back to their last commit, staged changes included. Files
added since the last commit, and untracked files, go to the trash instead of being deleted.

### Root menu (right-click below the rows)

New file…, New folder…, Go to file…, Search in files…, Source control (in a repository), Open folder /
Reveal in Explorer, Copy path, Up one level, Back to the space folder (after **Set as root**),
Refresh, Settings….

### Settings (`,` or the gear icon)

The settings take the place of the tree, by group, each with its value on the right: `● on` or
`○ off` for a switch, the value chosen otherwise. A line under the list explains the selected
setting. The settings are shared by every explorer. See [configuration.md](configuration.md#settings).

| Group     | Settings                                                      |
| --------- | ------------------------------------------------------------- |
| Tree      | Hidden files, Git-ignored files, Follow terminal, Icon theme  |
| Opening   | Enter opens (preview or editor), Editor, Single-click preview |
| Next tabs | Dock side (left or right), Open automatically                 |
| Preview   | Wrap lines, Line numbers, Diff layout (auto, split, inline)   |
| General   | Key bindings, Check for updates                               |

`up`/`down` (and their bound keys, page and first/last included) move, `/` finds a setting.
`right`/`l` and `enter` switch a setting or give it its next value, `left`/`h` its previous one. On
**Icon theme**, **Editor** and **Key bindings**, they open the icon themes page, the editor command
prompt or the key bindings page. When a newer release exists, a line under the title offers it: `u`
updates (or finishes the update). `escape`, and the keys bound to close and settings, go back to the
tree. The wheel moves the selection, a click on a setting changes it, and a click on the title goes
back.

**Icon theme** opens a page of its own: the Nerd Font default and the VS Code icon themes installed
by VS Code, VS Code Insiders, VSCodium, Cursor or Windsurf, the saved one marked `✓`. Below the list,
a sample tree of folders and files shows the theme under the selection. `enter` (or a click) keeps it
and goes back to the settings; `escape` goes back without a change.

**Key bindings** opens a page of its own: the explorer's actions by group, as in the key help, with
their keys (in yellow when changed). Below the list, the selected action shows its keys and its
default ones.

| Key                  | Action                                                                      |
| -------------------- | --------------------------------------------------------------------------- |
| `enter`              | wait for a key and add it to the action (taken from the action that had it) |
| `delete` `backspace` | remove the action's last key                                                |
| `r`                  | give the action back its default keys                                       |
| `/`                  | find an action                                                              |
| `escape` `left`      | back to the settings (while waiting for a key: cancel)                      |

The arrows always move in this page, whatever the bindings. A click selects an action, a second click
waits for its key. The last row, **Open keys.json**, opens the file in the editor. The changes are
saved in [`keys.json`](configuration.md#config-folder-keysjson) and apply at once.

### Key help (`?` or `f1`)

Every key of the explorer, as bound now, takes the place of the tree, over the pane's full height:
the keys on the left, what they do on the right, by group. The arrows, page keys and the wheel
scroll it, `/` finds a key or what it does. `escape`, or a click on the title, goes back to the tree.
Keys are changed on the **Key bindings** page of the settings. The preview, the search popup, source
control and the changes pane show their keys the same way (`?` or `f1`; `f1` only in the search).

### Find in the pages

In the settings, the key bindings and the key help, `/` opens a find field under the title. The list
keeps the rows whose name, value, keys or help has the text typed, and the field shows how many. Typed
characters go to the field. The arrows and `enter` still act on the list. `escape` closes the find,
and the selection stays on its row.

### Menu keys

The menu's frame is titled with the entry it acts on. Each item shows the key of its action on the
right, and that key runs it while the menu is open. **Delete…** and **Discard changes…** are in red.
`up`/`down` (and their bound keys) move, skipping separators. `enter`/`right`/`l` run the item. `escape`
always closes, as do the keys bound to close, collapse, menu and settings. A long menu scrolls, with
`▴` `▾` on its border. Click an item to run it. Click elsewhere to close the menu.

### Other behaviours

- **Width**: the explorer is 32 columns wide by default. Resizing it by hand (dragging its own
  border, `<` `>`, or `shift+left` `shift+right`) remembers the new width for the next explorers.
  The preview keeps its width the same way, for as long as it is open. Resizing one of them, or the
  window, leaves the other's width alone: the terminal takes the difference. Closing the explorer
  (`q`, `esc` or `prefix+f`) closes the tab's preview too, so that the tab stays `explorer | [terminal |
  preview]`: dragging the border between the terminal and the preview leaves the explorer still. An
  explorer opened in a tab that still has a preview puts it back beside the terminal. On KDE Plasma, holding
  `<` or `>` shows the accented characters popup instead of repeating the key, unless **System
  Settings → Keyboard → When a key is held** is set to **Repeat the key**; the `shift` arrows
  repeat either way.
- **Alone in a tab**: when the terminal exits and only the explorer is left, it closes, and the tab
  with it.
- **Follow the terminal folder**: when on, the explorer checks every 2 s the folder of the terminal
  next to it (that of its running program when herdr knows it) and reveals it. If the folder is
  outside the tree root but inside the tab's folder, the root goes back to the tab's folder. A folder
  outside the tab's folder is not followed.

## Preview

The preview shows one file per tab. Opening another file from the explorer or the search reuses it.
It reloads by itself when the file changes on disk, keeping its scroll position.

Header: the file path relative to the root, then details (lines, size, image format and size). On the
right, clickable switches: **Rendered · Raw** for Markdown, the active one underlined, and **wrap** and
**lines** (line numbers) for text, `●` on and `○` off. A narrow pane drops the switches from the right,
and then the size. The footer
lists the main keys and the scroll percentage; a click on a key does what it does.

### Keys

| Key                          | Action                                                   |
| ---------------------------- | -------------------------------------------------------- |
| `q` `escape` `ctrl+c`        | close (`escape` first clears a selection or the find)    |
| `up` `k` / `down` `j` `enter`| scroll one line                                          |
| `pageup` / `pagedown` `space` | scroll one page                                         |
| `ctrl+u` / `ctrl+d`          | scroll half a page                                       |
| `?` `f1`                     | all the keys                                             |
| `home` `g` / `end` `G`       | top / bottom                                             |
| `left` `h` / `right` `l`     | scroll 8 columns sideways (when not wrapping)            |
| `/`                          | find in file                                             |
| `n` / `N`                    | next / previous match                                    |
| `w`                          | toggle line wrap                                         |
| `L`                          | toggle line numbers                                      |
| `m`                          | Markdown: rendered / source                              |
| `r`                          | reload                                                   |
| `e`                          | edit in the terminal editor                              |
| `o`                          | open (system application for documents and media, else the editor) |
| `O`                          | open in app (allowed types only)                         |
| `y` / `Y`                    | copy the absolute / relative path                        |
| `z`                          | zoom the pane (toggle)                                   |
| `<` / `>`                    | 4 columns narrower / wider (hold to repeat)              |
| `shift+left` `shift+right`   | move the inner border 4 columns that way                 |
| `ctrl+p`                     | Go to file                                               |
| `ctrl+f`                     | Search in files                                          |

### Find

`/` opens a field in the footer. Matching is case-insensitive unless the query contains a capital
letter. Typing jumps to the first match from the current position. `enter` or `down` goes to the next
match (`enter` also leaves the field when there are matches), `up` to the previous one, and `escape`
closes the find. The footer shows `current/total`. Every match is highlighted, the current one brighter,
also in wrapped lines and in rendered Markdown.

### Mouse

- **Drag** with the left button to select text. The selection is copied to the clipboard when you
  release the button (*Copied N lines* / *Copied N characters*).
- **Double-click** selects a word (or a run of spaces, or one punctuation character). **Triple-click** selects the
  whole line, all its wrapped rows included.
- The selection stays shown until the next click or `escape`; the wheel scrolls without clearing it. One more cell
  is highlighted when it takes a line break.
- Dragging onto the top or bottom row scrolls; without wrap, dragging onto the left or right edge scrolls sideways.
  Over the header, the selection reaches the start of the top row; over the footer, the end of the bottom row.
- A wrapped line is copied without the breaks of the wrap; rendered Markdown without its left margin and trailing
  spaces.
- When the button is released outside the pane, the selection is copied at the next click or when the pane loses
  focus. Selection works everywhere but in images.
- **Wheel** scrolls 3 lines.
- Click the header switches to change the rendering, wrap or line numbers.

### What it shows

- **Text**: syntax highlighting for JS/TS, JSON, Python, shell, PowerShell, Rust, Go, C/C++, C#, Java,
  Kotlin, Swift, PHP, Ruby, Lua, SQL, HTML/XML, CSS, YAML, INI/TOML, Markdown, diff, Dockerfile and
  Makefile.
- **Markdown** (`md markdown mdx mkd`): headings, inline styles, lists, quotes, highlighted code
  blocks, GFM tables and task lists, alerts, and simple Mermaid flowcharts.
- **Images** (`png apng jpg jpeg jpe jfif gif bmp dib ico cur webp svg`): PNG, GIF (first frame), BMP,
  baseline JPEG and ICO/CUR are drawn with half blocks. For other formats, such as WebP, SVG or
  progressive JPEG, the preview shows the format and size only.
- **Binary** (a NUL byte in the first 8000 bytes): a hex dump.

## Search popup

The popup has two tabs, **Go to file** (`ctrl+p`) and **Search in files** (`ctrl+f`), the active one
highlighted. Queries, globs
and options are saved for the next time and shown selected, so typing replaces them.

### Go to file

Fuzzy matching on paths, scored like fzf and VS Code. Each result shows the file name, its folder on
the right, and the matched characters highlighted.
`path:line` (or `path:line:column`) opens the file at that line. The file list is git's tracked and
untracked files that are not ignored. Outside a repository, when git lists no files, or with **hidden**
on, the plugin walks the folder instead, without `.git`, and without `node_modules` and dot folders
unless **hidden** is on. Search in files uses the same file list.

### Search in files

Text search across the files, results grouped by file, with each file's count on the right. Chips
toggle **Aa** (match case), **ab** (whole word) and **.\*** (regex), blue when on. The **in** (include)
and **not in** (exclude) fields take comma-separated globs:
`*` and `?` within a segment, `**` across segments, `{a,b}`, `[abc]`. A pattern without a leading
`./` or `/` matches at any depth, and a folder matches its contents.

### Keys

| Key                         | Action                                                       |
| --------------------------- | ------------------------------------------------------------ |
| `escape` `ctrl+c`           | close                                                        |
| `f1`                        | all the keys (`?` is typed in the fields)                    |
| `ctrl+p` / `ctrl+f`         | switch tab                                                   |
| `alt+c` `alt+w` `alt+r`     | toggle match case, whole word, regex                         |
| `alt+h`                     | toggle **hidden**: include ignored and dot files             |
| `tab` / `shift+tab`         | Search in files: next / previous field (query, include, exclude) |
| `up` `ctrl+k` / `down` `ctrl+n` | move                                                     |
| `pageup` / `pagedown`       | move one page                                                |
| `ctrl+home` / `ctrl+end`    | first / last result                                          |
| `alt+left` / `alt+right`    | fold / unfold the file's results                             |
| `enter`                     | open in the preview at the line, and close the popup         |
| `alt+enter` `ctrl+o`        | open in the editor                                           |
| `ctrl+t`                    | insert the path into the terminal                            |
| `ctrl+y`                    | copy the relative path                                       |

Other keys edit the focused field, with the same editing keys as the explorer's prompts. `ctrl+o` is there
because Windows Terminal keeps `alt+enter` for full screen.

Opening a result in the preview or the editor, or inserting its path, closes the popup; on failure, the
popup stays and shows the message. Go to file counts the files on the right of the field (`12 of 840`).
In Search in files, the rule above the results counts them (`37 results in 9 files`). The rule also shows
`indexing…`, `searching…` and the messages.

### Mouse

Click a tab, a chip or a field. Click a result to select it, and double-click it to open it. In
Search in files, clicking a file's header row folds or unfolds it. The wheel scrolls 3 rows. From 100
columns wide, the right half previews the selected result.

## Source control

A full-tab pane for the repository of the explorer's root (with `prefix+d` and no explorer: the focused terminal's folder). Outside a repository, it shows *Not a git
repository*, and `i` runs `git init` there after confirmation.

Opened with `s` in the tree, it selects the selected file in Changes and shows its diff (on a folder, its first
changed file); a file without changes leaves the selection at the top, with *No changes in …* in the status line.

### Header and tabs

The header shows the repository name, then the branch (click it to open the branch picker), then its
state against the upstream (*up to date*, `↑ahead ↓behind`, *not published*), then the ongoing operation
(MERGING, REBASING, CHERRY-PICKING, REVERTING, APPLYING, BISECTING). On the right, one main button
shows what the branch needs: **↑ Publish branch** without an upstream, **↓ Pull 3** when it is behind,
**↑ Push 2** when it is ahead, **⇅ ↓1 ↑2** when it has diverged (pulling then says to merge or rebase in
the terminal), and nothing when it is up to date. Its icon spins while it runs. Then come **⇣ Fetch**,
**↻** (refresh) and **✕** (close). Below 80 columns, the labels are shorter.

Tabs: **Changes**, **History**, **Stashes**, **Refs** (keys `1` to `4`), the active one underlined. In
Changes, a **≡ List / ⊞ Tree** button switches the layout.

From 100 columns, the diff of the selected file, commit, stash or tag always shows on the right of the
list; with a section or a branch selected, that side says what to select. Drag the separator between
them, or press `<` and `>`, to give the list fewer or more columns; each view keeps its own share, also for the next panes.

### Commit box (Changes)

The commit box has the message field on its own line, then the buttons. The first says what it will
do: **✓ Commit 2 staged**, **✓ Commit all 6** with nothing staged, **✓ Amend**, **✓ Continue rebase**
or **✓ Commit merge**. Then come **✧ Claude** (write the message), **☐ Amend** and, during an operation,
**✕ Abort**. In a narrow list, the labels are shorter.

- With nothing staged, committing offers to stage everything first.
- Amend starts from the last commit's message. An empty message keeps it.
- During a merge, an empty message uses the one git prepared.
- Continuing is refused when it would run `exec` lines or a merge strategy other than git's own (see
  [Security](../SECURITY.md#continue)). A `git am` is continued or aborted in the terminal.
- During a rebase, **Continue** runs `git rebase --continue`. During a cherry-pick or revert, it
  continues when the message is empty and commits with your message otherwise.
- **✧** sends the file list and the diff (staged, or else all changes) to the `claude` CLI and fills
  the field with a one-line Conventional Commits message. Without Claude, the message is built from
  the file names (`add foo.js`, `update a.js and 2 other files`).

### Keys

The status line shows the keys of the moment, as many as fit, and `?` opens the full-pane key help of
the view (see [Key help](#key-help--or-f1)). A click on a key of the status line does what the key does.

| Key                        | Action                                                          |
| -------------------------- | --------------------------------------------------------------- |
| `q` `escape` `ctrl+c`      | close (below 100 columns, `escape` closes the diff first)       |
| `?` `f1`                   | all the keys                                                    |
| `<` / `>`                  | narrower / wider list beside the diff (kept)                    |
| `shift+left` `shift+right` | move the separator 4 columns that way (kept)                    |
| `up` `k` / `down` `j`      | move (the diff on the right follows)                            |
| `pageup` / `pagedown`, `home` `g` / `end` `G` | move by page / to the ends               |
| `ctrl+u` / `ctrl+d`        | move half a page                                                |
| `left` `h` / `right` `l`   | fold / unfold a section or folder                               |
| `enter`                    | file, commit, stash, tag: focus its diff (below 100 columns: show it) · section/folder: fold · branch: checkout |
| `tab`                      | focus the diff                                                  |
| `1`–`4`                    | switch view                                                     |
| `r`                        | refresh                                                         |
| `f` / `L` / `P`            | fetch / pull (fast-forward only) / push. The first push publishes the branch on `origin` or the only remote and sets its upstream |
| `ctrl+p` / `ctrl+f`        | Go to file / Search in files, in the repository                 |
| `b`                        | branch picker                                                   |
| `c`                        | type the commit message                                         |
| `ctrl+g`                   | write the message with Claude                                   |
| `alt+a`                    | toggle amend                                                    |

**Changes view**: `s`/`space` stage, `u` unstage, `a` stage all, `A` unstage all, `d` discard (after
confirmation), `t` list/tree (kept for the next panes), `X` abort the ongoing operation. These keys
act on the selected file, folder or section; the files staged or unstaged are highlighted for a moment
where they land. Unlike the explorer's *Discard changes*, `d` in *Changes* **deletes untracked files
for good**, not to the trash, with the folders left empty; in *Staged Changes*, the index and the file go
back to the last commit, and files added since are deleted. Conflicts cannot be discarded. On a file, as in
the explorer and the preview, `y` / `Y` copy its absolute / relative path, `e` edits it in the terminal
editor and `o` opens it in the editor.

**Stashes view**: `n` new stash, named on a row above the list (the message is optional): `enter`
stashes the untracked files too, `alt+enter` leaves them. `a` apply, `o` pop, `d` drop.

**History view**: the graph of the branches, then the subject, the branches and remote branches pointing
at the commit (in the colour of their lane) and its tags (yellow); the author (from 90 columns), the age and
the short hash on the right. Each branch has its own lane and colour, linked to the commit it started from
and to the merge that brought it back. The commits of a lane are marked ┿ on its line, which joins them up
(┯ for a branch tip, ┷ for the first commit); the commit checked out has its subject in bold. It shows every local and remote branch,
or with `a` only the current one (kept for the next panes). The graph takes 24 columns at most: lanes
further right are cut.

**Refs view**: local branches, remote branches, tags, remotes and worktrees; the empty ones are named
together on the last line. `enter` checks out a branch (a remote branch switches to its local branch, or
creates one tracking it). `D` deletes a local branch, and offers `-D` when it is not fully merged,
saying how many commits would be lost (those on no other branch).

**Comparing branches**: from 100 columns, the selected branch is compared on the right with the current
branch: how many commits it is ahead and behind, the commits only on it, those only on the current branch
(`=` before a commit whose change is also on the other side, as after a cherry-pick), and the files it changed
since it left the current branch. `=` opens the comparison at any width and focuses it; there, `up`/`down`
select a commit, `enter` shows it and `q` goes back to the comparison. `B` makes the selected branch the base
of the comparisons instead of the current branch (`B` again on it, or on the current branch, goes back).

**While typing the message**: `enter` commits or continues, `escape`/`tab` leaves the field, `ctrl+g`
writes with Claude, and `alt+a` toggles amend.

**Diff focused**: `up` `k` / `down` `j` `enter`, `pageup` / `pagedown` `space`, `home` `g` / `end` `G`,
`ctrl+u` / `ctrl+d` scroll. `n`/`N` jump between hunks, `s` switches inline/side by side (kept for the
next diffs), and `tab`, `q` or `escape` go back to the list (below 100 columns, `q` and `escape` close
the diff).

**Branch picker** (`b`): starts with *New branch…*, then local and remote branches. `up`/`down`
(`k`/`j`), `pageup`/`pagedown`, `enter` switch, `n` new branch, `D` delete, `/` or typing filters,
`escape` clears the filter then closes, `q` closes. A new branch is named on the picker's first row,
from the filter or else the current branch's prefix (`feat/`): `enter` creates it and switches to it,
`alt+enter` only creates it, `escape` cancels.

**Questions**: discarding, deleting a branch, aborting an operation, popping or dropping a stash, staging
everything to commit and `git init` ask in a card over the pane, as in the explorer: `left`/`right` move
between its buttons, `enter` presses the focused one (Cancel at first), `y` confirms, any other key cancels.

### Mouse

On the selected row, buttons appear: **+** stage, **−** unstage, **↶** discard. Click the header and
commit-box buttons, the tabs and the branch. Click a section or folder to fold it, double-click a
file or commit to show it, and click the diff to focus it. The wheel scrolls the list, the diff or
the picker under the mouse. From 100 columns, the diff shows on the right of the list, otherwise it
takes the whole pane.

The pane refreshes when the git folder changes and every 5 s, which picks up working-tree edits.

## Changes (diff pane)

Opened by **View changes**. For a file or folder, it compares tracked files with HEAD (index and
working tree; before the first commit, the staged then the unstaged changes) and shows untracked files in
full, up to 200. The header shows the name, the added and removed line counts, and the layout switch
**Inline · Side by side**, the active one underlined; a click on a side chooses it. The footer shows the
keys and how far down the diff is. The diff is inline, or side by side from 120 columns
wide, with changed words highlighted.

| Key                               | Action                    |
| --------------------------------- | ------------------------- |
| `q` `escape` `ctrl+c`             | close                     |
| `up` `k` / `down` `j` `enter`     | scroll                    |
| `pageup` / `pagedown` `space`     | scroll one page           |
| `ctrl+u` / `ctrl+d`               | scroll half a page        |
| `home` `g` / `end` `G`            | top / bottom              |
| `ctrl+p` / `ctrl+f`               | Go to file / Search in files |
| `?` `f1`                          | all the keys              |
| `n` / `N`                         | next / previous hunk      |
| `s`                               | inline / side by side (kept for the next diffs) |

The wheel scrolls 3 rows.

## Editor pane

The Editor pane runs a terminal editor on the file and closes when the editor exits. The editor is
the settings' editor command when it is a terminal editor, else `$VISUAL`, else `$EDITOR`, else the
first of `nano`, `nvim`, `vim`, `vi` found (`edit`, `nvim`, `vim`, `nano` on Windows).

## Scrollbars

Every list (explorer, preview, search results, source control list and diff, changes pane) shows a
scrollbar in its last column when it does not fit: a rail, a thumb, and `▴` `▾` buttons when the
list is at least 4 rows tall.

An empty list says why in grey, centred, with the keys that go on from there under it: *Empty folder*
(`n` new file, `N` folder), *No match* (`esc` clear), *No results* in the search, *No changes* in source
control (`ctrl+p` go to file, `2` history), *Not a git repository* (`i` git init here).

- `│` is the rail and `┃` the thumb, sized to the part shown.
- A press on the thumb grabs it: the list follows the mouse until the button is released, even outside the column.
- A press elsewhere on the rail moves the thumb under the mouse, then drags it the same way.
- A click on `▴` or `▾` scrolls one row; held, it repeats (after 400 ms, then every 50 ms) until the end of the
  list, or until the mouse leaves the button.
- The wheel still scrolls the list. The explorer's scrollbar ignores clicks while a menu is open, the preview's
  while a selection is being dragged, and source control's while the branch picker is open.
