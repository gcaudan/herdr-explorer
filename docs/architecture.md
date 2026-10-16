# How it works

The plugin is a set of CommonJS Node.js scripts that use only Node's built-in modules, with no build step.
`herdr-plugin.toml` declares them to herdr:

| Kind      | Id         | Command                       | Placement                 |
| --------- | ---------- | ----------------------------- | ------------------------- |
| startup   |            | `node src/hook.js --restore`  |                           |
| event     | `tab.created` | `node src/hook.js`         |                           |
| pane      | `tree`     | `node src/explorer.js`        | split                     |
| pane      | `preview`  | `node src/preview.js`         | split                     |
| pane      | `search`   | `node src/search.js`          | popup, 80% × 80%          |
| pane      | `scm`      | `node src/scm.js`             | zoomed                    |
| pane      | `diff`     | `node src/diff.js`            | zoomed                    |
| pane      | `edit`     | `node src/edit.js`            | zoomed                    |
| action    | `toggle`   | `node src/hook.js --toggle`   | tab context               |
| action    | `search`   | `node src/hook.js --search`   | tab context               |
| action    | `scm`      | `node src/hook.js --scm`      | tab context               |

The scripts talk to herdr through its CLI (`herdr pane …`, `herdr plugin pane open …`), whose JSON
answers they parse, and recognise the plugin's panes by their label (Explorer, Preview, Search, Source Control,
Changes, Editor). [CONTRIBUTING.md](../CONTRIBUTING.md#project-structure) gives the role of each source file, and
[SECURITY.md](../SECURITY.md) how git and the other programs are run.

## Placement

**Explorer** (`hook.js`, which exits at once when herdr is not found). When a tab is created, the hook skips the
tab unless `autoOpen` is on and the tab has a single pane that is not already an explorer. `--toggle` closes the
tab's explorer, and its preview, if there is one, and opens one otherwise. `--all`, bound to no action, opens one in
every tab that has a single pane.

1. The hook takes the top-left pane, or the top-right pane when docked on the right.
2. It opens the `tree` pane as a split to the right of that pane, without focus. The root is the
   workspace folder, or the pane's own folder.
3. It resizes the split right away to the saved width plus the 2-column border. The explorer then
   corrects any rounding itself, at 300 ms, 1 s and 2.5 s after starting, and again whenever the
   window size changes.
4. herdr only splits to the right or downwards, so when docked on the left, the hook swaps the
   explorer with the pane to move it left. When the tab still has a preview and a single terminal, the hook moves
   the preview out of the tab and back beside the terminal, at the same width, so that the explorer has a
   split of its own at the side of the tab (`settlePreview`).
5. It gives the focus back to the terminal. If the tab was created in the background, the display
   briefly switches to it, then returns to the tab you were viewing.

**Preview** (`actions.preview`). The first file opens the `preview` pane as a split to the right of
the terminal next to the explorer, giving `explorer | terminal | preview`.

**Popups and full-tab panes**. The search opens as a popup over the active pane. Source control, the
changes pane and the editor open *zoomed* (full tab), targeting the explorer's tab. `--search` and `--scm` use the
explorer's saved root, else the focused terminal's folder, else the workspace folder, and pass the explorer's pane
(`EXPLORER_PANE`), so that the files they open go to that tab's preview.

The plugin finds "the terminal next to the explorer" by walking `herdr pane neighbor` up to 4 steps
on the docked side, then on the other side, and skips the plugin's own panes.

**Right-click**. herdr keeps right-clicks for its own menu unless a pane asks for them; the explorer
does so at startup (`herdr pane input <pane> --right-click pane`).

**Closing**. When the tab's terminal exits and only the explorer is left, it closes too, and the tab
with it. The tab's preview always closes with the explorer.

## Pane environment

Inputs reach a pane through `--env` variables of `herdr plugin pane open`:

| Variable | Read by | Meaning |
|---|---|---|
| `EXPLORER_ROOT` | explorer, preview, search, scm, diff | the pane's folder |
| `EXPLORER_TREE` | explorer | the tree root, when *Set as root* moved it |
| `EXPLORER_FILE` | preview, scm, diff, edit | the file to show, select or edit |
| `EXPLORER_LINE` | preview | the line to scroll to (1-based) |
| `EXPLORER_TAB` | preview | the tab id, which names its request file |
| `EXPLORER_SEEN` | preview | the last request handled, across a restart |
| `EXPLORER_PANE` | search, scm, diff | the pane to act from (the diff's notes go to the terminal next to it) |
| `EXPLORER_HISTORY` | scm | a file whose history to show, in place of `EXPLORER_FILE` |
| `EXPLORER_MODE`, `EXPLORER_QUERY` | search | `files` or `content`, and the first query |
| `EXPLORER_EDITOR` | edit | the terminal editor command of the settings |
| `EXPLORER_COMMIT`, `EXPLORER_STAGED` | diff | a commit, or the staged changes only (no caller sets them yet) |

## Review notes

The notes of a tab are kept in `notes-<tab id>.json` in the state folder, written to a temporary file then renamed:
a list of `{ file, from, to, kind, text, t }`, the file absolute and the lines 1-based (0 for the whole file). The
preview and the Changes pane add to it and poll it every 500 ms (`fs.watchFile`), so that a note taken in one shows
in the other. Sending (`notes.send`) finds the terminal next to the explorer as **Insert into terminal** does,
builds one line with the paths relative to that terminal's folder, types it with `herdr pane send-text` (no newline,
so nothing is submitted, and control characters are removed from the notes), then removes the notes sent. The diff
pane finds its tab with `herdr pane get`.

## Images

The preview asks the terminal, at start, whether it handles the Kitty graphics protocol (`a=q` query) and the size of
a cell in pixels (`CSI 16 t`); `ui.decode` passes the answers to a `reply` handler instead of reading them as keys.
herdr answers for the outer terminal. The `images` setting chooses: pixels when the terminal answered (`auto`, the
default: until the answer, or for 500 ms, an image is not drawn), half blocks (`blocks`), or pixels without waiting
(`pixels`); `p` overrides it in one pane, and each preview polls `settings.json` every second for a new value. The
explorer sends the same query, only to tell in its settings whether the terminal has the protocol. In pixels, the
image is decoded, scaled to the pane's size in pixels (box filter), sent once as zlib-compressed RGBA in 4096-byte
chunks (`a=t`, image id `0x4846`), then placed over blank rows (`a=p`, `C=1` so that the cursor stays); it is placed
again when its place changes, and deleted when another file shows, under the key help and at exit. Without the
protocol, it is drawn with half blocks. WebP files go through `dwebp <absolute path> -o -`, whose PNG output is then
decoded like any PNG.

## Preview requests

Each tab has at most one preview, reused for every file. To show a file, the explorer or the search
looks for a pane labelled *Preview* in the tab. If there is one, it writes `{ file, root, line, t }` to
`preview-<tab id>.json` in the state folder, which the preview polls every 200 ms (`fs.watchFile`), loading each
new request at its line; otherwise it opens a new preview pane with the file in its environment.

The preview watches the folder of the file it shows rather than the file itself, because editors and
agents often replace a file by renaming a new one over it. It reloads 80 ms after a change, keeping
the scroll position.

## Restore after a herdr restart

herdr restores plugin panes as plain shells, keeping only their label and folder. To recover what
they showed, each pane writes its state to `panes/<pane id>.json`:

- the explorer: the tab folder (`home`) and the current tree root (`root`);
- the preview: `file`, `root` and `line`.

At startup, `hook.js --restore` closes, in every workspace, the panes labelled as the plugin's whose foreground
process is not `node`. In each tab that had an explorer, it opens a new one, at the saved root when both saved
folders still exist; in each tab that had a preview, it shows the saved file at its line, when the file still
exists. The other plugin panes are not reopened. It then deletes the state files of panes that no longer exist.
herdr keeps pane ids across a restart, which is what lets each pane find its saved state.

The startup hook and the `tab.created` hook also run the daily update check.

## Refresh and watchers

| Pane            | What triggers a refresh                                                                     |
| --------------- | ------------------------------------------------------------------------------------------- |
| Explorer        | `fs.watch` on the root, recursive unless the root is `$HOME`, a parent of it or the filesystem root (then top level only, or when the recursive watch fails). Changes inside `.git` or in collapsed ignored folders are skipped |
| Explorer        | `fs.watch` on the git folder for `index` and `HEAD` (commit, stage, branch switch)          |
| Explorer        | a 30 s safety rescan. A `.git` appearing later (`git init`, clone) is detected              |
| Explorer        | `settings.json`, `keys.json` and `update.json`, polled every second                         |
| Explorer        | the folder of the neighbouring terminal, polled every 2 s when **Follow** is on             |
| Preview         | the file's folder, and the tab's request file every 200 ms                                  |
| Source control  | the git folder (`index`, `HEAD`, `ORIG_HEAD`, `FETCH_HEAD`, `MERGE_HEAD`, `packed-refs`, `stash`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`), the common git folder and `refs/` (worktrees), a 5 s poll, and focus |

Refreshes are debounced to one every 250 ms and never overlap. A file written continuously, such as a
log, cannot postpone them forever. Git status runs with `GIT_OPTIONAL_LOCKS=0`, so it never takes
`index.lock` and a concurrent `git commit` does not fail.

Screens are painted line by line. Only the lines that changed since the last frame are written,
inside synchronized-update sequences. The panes use the alternate screen, SGR mouse reports, focus
reports and bracketed paste. `ui.textWidth` counts terminal columns (wide characters, emoji, combining marks), so
that each line fills the pane exactly, and control characters from names, files and git never reach the terminal
(`ui.clean`, `ui.cleanLine`).

## Pane widths

herdr keeps the ratio of each split, so resizing the window or another pane rescales the explorer and the
preview. After each resize, `actions.widthKeeper` sets back the ratio of the split of which the pane is a whole
side (`sideSplit`, `herdr pane resize`), at least 10 columns and leaving 20 to the other side. When the pane's own
split changed ratio (its border dragged), it keeps the new width instead, which the explorer saves in `width`.
`<` and `>` change the width kept; the other kept panes this moves are named in the `resizing` file for 500 ms and
put back by the same pane, so that they are not moved twice (herdr resizes by a relative amount).

## Launcher and self-restart

The explorer and preview panes run through a small launcher (`launch.js`). The pane's first process
stays as the launcher and runs the real script as a child process with an IPC channel.

- The launcher keeps the terminal in raw mode between children, so a `ctrl+c` typed during a restart
  cannot kill the pane, and forwards `SIGTERM`, `SIGHUP` and `SIGINT` to the child. If it is killed, the child
  exits too (IPC `disconnect`).
- `update.watch` polls the `version` line of `herdr-plugin.toml` every 2 s. When it differs from the
  version the process started with, the pane waits 2 s, so that a pull has written every file. Then
  it checks every second until the pane is idle (see [Pane restart](installation.md#pane-restart)).
- To restart, the child releases the terminal and sends the launcher what the next process must show
  (`EXPLORER_TREE` for the explorer; `EXPLORER_FILE`, `EXPLORER_ROOT`, `EXPLORER_LINE` and
  `EXPLORER_SEEN` for the preview), then exits with code **75**. The launcher starts the script again
  with that environment, in the same terminal, so the pane keeps its id, place and width.
- Any other exit code ends the launcher, and the pane with it.

A process cannot replace itself on Windows, and one that only waited for its successor would keep its memory,
one more at each update. The launcher loads nothing else, so it stays on the code it started with.

The search, source control, changes and editor panes are short-lived and have no launcher. A pane
started by a version without the launcher (0.4.0 and earlier) is the exception, see
[Limitations](troubleshooting.md#limitations).
