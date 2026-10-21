# Usage

The plugin has six panes:

| Pane            | Opens as                     | Opened by                                                     |
| --------------- | ---------------------------- | ------------------------------------------------------------- |
| Explorer        | split, left or right of tab  | new tab, `prefix+f`                                           |
| Preview         | split, right of the terminal | `Enter`/double-click on a file, `p`, `d` (its diff), search results |
| Search          | popup, 80% × 80%             | `ctrl+p`, `ctrl+f`, `prefix+a`                                |
| Source Control  | full tab (zoomed)            | `s`, `prefix+d`                                               |
| Changes         | full tab (zoomed)            | **View changes** on a folder                                  |
| Editor          | full tab (zoomed)            | `e`, **Edit in terminal**, `o` with a terminal editor         |

Key names below are those of [configuration.md](configuration.md#key-names). Only the explorer's
keys can be rebound.

## Explorer

### Header

The header shows the root folder's name in capitals and bold, in bold italic when **Set as root**
moved the tree away from the tab's folder. After an action, a short message replaces it: `✓` done (2 s), `·`
nothing to do (2 s), `✕` in red failed (4 s), `⋯` running. The preview and source control show their messages the
same way, in their bottom line. The icons on the right are clickable, and hidden while a message shows:

| Icon      | Click                                                                  |
| --------- | ---------------------------------------------------------------------- |
| `↑`       | update to the newer release (green, when one exists and **Check for updates** is on) |
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

The last column is the action's name in [`keys.json`](configuration.md#config-folder-keysjson).

| Key                      | Action                                                                         | Name |
| ------------------------ | ------------------------------------------------------------------------------ | ---- |
| `up` `k` / `down` `j`    | move                                                                           | `up` / `down` |
| `pageup` / `pagedown`    | move one page                                                                  | `pageUp` / `pageDown` |
| `ctrl+u` / `ctrl+d`      | move half a page                                                               | `halfPageUp` / `halfPageDown` |
| `home` `g` / `end` `G`   | first / last row                                                               | `top` / `bottom` |
| `?` `f1`                 | the key help page, see below                                                   | `help` |
| `enter` `space`          | folder: fold or unfold. File: open (see [Opening files](#opening-files))       | `open` |
| `right` `l`              | unfold a folder, or go to its first child when already open                    | `expand` |
| `left` `h`               | fold a folder, or go to the parent folder                                      | `collapse` |
| `m` `menu` `shift+f10`   | action menu of the selected row                                                | `menu` |
| `escape` `q`             | close the menu, the filter or the page shown, else the explorer and the tab's preview | `close` |
| `r`                      | refresh                                                                        | `refresh` |
| `<` / `>`                | 4 columns wider / narrower (hold to repeat), kept for the next explorers       | `wider` / `narrower` |
| `shift+right` / `shift+left` | the same, docked on the left (swapped when docked on the right): each pushes the inner border its way | `wider` / `narrower` |
| `/`                      | filter the tree                                                                | `filter` |
| `ctrl+p`                 | Go to file                                                                     | `goToFile` |
| `ctrl+f`                 | Search in files (in the selected folder, else the root)                        | `searchInFiles` |
| `s`                      | Source control, on the selected file (on a folder: its first changed file)     | `sourceControl` |
| `,`                      | Settings                                                                       | `settings` |
| `.`                      | show / hide dot files                                                          | `toggleHidden` |
| `i`                      | show / hide git-ignored files                                                  | `toggleIgnored` |
| `n` / `N`                | new file / new folder, in the selected folder (or the selected file's folder)  | `newFile` / `newFolder` |
| `R` `f2`                 | rename                                                                         | `rename` |
| `delete`                 | move to the trash                                                              | `delete` |
| `e`                      | edit the file in the terminal editor                                           | `edit` |
| `o`                      | open in the external editor                                                    | `openInEditor` |
| `O`                      | open the file with its system application (documents, media, text formats)     | `openInApp` |
| `p`                      | preview the file and focus the preview                                         | `preview` |
| `y` / `Y`                | copy the absolute / relative path                                              | `copyPath` / `copyRelativePath` |
| `backspace`              | make the parent folder the tree root                                           | `parentRoot` |
| `c`                      | changed files only (see [Reviewing changes](#reviewing-changes))               | `changedOnly` |
| `d`                      | changes of the file in the preview ([Diff view](#diff-view)); of a folder in the Changes pane | `viewChanges` |
| `]` / `[`                | next / previous changed file, previewed                                        | `nextChange` / `previousChange` |
| `H`                      | history of the selected file, in source control                               | `fileHistory` |

### Mouse

- **Click** a folder to fold or unfold it, a file to select it (and preview it, with **Preview follows selection**
  on). **Double-click** a file (within 400 ms) to open it.
- **Right-click** a row for its action menu, below the last row for the root menu: in this pane, right-clicks go
  to the explorer instead of herdr's menu.
- **Wheel** scrolls 3 rows (in a menu, it moves the selection). See also [Header](#header) and
  [Scrollbars](#scrollbars).

### Filter

`/` opens a filter field at the bottom. The tree keeps the entries whose name contains the text (any case), with
their parent folders dimmed, the match in bold blue, and a count of matches. Git-ignored folders are not searched,
and folders cannot be folded while filtering.

- Typed characters go to the field, as do `backspace`, `delete`, `ctrl+u` and `ctrl+d` (see
  [Prompts](#prompts)). The other bound keys that are not characters (arrows, `enter`, `ctrl+p`, `ctrl+f`…) act
  on the list.
- `tab` leaves the field and keeps the filter, so the single-letter keys work again; the bottom line then shows the
  filter in blue and `esc clear`. Click it to type again.
- `escape` closes the filter and reveals the selected entry in the full tree.

### Reviewing changes

`c` keeps in the tree only the changed files (the git badges `M`, `A`, `U`, `R`, `D`, `!`) and the folders holding
them, all unfolded; the bottom line shows `● Changes` and their count, and `escape` (or `c` again) brings the whole
tree back with the selected entry shown. The filter (`/`) then searches the changed files only.

`]` and `[` move to the next and previous changed file in the tree's order, unfolding its folders, and show it in the
preview; past the last one, they start again from the first. Among the changed files, the preview shows each one's
changes ([Diff view](#diff-view)), as **Preview follows selection** does there; in the whole tree, the file itself.

Files and folders deleted from the disk show only among the changed files, struck through with the `D` badge; `]`
and `[` reaching one switch to that view. `enter` or a double-click shows what it held in its diff, and its menu keeps
**View changes**, **File history**, **Stage**, **Unstage**, **Discard changes…** (which restores it) and **Copy
relative path**; the actions that need the file on disk tell how to restore it instead.

### Prompts

**Rename** turns the row into a field, the cursor before the extension; a change of case only is allowed.
**New file** and **New folder** open the folder and add a field at the top of it, whose icon follows the name
typed. A name may contain subfolders (`a/b.txt`, created as needed) but cannot leave the folder; a name ending
with `/` creates a folder. The editor command is typed on the bottom line, where an empty one restores the
default. The bottom line shows the keys (`⏎ rename`, `⏎ create in src`, `esc cancel`): `enter` accepts,
`escape` or any click cancels.

Every text field (prompts, filter, finds, search and source control fields) edits with these keys, unless the
pane uses the key itself, and takes pasted text.

| Key                               | Action                              |
| --------------------------------- | ----------------------------------- |
| `left` `right`, `ctrl+b` `ctrl+f` | move the cursor                     |
| `home` `end`, `ctrl+a` `ctrl+e`   | start / end                         |
| `backspace`, `delete` `ctrl+d`    | delete before / after the cursor    |
| `ctrl+u` / `ctrl+k`               | delete to the start / to the end    |
| `ctrl+w` `alt+backspace`          | delete the word before the cursor   |

### Confirmations

Moving to the trash, deleting for good, discarding changes and updating ask first, in a card over the
pane: the question, the full name and its folder, a note when the action cannot be undone, then two
buttons, Cancel selected at first. `left`/`right` (also `h`/`l`, `tab`/`shift+tab`) move between them, `enter`
presses the selected one, and `y` or a click on the action's button confirms. Any other key or click cancels.
Source control asks the same way.

### Opening files

`enter`, a double-click or the menu's first item decides as follows:

1. With **Enter opens** set to `preview` (the default), a file the preview can show goes to the preview:
   every file except documents and media other than images (Office files, PDF, archives, audio and video).
2. Otherwise, documents and media (`png jpg jpeg gif webp bmp ico svg pdf doc docx xls xlsx ppt pptx
   odt ods odp zip 7z rar gz tgz mp3 mp4 wav mkv avi mov`) open with the system application.
3. Everything else goes to the editor. A double-click never runs an executable or a script.

### Action menu (on a file or folder)

| Item                     | Shown for                         | Action                                                       |
| ------------------------ | --------------------------------- | ------------------------------------------------------------ |
| Preview                  | files the preview can show        | show in the tab's preview                                    |
| Open in editor           | files and folders                 | external editor (see [configuration](configuration.md#editor-command)) |
| Open in app              | `txt md csv log json xml html htm` and the media types above | system application |
| Edit in terminal         | files                             | Editor pane                                                  |
| Insert into terminal     | all                               | type the path into the terminal next to the explorer         |
| New file… / New folder…  | all                               | in the folder (or the file's folder)                         |
| Search in folder…        | folders                           | Search in files limited to that folder                       |
| Set as root              | folders                           | make the folder the tree root                                |
| Rename…                  | all                               |                                                              |
| Duplicate                | all                               | copy as `name copy.ext`, `name copy 2.ext`…                  |
| Delete…                  | all                               | move to the trash, after confirmation                        |
| Open folder (Reveal in Explorer on Windows) | all            | Windows and macOS select the entry in File Explorer or Finder; on Linux, the file manager opens the folder holding it |
| Copy path                | all                               | absolute path                                                |
| Copy relative path       | all                               | relative to the tree root                                    |
| View changes             | changed entries in a repository   | a file: its Diff view in the preview; a folder: the Changes pane |
| File history             | files in a repository, not untracked | source control's History, limited to the file             |
| Stage                    | changed entries                   | `git add -A` on the path                                     |
| Unstage                  | entries in a repository, not ignored | restore the index from HEAD                               |
| Discard changes…         | changed entries                   | after confirmation, see below                                |

**Insert into terminal** finds the terminal next to the explorer, skipping the plugin's own panes, types the path
relative to the terminal's folder followed by a space, then focuses the terminal. When an agent runs there, the
path is prefixed with `@`, unless it contains spaces: it is then quoted instead. In a shell, it is quoted when
needed.

**Delete…** moves the entry to the trash (see [Platform tools](installation.md#platform-tools)). Without one, a
second confirmation offers to delete it for good.

**Discard changes…** puts tracked files back to their last commit, staged changes included. Files
added since the last commit, and untracked files, go to the trash instead of being deleted.

### Root menu (right-click below the rows)

New file…, New folder…, Go to file…, Search in files…, Source control (in a repository), Open folder
(Reveal in Explorer), Copy path, Up one level, Back to the space folder (after **Set as root**),
Refresh, Settings….

### Settings (`,` or the gear icon)

The settings take the place of the tree, by group, each with its value on the right: `● on` or
`○ off` for a switch, the value chosen otherwise. A line under the list explains the selected
setting. The settings are shared by every explorer. See [configuration.md](configuration.md#settings).

| Group     | Settings                                                      |
| --------- | ------------------------------------------------------------- |
| Tree      | Hidden files, Git-ignored files, Follow terminal, Icon set, Icon theme |
| Opening   | Enter opens (preview or editor), Editor, Preview follows selection |
| Next tabs | Dock side (left or right), Open automatically                 |
| Preview   | Wrap lines, Line numbers, Images, Diff layout (auto, split, inline) |
| General   | Key bindings, Check for updates                               |

`up`/`down` (and their bound keys, page and first/last included) move, `/` finds a setting, `right`/`l` and
`enter` give a setting its next value (on **Icon theme**, **Editor** and **Key bindings**: open its page or
prompt), `left`/`h` its previous one. A newer release shows a line under the title: `u` updates (or finishes the
update). `escape`, and the keys bound to close and settings, go back to the tree. The wheel moves the selection, a
click on a setting changes it, a click on the title goes back.

**Icon theme** opens a page of its own: the Nerd Font default and the VS Code icon themes installed (see
[Icon themes](configuration.md#icon-themes)), the saved one marked `✓`, above a sample tree of folders and files
in the theme under the selection. `enter` (or a click) keeps it and goes back to the settings; `escape` goes back
without a change. **Icon set** (Nerd Font, emoji, Unicode, ASCII) is for a terminal without a Nerd Font (see
[Without a Nerd Font](configuration.md#without-a-nerd-font)).

**Key bindings** opens a page of its own: the explorer's actions by group, as in the key help, with
their keys (in yellow when changed). Below the list, the selected action shows its keys and its
default ones.

| Key                  | Action                                                                      |
| -------------------- | --------------------------------------------------------------------------- |
| `enter`              | wait for a key and add it to the action (taken from the action that had it) |
| `delete` `backspace` | remove the action's last key                                                |
| `r`                  | give the action back its default keys                                       |
| `/`                  | find an action                                                              |
| `escape` `left`      | back to the settings, as do the keys bound to close, collapse and settings (while waiting for a key: cancel) |

The arrows always move in this page, whatever the bindings. A click selects an action, a second click
waits for its key. The last row, **Open keys.json**, opens the file in the Editor pane. The changes are
saved in [`keys.json`](configuration.md#config-folder-keysjson) and apply at once.

### Key help (`?` or `f1`)

Every key of the explorer, as bound now, takes the place of the tree, by group, over the pane's full height. The arrows, page keys, `j` `k` `g` `G`,
`ctrl+u` `ctrl+d` and the wheel scroll it, `/` finds a key or what it does. `escape`, `q`, `?`,
`f1`, or a click on the title, goes back to the tree. The preview, the search popup, source
control and the changes pane show their keys the same way (`?` or `f1`; `f1` only in the search).

### Find in the pages

In the settings, the key bindings and the key help, `/` opens a find field under the title. The list keeps the
rows whose name, value, keys or help has the text typed, and the field counts them. Typed characters go to the
field; the arrows and `enter` still act on the list. `escape` closes the find, the selection staying on its row.

### Menu keys

The menu's frame is titled with the entry it acts on. Each item shows the key of its action on the right, which
runs it while the menu is open. **Delete…** and **Discard changes…** are in red. `up`/`down` (and their bound keys)
move, skipping separators, and `enter`/`space`/`right`/`l` run the item. `escape` always closes, as do the keys
bound to close, collapse and menu; the key of settings opens **Settings…**. A long menu scrolls, with `▴` `▾` on
its border. A click runs an item; a click elsewhere closes the menu.

### Other behaviours

- **Width**: the explorer is 32 columns wide by default. Resizing it by hand (its border, `<` `>`, or the `shift`
  arrows) keeps the new width for the next explorers. The preview keeps its width the same way while it is open.
  Resizing one of them, or the window, leaves the other's width alone: the terminal takes the difference. Closing
  the explorer (`q`, `esc` or `prefix+f`) closes the tab's preview too, so that the tab stays
  `explorer | [terminal | preview]` and dragging the border between the terminal and the preview leaves the
  explorer still; an explorer opened in a tab that still has a preview puts it back beside the terminal. On KDE
  Plasma, holding `<` or `>` shows the accented characters popup instead of repeating the key, unless **System
  Settings → Keyboard → When a key is held** is set to **Repeat the key**; the `shift` arrows repeat either way.
- **Alone in a tab**: when the terminal exits and only the explorer is left, it closes, and the tab
  with it.
- **Refresh**: the tree follows file changes, with a full rescan every 30 s. Expanded folders are remembered per
  tree root.
- **Follow the terminal folder**: when on, the explorer checks every 2 s the folder of the terminal next to it
  (that of its running program when herdr knows it) and reveals it. A folder outside the tree root but inside the
  tab's folder brings the root back to the tab's folder; a folder outside the tab's folder becomes the root, except
  `$HOME`, one of its parents or the filesystem root, which are not followed.

## Preview

The preview, one per herdr tab, keeps its files in tabs, as VS Code's editors. A file opened by a single click,
by following the selection, with `]`/`[`, from the search or by an agent (`peek.js`) replaces the **temporary** tab,
its name in italics; `enter`, a double-click, the explorer's `p` or its menu's **Preview** open a **kept** tab, which
the next file does not replace. In the preview, `t`, a double-click on the tab, `e`, `o` or a review note keep the
temporary tab. Each tab keeps its scroll, line, File or Diff view, comparison, Markdown mode and marks; the tabs come
back after a herdr restart (those whose file still exists). It reloads by itself when the file changes on disk,
keeping its scroll position.

Header: with several tabs, the tabs (the current one bold on a lighter background, `×` to close each), then the
details when they fit; with one, the file path relative to the root, then details (lines, size, image format and size; in the Diff view, the
added and removed lines). On the right:

- in the Diff view, what the changes are compared with, dimmed: `vs HEAD` or `vs main` (the rest shows in the content
  itself);
- the **File · Diff** switch, the active choice filled: a click on the other one switches ([Diff view](#diff-view));
- `⚙` (or `,`, as the explorer's settings), which opens the **View** menu of the view's options, as the explorer's settings show them: a choice
  in blue, or `● on` / `○ off`. **Markdown** (rendered or raw, `m`), **Wrap lines** (`w`) and **Line numbers** (`L`)
  for text, **Image** (sharp or pixelated, `p`); in the Diff view, **Layout** (inline or split, `s`) and **Compare
  with** (HEAD or the default branch, `b`). The key beside each does the same outside the menu. `left` / `right`,
  `enter`, `space` or a click changes the option in place, the menu staying open; `up` / `down` move, `escape`, `q`,
  `,` or a click elsewhere close it. The footer's actions are not repeated there, and the footer leaves out what the
  header's controls do (`d`, `,`): the key help (`?`) lists every key.

A narrow pane drops the reminder, then the switch, then the details; `⚙` stays. Tabs that do not fit are marked
`‹` / `›` (a click shows the next one that way), and the strip scrolls only when the current tab would leave it. The footer lists the main keys, which
a click runs, and the scroll percentage.

### Keys

| Key                          | Action                                                   |
| ---------------------------- | -------------------------------------------------------- |
| `q` `escape` `ctrl+c`        | close (first close the find; `escape` first clears a selection) |
| `up` `k` / `down` `j` `enter`| scroll one line                                          |
| `pageup` / `pagedown` `space` | scroll one page                                         |
| `ctrl+u` / `ctrl+d`          | scroll half a page                                       |
| `?` `f1`                     | all the keys                                             |
| `tab` / `shift+tab`, `ctrl+pagedown` / `ctrl+pageup` | next / previous tab              |
| `t`                          | keep the temporary tab                                   |
| `x`                          | close the tab (the last one: the preview)                |
| `home` `g` / `end` `G`       | top / bottom                                             |
| `left` `h` / `right` `l`     | scroll 8 columns sideways (when not wrapping)            |
| `/`                          | find in file                                             |
| `n` / `N`                    | next / previous match; without a find, next / previous mark of [`peek.js`](#show-a-file-from-an-agent-or-a-script), else hunk in the Diff view |
| `d`                          | the file or its changes ([Diff view](#diff-view))        |
| `b` / `s`                    | Diff view: compare with HEAD or where the branch started / inline or split (saved by width) |
| `,`                          | the View menu of the view's options (or `⚙`)            |
| `w`                          | toggle line wrap (saved)                                 |
| `L`                          | toggle line numbers (saved)                              |
| `m`                          | Markdown: rendered / source                              |
| `p`                          | image: sharp / pixelated, in this pane only (see [Sharp images](#sharp-images)) |
| `r`                          | reload                                                   |
| `i`                          | edit here, in the preview (see [Editing in the preview](#editing-in-the-preview)) |
| `e`                          | edit in the terminal editor                              |
| `o`                          | open (system application for documents and media, else the editor) |
| `O`                          | open in app (allowed types only)                         |
| `y` / `Y`                    | copy the absolute / relative path; `Y` adds the selected lines (`src/app.js:12-14`) |
| `H`                          | history of the file, in source control                   |
| `a`                          | take a review note, see [Review notes](#review-notes)    |
| `A` / `C` / `u`              | send the notes to the agent / copy them / remove the last note |
| `z`                          | zoom the pane (toggle)                                   |
| `<` / `>`                    | 4 columns narrower / wider (hold to repeat)              |
| `shift+left` `shift+right`   | move the inner border 4 columns that way                 |
| `ctrl+p`                     | Go to file                                               |
| `ctrl+f`                     | Search in files                                          |

### Editing in the preview

`i` (or **Edit here** in the View menu) edits the tab's file in place: a text file in UTF-8, up to 2 MB (a binary
or otherwise encoded file is refused); Markdown edits as source. Lines do not wrap while editing, the terminal's
cursor shows as a bar, and the highlighting follows what you type.

| Key | Action |
|---|---|
| arrows, `home` / `end`, `pageup` / `pagedown`, `ctrl+left` / `ctrl+right` | move (by word with `ctrl`); with `shift`, select |
| `enter` | new line, keeping the indentation |
| `tab` / `shift+tab` | indent / unindent, with the file's own indentation |
| `ctrl+s` | save |
| `ctrl+z` / `ctrl+y` `ctrl+shift+z` | undo / redo (typed text grouped by word) |
| `ctrl+c` / `ctrl+x` / `ctrl+v` | copy / cut / paste, the whole line when nothing is selected; `ctrl+a` selects all |
| `ctrl+f` | find: `enter` or `down` next, `up` previous |
| `ctrl+r` | replace: `tab` switches field, `enter` replaces one, `alt+a` all (`ctrl+h` is backspace in terminals) |
| `ctrl+g` | go to a line |
| `escape` | clear the selection, then leave editing |
| `f1` | the editing keys |

A click places the cursor, a double or triple click selects a word or the line, a drag selects (scrolling at the
edges), the wheel scrolls; text pasted from the terminal goes in as typed.

- **Saving** keeps the file's line endings (LF or CRLF; mixed ones become the most frequent, which a message says),
  its last newline or its lack of one, and its mode; the file is written to a temporary file renamed over it.
- **Changes since opening** show in the gutter: a green `▎` for added lines, blue for modified ones, a red `▔`
  where lines were removed.
- **Unsaved changes**: leaving editing, closing the tab (its `×` shows `●`) or the preview asks Save, Don't save or
  Cancel. Other tabs keep working meanwhile. A pane closed by herdr itself loses them.
- **Changed on disk**: without edits of yours, the file reloads silently (which empties the undo history and the
  gutter); with edits, a bar offers `alt+r` to reload (losing them) or `alt+k` to keep yours, and saving over the
  newer file then asks first. The plugin waits for editing to end before restarting on an update.

### Review notes

Notes are remarks on lines of a file, gathered while reading, then typed together into the agent's prompt. They
belong to the tab: the preview and the [Changes pane](#changes-diff-pane) of a tab share them.

- `a` opens a field in the footer for a note on the selected lines (select them with the mouse), else on the line
  the preview was opened at (a search result), else on the whole file. `tab` (`shift+tab` backwards) gives it a
  kind, `fix`, `question`, `nit` or `verify`, shown before the text; `enter` saves it, `escape` cancels.
- Each note shows in yellow under the last line it covers, whose numbers turn yellow; notes on the whole file, and
  every note in rendered Markdown, show at the top.
- `A` types the tab's notes, sorted by file and line, into the terminal next to the explorer, on one line, without
  submitting it, then clears them: `Review notes: src/app.js:12-14 (fix): rename it; README.md: add an example`. The
  paths are relative to the terminal's folder. The footer shows `A send N notes` while there are some.
- `C` copies the tab's notes instead, one per line under `Review notes:`, the paths relative to the preview's folder,
  and keeps them.
- `u` removes the last note taken.

The notes live in the plugin's state folder until they are sent, so they survive closing the preview.

### Find

`/` opens a field in the footer. Matching ignores case unless the query has a capital letter, and typing jumps to
the first match from the current position. `enter` or `down` goes to the next match (`enter` also leaves the field
when there are matches), `up` to the previous one, and `escape` closes the find. The footer shows `current/total`.
Every match is highlighted, the current one brighter, also in wrapped lines and in rendered Markdown.

### Mouse

- **Drag** with the left button to select text, copied to the clipboard on release (*Copied N lines* /
  *Copied N characters*). **Double-click** selects a word (or a run of spaces, or one punctuation character),
  **triple-click** the whole line, all its wrapped rows included.
- The selection stays shown until the next click or `escape`; the wheel scrolls without clearing it. One more cell
  is highlighted when it takes a line break.
- Dragging onto the top or bottom row scrolls, onto the left or right edge (without wrap) scrolls sideways. Over
  the header, the selection reaches the start of the top row; over the footer, the end of the bottom row.
- A wrapped line is copied without the breaks of the wrap; rendered Markdown without its left margin and trailing
  spaces.
- Released outside the pane, the selection is copied at the next click or when the pane loses focus. Selection
  works everywhere but in images.
- Under a pixelated image, a click on **p sharp** shows it sharp, on **sharp by default** sets **Images** to
  *sharp if possible*, and on **Sharp images ↗** opens [Sharp images](#sharp-images) in the browser.
- The **wheel** scrolls 3 lines. In the header, **File · Diff** switches the view and `⚙` opens the View menu.
- On a tab: a click shows it, a double-click keeps it, a click on its `×` or a middle click closes it.

### Diff view

`d` (or **File · Diff** in the header) shows the file's git changes in place of the file, as the Changes pane does:
compared with HEAD, staged and unstaged together, an untracked file in full, a deleted one as removed lines. The
header shows the added and removed line counts and `vs HEAD` or `vs main`; the View menu (`⚙`, `,`) chooses the **Layout**, inline or split
(side by side, `s`), and what to **Compare with**, HEAD or the default branch (`b`). The layout is saved for every
diff, one for narrow diffs and one for wide ones: inline beside the terminal and split once zoomed (`z`), for
example. `n` and `N` jump
between hunks, `a` notes the lines of the rows selected with the mouse (or of the top row), and it reloads with the
file. Find (`/`) works in the file only.

The explorer opens it with `d` and **View changes** on a file, and among the changed files with `]`, `[` and the
selection; `peek.js --diff` too.

### What it shows

- **Text**: syntax highlighting for JS/TS, JSON, Python, shell, PowerShell, Rust, Go, C/C++, C#, Java,
  Kotlin, Swift, PHP, Ruby, Lua, SQL, HTML/XML, CSS, YAML, INI/TOML, Markdown, diff, Dockerfile and
  Makefile.
- **Markdown** (`md markdown mdx mkd`): headings, inline styles, lists, quotes, highlighted code
  blocks, GFM tables and task lists, alerts, and simple Mermaid flowcharts.
- **Markdown images**: a local image of a paragraph (Markdown image syntax, or `<img src>`), relative to the Markdown file,
  is drawn under it, 60 columns and 20 rows at most: in pixels or with half blocks, like an image file (see
  [Sharp images](#sharp-images)). Links to the web are never fetched.
- **Images** (`png apng jpg jpeg jpe jfif gif bmp dib ico cur webp tif tiff svg`): PNG, GIF (first frame), BMP,
  JPEG (baseline and progressive), TIFF and ICO/CUR are decoded by the plugin, WebP by `dwebp` when it is installed
  (libwebp, see [Requirements](installation.md#requirements)). When herdr can show images (the Kitty graphics
  protocol), the image is drawn in real pixels, fitted to the pane; otherwise with half blocks (see
  [Sharp images](#sharp-images)). For other formats, such as SVG or a lossless or
  arithmetic-coded JPEG, the preview shows the format and size only. TIFF: strips (not tiles), one plane,
  uncompressed, PackBits, LZW or Deflate.
- **Binary** (a NUL byte in the first 8000 bytes): a hex dump.

### Sharp images

Images are drawn either pixelated, with half blocks (two coloured squares per cell), which works in any terminal, or
sharp, in real pixels with the Kitty graphics protocol. By default, they are sharp when the terminal has the
protocol, which needs two things: herdr running in a terminal
that has it, such as [Kitty](https://sw.kovidgoyal.net/kitty/), [Ghostty](https://ghostty.org/),
[WezTerm](https://wezterm.org/) or Konsole, and herdr's `terminal.kitty_graphics` left on (its default). The
plugin cannot install a terminal: install one of them, then start herdr in it.

**Settings → Images** chooses for every preview:

| Value | Images |
|---|---|
| sharp if possible (default) | in pixels when the terminal answers the protocol's query, else half blocks |
| pixelated | half blocks |
| always sharp | in pixels without waiting for the answer, for a terminal that has the protocol but does not answer; nothing shows where it does not have it |

The setting tells whether this terminal answered. `p` switches the image shown between sharp and pixelated, in
this pane only, until the setting changes. Under a pixelated image, a line tells how to see it sharp: `p` and
**sharp by default** when the terminal has the protocol, else a link to this section. Images in rendered Markdown
follow the same setting, and `p` (or **Images** in the **View** menu) switches them all; scrolled partly out of the
pane, an image shows its visible part.

## Search popup

The popup has two tabs, **Go to file** (`ctrl+p`) and **Search in files** (`ctrl+f`). Queries, globs and the case,
word and regex options are saved for the next time, shown selected so that typing replaces them. **hidden** starts
off each time.

### Go to file

Fuzzy matching on paths, scored like fzf and VS Code. Each result shows the file name, its folder on the right,
and the matched characters highlighted. `path:line` (or `path:line:column`) opens the file at that line. The file
list is git's tracked and untracked files that are not ignored. Outside a repository, when git lists no files, or
with **hidden** on, the plugin walks the folder instead, without `.git`, and without `node_modules` and dot
folders unless **hidden** is on. Search in files uses the same file list.

### Search in files

Text search across the files, results grouped by file, with each file's count on the right. Chips
toggle **Aa** (match case), **ab** (whole word) and **.\*** (regex), blue when on. The **in** (include)
and **not in** (exclude) fields take comma-separated globs:
`*` and `?` within a segment, `**` across segments, `{a,b}`, `[abc]`, `[!abc]`. A pattern without a leading
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
| `enter`                     | open in the preview at the line                              |
| `alt+enter` `ctrl+o`        | open in the editor                                           |
| `ctrl+t`                    | insert the path into the terminal                            |
| `ctrl+y`                    | copy the relative path                                       |

Other keys edit the focused field (see [Prompts](#prompts)). `ctrl+o` is there because Windows Terminal keeps
`alt+enter` for full screen. Opening a result or inserting its path closes the popup; on failure, the popup stays
and shows the message.

Go to file counts the files on the right of the field (`12 of 840`, or `840` with no query). In Search in files,
the rule above the results counts them (`37 results in 9 files`), and shows `indexing…`, `searching…` and the
messages.

### Mouse

Click a tab, a chip or a field. Click a result to select it, and double-click it to open it. In
Search in files, clicking a file's header row folds or unfolds it. The wheel scrolls 3 rows. From 100
columns wide, the right half previews the selected result.

## Source control

A full-tab pane for the repository of the explorer's root (with `prefix+d` and no explorer: the focused
terminal's folder). Outside a repository, it shows *Not a git repository*, and `i` runs `git init` there after
confirmation. Opened with `s` in the tree, it selects the selected file in Changes and shows its diff (on a
folder, its first changed file); a file without changes leaves the selection at the top, with *No changes in …*
in the status line.

### Header and tabs

The header shows the repository name, the branch (click it for the branch picker), its state against the
upstream (*up to date*, `↑ahead ↓behind`, *not published*), then the ongoing operation (MERGING, REBASING,
CHERRY-PICKING, REVERTING, APPLYING, BISECTING). On the right, one main button shows what the branch needs:
**↑ Publish branch** without an upstream, **↓ Pull 3** when it is behind, **↑ Push 2** when it is ahead,
**⇅ ↓1 ↑2** when it has diverged (pulling then says to merge or rebase in the terminal), nothing when up to date;
its icon spins while it runs. Then come **⇣ Fetch**, **↻** (refresh) and **✕** (close), with shorter labels below
80 columns.

Tabs: **Changes**, **History**, **Stashes**, **Refs** (keys `1` to `4`), the active one underlined, Changes and
Stashes with their count. In Changes, a **List · Tree** switch, the active choice filled, changes the layout.

From 100 columns, the diff of the selected file, commit, stash or tag always shows on the right of the list
(otherwise it takes the whole pane); with a section or a branch selected, that side says what to select. Drag the
separator, or press `<` and `>`, to give the list fewer or more columns; each view keeps its own share, also for
the next panes.

### Commit box (Changes)

The commit box has the message field on its own line, then the buttons. The first says what it will
do: **✓ Commit 2 staged**, **✓ Commit all 6** with nothing staged, **✓ Amend**, **✓ Continue rebase**
or **✓ Commit merge**. Then come **✧ Claude** (write the message), **☐ Amend** and, during an operation,
**✕ Abort**. In a narrow list, the labels are shorter.

- With nothing staged, committing offers to stage everything first.
- Amend fills an empty field with the last commit's subject. An empty message keeps the last message.
- During a merge, an empty message uses the one git prepared.
- Continuing is refused when it would run `exec` lines or a merge strategy other than git's own (see
  [Security](../SECURITY.md#continue)). A `git am` is continued or aborted in the terminal.
- During a rebase, **Continue** runs `git rebase --continue`. During a cherry-pick or revert, it continues when the
  message is empty, and otherwise commits with your message, then goes on with the next commit picked.
- **✧** sends the file list and the diff (staged, or else all changes) to `claude -p`, without tools, and fills
  the field with a one-line Conventional Commits message. When Claude is missing, fails or times out,
  the message is built from the file names (`add foo.js`, `update a.js and 2 other files`).

### Keys

The status line shows the keys of the moment, as many as fit, which a click runs; `?` opens the full-pane key
help of the view (see [Key help](#key-help--or-f1)).

| Key                        | Action                                                          |
| -------------------------- | --------------------------------------------------------------- |
| `q` `escape` `ctrl+c`      | close (below 100 columns, `q` and `escape` close the diff first) |
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
| `f` / `L` / `P`            | fetch / pull (fast-forward only) / push. The first push publishes the branch on `origin` (else the first remote) and sets its upstream |
| `ctrl+p` / `ctrl+f`        | Go to file / Search in files, in the repository                 |
| `b`                        | branch picker                                                   |
| `m` `shift+f10`            | the [menu](#git-operations) of the selected branch or commit and of the repository (also `⋯` in the header) |
| `T`                        | create a tag on HEAD                                            |
| `c`                        | type the commit message (from any view: back to Changes)        |
| `ctrl+g`                   | write the message with Claude (same)                            |
| `alt+a`                    | toggle amend                                                    |

**Changes view**: `s`/`space` stage, `u` unstage, `a` stage all, `A` unstage all, `d` discard (after
confirmation), `t` list/tree (kept for the next panes), `X` abort the ongoing operation. These keys act on the
selected file, folder or section; the files staged or unstaged are highlighted for a moment where they land. In
*Changes*, `d` puts tracked files back to the index and, unlike the explorer's *Discard changes*, **deletes
untracked files for good**, not to the trash, with the folders left empty; in *Staged Changes*, the index and the
file go back to the last commit, and files added since are deleted. Conflicts cannot be discarded. On a file, as
in the explorer and the preview, `y` / `Y` copy its absolute / relative path, `e` edits it in the terminal editor
and `o` opens it in the editor.

**Stashes view**: `n` new stash, named on a row above the list (the message is optional): `enter`
stashes the untracked files too, `alt+enter` or `ctrl+o` leaves them. `a` apply, `o` pop, `d` drop.

**File history**: from the explorer (`H` or **File history**) or the preview (`H`), the History view shows only the
commits that changed the file, each linked to the previous one in the graph, and a commit's diff is limited to the
file. The file's path shows on the right of the tabs, with its former name when it was renamed (`← old.js`, or the
count of names): a click on it, or `x`, goes back to every file. Renames are followed: `git log --follow` finds the
names the file had on the current branch, and the history lists the commits of all of them, so a file that later
took one of those names shows there too.

**History view**: the branch graph, the subject, the branches and remote branches at the commit (in their lane's
colour) and its tags (yellow); on the right, the author (from 90 columns), the age and the short hash. Each branch
has its own lane and colour, linked to the commit it started from and to the merge that brought it back; its
commits are marked ┿ on its line (┯ for a branch tip, ┷ for the first commit), and the checked-out commit's
subject is bold. It shows every local and remote branch, or with `a` only the current one (kept for the next
panes). The graph takes 24 columns at most: lanes further right are cut. On a commit, `t` creates a tag and `C`
cherry-picks it onto the current branch (refused when the branch already has it).

**Refs view**: local branches, remote branches, tags, remotes and worktrees; the empty ones are named
together on the last line. `enter` checks out a branch (a remote branch switches to its local branch, or
creates one tracking it). `D` deletes a local branch, and offers `-D` when it is not fully merged,
saying how many commits would be lost (those on no other branch). `M` merges the selected branch into the current
one, `R` rebases the current branch onto it.

**Comparing branches**: from 100 columns, the selected branch is compared on the right with the current branch:
commits ahead and behind, the commits only on each side (`=` marks a change also on the other side, as after a
cherry-pick), and the files it changed since it left the current branch. `=` opens the comparison at any width and focuses it; there, `up`/`down` select a commit,
`enter` shows it, and `q` or `escape` on that commit goes back to the comparison. `B` makes the selected branch the
base of the comparisons instead of the current branch (`B` again on it, or on the current branch, goes back).

### Git operations

`m` (or the menu key, `shift+f10`, the header's `⋯`) opens a menu: first the actions of the selected branch (Refs)
or commit (History), then those of the repository, each with its key when it has one:

| Item | What it runs |
|---|---|
| Merge into / Rebase onto (`M` / `R` on a branch), Merge a branch… / Rebase onto a branch… | `git merge` / `git rebase` with the chosen branch, after a question; the menu items open the branch picker to choose it |
| Create tag… (`t` on a commit, `T` for HEAD) | a lightweight tag named in place (`enter`); `ctrl+o` asks for a message and makes an annotated tag |
| Cherry-pick (`C` on a commit) | `git cherry-pick`, after a question |
| Force push (with lease)… | `git push --force-with-lease`, never a plain `--force`; the question says how many commits of the upstream would be lost |
| Undo last commit… | `git reset --soft HEAD~1`: the changes stay staged and the message comes back in the commit box; the question warns when the commit is already pushed |
| Reset --hard… (to HEAD, or to the selected commit) | `git reset --hard`; the question counts the changed files and the commits lost (those on no other branch, tag or remote branch). Untracked files are kept |

A merge, rebase or cherry-pick that stops on a conflict switches to Changes, where the commit box continues it and
`X` aborts it. They use the `ort` strategy (git 2.33 or later), whatever the repository's `branch.<name>.mergeOptions`.

**Fetch, pull, push** never prompt and stop after 2 minutes; a repository whose own config sets network keys
is refused (see [Messages](troubleshooting.md#source-control)).

**While typing the message**: `enter` commits or continues, `escape`/`tab` leaves the field, `ctrl+g`
writes with Claude, and `alt+a` toggles amend.

**Diff focused**: `up` `k` / `down` `j` `enter`, `pageup` / `pagedown` `space`, `home` `g` / `end` `G`,
`ctrl+u` / `ctrl+d` scroll. `n`/`N` jump between hunks, `s` switches inline/side by side (side by side by default
from 120 columns; kept for the next diffs), and `tab`, `q` or `escape` go back to the list (below 100 columns, `q` and `escape` close
the diff).

**Branch picker** (`b`): starts with *New branch…*, then local and remote branches. `up`/`down`
(`k`/`j`), `pageup`/`pagedown`, `enter` switch, `n` new branch, `D` delete (not a remote or the
current branch), `q` closes. `/` or any other character starts the filter, where `k` `j` `n` `D` `q`
are typed; `escape` clears the filter, then closes. A new branch is named on the picker's first row,
from the filter or else the current branch's prefix (`feat/`): `enter` creates it and switches to it,
`alt+enter` or `ctrl+o` only creates it, `escape` cancels.

**Questions**: discarding, deleting a branch, aborting an operation, popping or dropping a stash, staging
everything to commit, `git init` and the [git operations](#git-operations) ask in a card over the pane, with the keys of the explorer's
[confirmations](#confirmations).

### Mouse

On the selected row of Changes, buttons appear: **↶** discard and **+** stage on unstaged changes, **+** on
conflicts, **−** unstage on staged changes. Click the header and commit-box buttons, the tabs and the branch.
Click a section or folder to fold it, double-click a row to open it as `enter` does (a branch: checkout), and
click the diff to focus it. The wheel scrolls the list, the diff or the picker under the mouse.

The pane refreshes when the git folder changes, when it gets the focus, and every 5 s, which picks up
working-tree edits.

## Changes (diff pane)

Opened by **View changes** on a folder (a file's changes show in the preview's [Diff view](#diff-view)). For a file or
folder, it compares tracked files with HEAD (index and working tree;
before the first commit, the staged then the unstaged changes) and shows untracked files in full, up to 200. The
header shows the name and the added and removed line counts, then two switches, the active choice filled, which a
click changes: **HEAD · main** (what the changes are compared with, `b`) and **Inline · Split** (`s`, saved for the
diffs of that width, see [`diffLayout`](configuration.md#settings)); the footer, the keys and how far down the diff
is. The diff is inline, or
side by side from 120 columns, with changed words highlighted.

A cursor, a blue bar in the first column, moves over the rows: `up` `k` / `down` `j` `enter`, `pageup` /
`pagedown` `space`, `ctrl+u` / `ctrl+d`, `home` `g` / `end` `G`, or a click on a row; the view follows it, and the
wheel scrolls 3 rows. `n`/`N` put it on the next or previous hunk.

| Key        | Action                                                                                         |
| ---------- | ---------------------------------------------------------------------------------------------- |
| `a`        | a [review note](#review-notes) on the cursor's line (a removed line: the line now in its place), on the whole hunk from its `@@` row, on the whole file from its name |
| `A` / `C` / `u` | send the tab's notes to the agent / copy them (paths relative to the repository) / remove the last note |
| `y`        | copy the cursor's line as `path:line`, relative to the pane's folder                           |
| `b`        | compare with HEAD, or with the point where the branch left the default branch (the remote's `HEAD`, else `main` or `master`): every change of the branch, committed or not |
| `s`        | inline / side by side (kept for the next diffs)                                                |
| `q` `escape` `ctrl+c` | close                                                                               |
| `ctrl+p` / `ctrl+f` | Go to file / Search in files                                                          |
| `?` `f1`   | all the keys                                                                                   |

Lines with a note have a yellow bar in the first column.

## Show a file from an agent or a script

`src/peek.js` shows a file in the preview of the tab it runs in, scrolled to a line, opening the preview next to the
terminal when there is none, without taking the focus. Run it from a herdr pane, such as the terminal of an agent:

```sh
dir=$(dirname "$(herdr plugin list --plugin gcaudan.explorer --json | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).result.plugins[0].manifest_path))')")
node "$dir/src/peek.js" src/app.js:42
```

- The argument is `file`, or `file:places`, relative to the current folder or absolute. Places are separated by
  commas: a line (`42`), lines (`12-14`) or columns of a line (`42:5-18`, counted in characters from 1, both included);
  `42:5`, as compilers write it, is line 42. The preview scrolls to the first place and highlights them all: whole
  lines on the line highlight's colour, columns on the selection's. `n` and `N` go from one to the next (when no
  find is open), `escape` clears them; a Markdown file shows as source, so that they match its lines.
- `--last` shows the changed file of the repository (modified, added or untracked) written last.
- `--diff` before the file shows its changes ([Diff view](#diff-view)) rather than the file: the view goes to the
  first place, and the rows of the new version's lines the places cover carry a blue bar (whole lines only). A
  deleted file is accepted there.
- It prints `Preview: src/app.js:12-14,42:5-18`, or the reason on stderr with exit code 1 (2 for a wrong argument or
  place).
- `herdr plugin list` gives the plugin's folder (`manifest_path`) whether it was installed from the index or from a
  clone, so the command works in both cases.

**Peek action**: `gcaudan.explorer.peek` runs `peek.js --last` for the focused tab, in the folder of its focused
terminal: bind it to a key to see what an agent just wrote (see [Key bindings](installation.md#key-bindings)).

**Claude Code skill**: [`skills/herdr-preview`](../skills/herdr-preview/SKILL.md) tells Claude Code when and how to
call it, to show you the code it talks about or a file it just wrote. The install script links it into
`~/.claude/skills`; installed from the plugin index, link it yourself:

```sh
ln -s "$dir/skills/herdr-preview" ~/.claude/skills/herdr-preview
```

## Editor pane

The Editor pane runs a terminal editor on the file and closes when the editor exits. The editor is
the settings' editor command when it is a terminal editor, else `$VISUAL`, else `$EDITOR`, else the
first of `nano`, `nvim`, `vim`, `vi` found (`edit`, `nvim`, `vim`, `nano` on Windows).

## Scrollbars

A list that does not fit shows a scrollbar in its last column: a rail `│`, a thumb `┃` sized to the part shown,
and `▴` `▾` buttons from 4 rows tall.

- A press on the thumb grabs it: the list follows the mouse until the button is released, even outside the
  column. A press elsewhere on the rail moves the thumb under the mouse, then drags it the same way.
- A click on `▴` or `▾` scrolls one row; held, it repeats (after 400 ms, then every 50 ms) until the end of the
  list, or until the mouse leaves the button.
- The wheel still scrolls the list. The explorer's scrollbar ignores clicks while a menu is open, the preview's
  while a selection is being dragged, and source control's while the branch picker is open.

An empty list says why in grey, centred, with the keys that go on from there under it: *Empty folder*
(`n` new file, `N` folder), *No match* (`esc` clear), *No results* in the search, *No changes* in the
source control list (`ctrl+p` go to file, `2` history), *Not a git repository* (`i` git init here).
