# Explorer

## Header

The header shows the root folder's name in capitals and bold, in bold italic when **Set as root**
moved the tree away from the tab's folder. After an action, a short message replaces it: `✓` done (2 s), `·`
nothing to do (2 s), `✕` in red failed (4 s), `⋯` running. The preview and source control show their messages the
same way, in their bottom line, cut to leave room for the pill and the gear on the right, which are clickable:

| Pill or icon     | Click                                                                  |
| ---------------- | ---------------------------------------------------------------------- |
| `↑ 0.9.0`        | the newer release (green, when one exists, is not skipped and **Check for updates** is on): the installed and new versions, its first changes, and `c` copy herdr's update command, `r` its notes in the browser, `s` skip this version (no pill until the next one), `n` later |
| `✓ what's new`   | for a minute after an update (blue, the new version in a message): the release notes in the browser |
| gear             | Settings                                                               |

Below 24 columns, a pill shows its mark only (`↑`, `!`, `✓`).

## Tree

Folders come first, then files, sorted by name with numbers in natural order. The `.git` folder is
never shown. With **Compact folders** (on by default), a folder whose only entry is a folder shares its row, as in
VS Code: `src/main/java` opens and closes as one, and `←` goes to the folder above the chain. A folder not yet
opened shows alone until it is: the tree reads only the folders it shows. The filter shows every
folder on its own row. Git decorations follow VS Code:

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

A root that is in no repository, such as a folder holding several projects, shows the changes of the repositories
of its folders (one level down, not in `$HOME`, its parents or `/`): each folder holding one gets its `●`, and every
git action runs in the repository of its entry. A repository made in one of those folders later (`git init`,
clone) is found as it appears.

## Keys (defaults)

The last column is the action's name in [`keys.json`](../configuration.md#config-folder-keysjson).

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
| `escape` `q`             | close the menu, the filter, the changed files view or the page shown, else the explorer and the tab's preview | `close` |
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
| `e`                      | open in the external editor                                                    | `openInEditor` |
| `E`                      | edit the file in the terminal editor                                           | `edit` |
| `o`                      | open with the system application (documents, media, text formats), other files in the editor, a folder in the file manager | `openInApp` |
| `p`                      | preview the file in a kept tab and focus the preview                           | `preview` |
| `y` / `Y`                | copy the absolute / relative path                                              | `copyPath` / `copyRelativePath` |
| `backspace`              | make the parent folder the tree root                                           | `parentRoot` |
| `c`                      | changed files only (see [Reviewing changes](#reviewing-changes))               | `changedOnly` |
| `d`                      | changes of the file in the preview ([Diff view](preview.md#diff-view)); of a folder in the Changes pane | `viewChanges` |
| `]` / `[`                | next / previous changed file, previewed                                        | `nextChange` / `previousChange` |
| `H`                      | history of the selected file, in source control                               | `fileHistory` |

## Mouse

- **Click** a folder to fold or unfold it, a file to select it (and preview it, with **Preview follows selection**
  on, once the selection rests for 150 ms). **Double-click** a file (within 400 ms) to open it.
- **Right-click** a row for its action menu, below the last row for the root menu: in this pane, right-clicks go
  to the explorer instead of herdr's menu.
- **Wheel** scrolls 3 rows (in a menu, it moves the selection). See also [Header](#header) and
  [Scrollbars](../usage.md#scrollbars).

## Filter

`/` opens a filter field at the bottom. The tree keeps the entries whose name contains the text (any case), with
their parent folders dimmed, the match in bold blue, and a count of matches. Git-ignored folders are not searched,
the filter reads 20000 entries at most, and folders cannot be folded while filtering.

- Typed characters go to the field, as do `backspace`, `delete`, `ctrl+u` and `ctrl+d` (see
  [Prompts](#prompts)). The other bound keys that are not characters (arrows, `enter`, `ctrl+p`, `ctrl+f`…) act
  on the list.
- `tab` leaves the field and keeps the filter, so the single-letter keys work again; the bottom line then shows the
  filter in blue and `esc clear`. Click it to type again.
- `escape` closes the filter and reveals the selected entry in the full tree.

## Reviewing changes

`c` keeps in the tree only the changed files (the git badges `M`, `A`, `U`, `R`, `D`, `!`) and the folders holding
them, all unfolded; the bottom line shows `● Changes` and their count, and `escape` (or `c` again) brings the whole
tree back with the selected entry shown. The filter (`/`) then searches the changed files only.

`]` and `[` move to the next and previous changed file in the tree's order, unfolding its folders, and show it in the
preview; past the last one, they start again from the first. Among the changed files, the preview shows each one's
changes ([Diff view](preview.md#diff-view)), as **Preview follows selection** does there; in the whole tree, the file itself.

Files and folders deleted from the disk show only among the changed files, struck through with the `D` badge; `]`
and `[` reaching one switch to that view. `enter` or a double-click shows what it held in its diff, and its menu keeps
**View changes**, **File history**, **Stage**, **Unstage**, **Discard changes…** (which restores it) and **Copy
relative path**; the actions that need the file on disk tell how to restore it instead.

## Prompts

**Rename** turns the row into a field, the cursor before the extension; a change of case only is allowed.
**New file** and **New folder** open the folder and add a field at the top of it, whose icon follows the name
typed. A name may contain subfolders (`a/b.txt`, created as needed) but cannot leave the folder; a name ending
with `/` creates a folder. The editor command is typed on the bottom line, where an empty one restores the
default. The bottom line shows the keys (`⏎ rename`, `⏎ create in src`, `esc cancel`): `enter` accepts,
`escape` or any click cancels.

Every text field (prompts, filter, finds, search and source control fields) edits with these keys, unless the
pane uses the key itself (in the search, `ctrl+k` moves in the results and `ctrl+f` switches tab), and takes
pasted text.

| Key                               | Action                              |
| --------------------------------- | ----------------------------------- |
| `left` `right`, `ctrl+b` `ctrl+f` | move the cursor                     |
| `home`, `end`                     | start / end                         |
| `backspace`, `delete` `ctrl+d`    | delete before / after the cursor    |
| `ctrl+u` / `ctrl+k`               | delete to the start / to the end    |
| `ctrl+w` `alt+backspace`          | delete the word before the cursor   |

## Confirmations

Moving to the trash, deleting for good, discarding changes and updating ask first, in a card over the
pane: the question, the full name and its folder, a note when the action cannot be undone, then two
buttons, Cancel selected at first. `left`/`right` (also `h`/`l`, `tab`/`shift+tab`) move between them, `enter`
presses the selected one, and `y` or a click on the action's button confirms. Any other key or click cancels.
Source control asks the same way.

## Opening files

`enter`, a double-click or the menu's first item decides as follows:

1. With **Enter opens** set to `preview` (the default), a file the preview can show goes to the preview:
   every file except documents and media other than images (Office files, PDF, archives, audio and video).
2. Otherwise, documents and media (`png jpg jpeg gif webp bmp ico svg pdf doc docx xls xlsx ppt pptx
   odt ods odp zip 7z rar gz tgz mp3 mp4 wav mkv avi mov`) open with the system application.
3. Everything else goes to the editor. A double-click never runs an executable or a script.

## Action menu (on a file or folder)

| Item                     | Shown for                         | Action                                                       |
| ------------------------ | --------------------------------- | ------------------------------------------------------------ |
| Preview                  | files the preview can show        | show in the tab's preview                                    |
| Open in editor           | files and folders                 | external editor: the settings' command, else VS Code when found, else the Editor pane (see [configuration](../configuration.md#editor-command)) |
| Open in app              | `txt md csv log json xml html htm` and the media types above | system application |
| Open in file manager     | folders                           | the folder in the system's file manager                      |
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
needed. A name with a control character is refused.

**Delete…** moves the entry to the trash (see [Platform tools](../installation.md#platform-tools)). Without one, a
second confirmation offers to delete it for good.

**Discard changes…** puts tracked files back to their last commit, staged changes included. Files
added since the last commit, and untracked files, go to the trash instead of being deleted.

## Root menu (right-click below the rows)

New file…, New folder…, Go to file…, Search in files…, Source control and Changed files only (or All files) in a
repository (outside one, Source control opens the repository of the selected entry), Open folder
(Reveal in Explorer), Copy path, Up one level, Back to the space folder (after **Set as root**),
Refresh, Settings….

## Settings (`,` or the gear icon)

The settings take the place of the tree, by group, each with its value on the right: `● on` or
`○ off` for a switch, the value chosen otherwise. A line under the list explains the selected
setting. The settings are shared by every explorer. See [configuration.md](../configuration.md#settings).

| Group      | Settings                                                            |
| ---------- | ------------------------------------------------------------------- |
| Appearance | Theme, Icon set, Icon theme                                         |
| Tree       | Hidden files, Git-ignored files, Compact folders, Follow terminal   |
| Opening    | Enter opens (preview or editor), Preview follows selection, Editor  |
| Preview    | Wrap lines, Line numbers, Changes, Images, Diff layout (auto, split, inline) |
| Review     | Notes prefix, Source control tool                                   |
| New tabs   | Open automatically, Dock side (left or right)                       |
| Advanced   | Key bindings, Check for updates                                     |

`up`/`down` (and their bound keys, page and first/last included) move, `/` finds a setting, `right`/`l` and
`enter` give a setting its next value (on **Icon theme**, **Editor**, **Notes prefix**, **Source control tool** and **Key bindings**: open its
page or prompt), `left`/`h` its previous one. **Theme** chooses dark or light colours for every pane (VS Code Dark+ or
Light+, syntax and Markdown included); on **auto**, the default, each pane asks the terminal its background colour at
start and turns light on a light one. A newer release shows a line under the title, a skipped one too (dimmed): `u`
opens the update, as the header's pill does. `escape`, and the keys bound to close and settings, go back to the tree. The wheel moves the selection, a
click on a setting changes it, a click on the title goes back.

**Icon theme** opens a page of its own: the Nerd Font default and the VS Code icon themes installed (see
[Icon themes](../configuration.md#icon-themes)), the saved one marked `✓`, above a sample tree of folders and files
in the theme under the selection. `enter` (or a click) keeps it and goes back to the settings; `escape` goes back
without a change. **Icon set** (Nerd Font, emoji, Unicode, ASCII) is for a terminal without a Nerd Font (see
[Without a Nerd Font](../configuration.md#without-a-nerd-font)).

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
saved in [`keys.json`](../configuration.md#config-folder-keysjson) and apply at once.

## Key help (`?` or `f1`)

Every key of the explorer, as bound now, takes the place of the tree, by group, over the pane's full height. It starts
with **Panes (herdr keys)**: the keys herdr's config binds to the plugin's actions, which work from any pane (open or
close the explorer, go to file, source control, preview the last changed file; `—` when the config binds none, see
[Key bindings](../installation.md#key-bindings)). The arrows, page keys, `j` `k` `g` `G`,
`ctrl+u` `ctrl+d` and the wheel scroll it, `/` finds a key or what it does. `escape`, `q`, `?`,
`f1`, or a click on the title, goes back to the tree. The preview, the search, source
control and the changes pane show their keys the same way (`?` or `f1`; `f1` only in the search). Each help shows the
plugin's version and the commit it was built from at the right of its footer (`v1.0.0 · 7807fc4`).

## Find in the pages

In the settings, the key bindings and the key help, `/` opens a find field under the title. The list keeps the
rows whose name, value, keys or help has the text typed, and the field counts them. Typed characters go to the
field; the arrows and `enter` still act on the list. `escape` closes the find, the selection staying on its row.

## Menu keys

The menu's frame is titled with the entry it acts on. Each item shows the key of its action after its label,
dimmed, which runs it while the menu is open. **Delete…** and **Discard changes…** are in red. `up`/`down` (and
their bound keys) move, skipping separators, and `enter`/`space` (and the other keys bound to open) run the item;
`right`/`l` do nothing, so that an arrow never runs an action. `escape` always closes, as do the keys bound to close,
collapse and menu; the key of settings opens **Settings…**. A long menu scrolls, with `▴` `▾` on its border. A click runs an item; a click elsewhere closes the menu.

## Other behaviours

- **Width**: the explorer is 32 columns wide by default, 10 at least. Resizing it by hand (its border, `<` `>`, or the `shift`
  arrows) keeps the new width for the next explorers. The preview keeps its width the same way while it is open.
  Resizing one of them, or the window, leaves the other's width alone: the terminal takes the difference. Closing
  the explorer (`q`, `esc` or `prefix+f`) closes the tab's preview too, so that the tab stays
  `explorer | [terminal | preview]` and dragging the border between the terminal and the preview leaves the
  explorer still; an explorer opened in a tab that still has a preview puts it back beside the terminal. On KDE
  Plasma, holding `<` or `>` shows the accented characters popup instead of repeating the key, unless **System
  Settings → Keyboard → When a key is held** is set to **Repeat the key**; the `shift` arrows repeat either way.
- **Alone in a tab**: when the terminal exits and only the explorer is left, it closes, and the tab
  with it.
- **Refresh**: the tree follows file changes (at most every 250 ms; only the top level in `$HOME`, one of its parents
  or the filesystem root) and changes of git's index or HEAD, with a full rescan every 30 s. Expanded folders are
  remembered per tree root.
- **Tree root**: the root chosen with **Set as root** or `backspace` comes back after a herdr restart.
- **Follow the terminal folder**: when on, the explorer checks every 2 s the folder of the terminal next to it
  (that of its running program when herdr knows it) and reveals it. Only the explorer of the tab herdr shows checks;
  the others look again as soon as their tab is shown. A folder outside the tree root but inside the
  tab's folder brings the root back to the tab's folder; a folder outside the tab's folder becomes the root, except
  `$HOME`, one of its parents or the filesystem root, which are not followed.
