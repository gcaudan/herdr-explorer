# How it works

The plugin is a set of CommonJS Node.js scripts that use only Node's built-in modules, with no build step.
`herdr-plugin.toml` declares them to herdr:

| Kind      | Id         | Command                       | Placement                 |
| --------- | ---------- | ----------------------------- | ------------------------- |
| startup   |            | `node src/hook.js --restore`  |                           |
| event     | `tab.created` | `node src/hook.js`         |                           |
| event     | `tab.closed`, `workspace.closed` | `node src/hook.js --closed` |        |
| event     | `pane.agent_status_changed` | `node src/hook.js --agent` |              |
| event     | `tab.focused` | `node src/hook.js --focused` |                         |
| pane      | `tree`     | `node src/explorer.js`        | split                     |
| pane      | `preview`  | `node src/preview.js`         | split                     |
| pane      | `search`   | `node src/search.js`          | tab                       |
| pane      | `scm`      | `node src/scm.js`             | tab                       |
| pane      | `diff`     | `node src/diff.js`            | split                     |
| pane      | `edit`     | `node src/edit.js`            | tab                       |
| action    | `toggle`   | `node src/hook.js --toggle`   | tab context               |
| action    | `search`   | `node src/hook.js --search`   | tab context               |
| action    | `scm`      | `node src/hook.js --scm`      | tab context               |
| action    | `peek`     | `node src/peek.js --last`     | tab context               |
| action    | `open-link` | `node src/peek.js --url`     | link handler `file`       |
| link      | `file`     | `^file://` → `open-link`      | ctrl+click on a link      |

The scripts herdr runs are at the root of `src/`, with `launch.js` (the launcher of the explorer and preview, see
[Launcher and self-restart](#launcher-and-self-restart)). Shared modules are in `src/core/` (programs and state files,
actions, terminal UI, the editing buffer, preview tabs, review notes, icons, updates) and `src/render/` (syntax
highlighting, Markdown, images, branch graph); the explorer and source control keep theirs in `src/explorer/` and
`src/scm/`. `diff.js` is the Changes pane and also the diff parser and renderer that the preview's Diff view and
source control load. The `*.js` files at the top of the repository only load their counterpart in `src/`, for panes and
hooks started from a 0.5.x manifest until the installer is run again.

The scripts talk to herdr through its CLI (`herdr pane …`, `herdr plugin pane open …`), whose JSON
answers they parse, and recognise the plugin's panes by their label (Explorer, Preview, Search, Source Control,
Changes, Editor). [CONTRIBUTING.md](../CONTRIBUTING.md#project-structure) gives the role of each source file, and
[SECURITY.md](../SECURITY.md) how git and the other programs are run.

## Placement

**Explorer** (`hook.js`, which exits at once when herdr is not found). When a tab is created, the hook skips the
tab unless `autoOpen` is on and the tab has a single pane that is not already an explorer. `--toggle` opens the
tab's explorer when it has none, and closes it, with its preview, when it has one. The hook and `--toggle` open no
explorer in a tab holding only the plugin's panes (the editor's).

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

**Side panes, tabs of their own and the editor**. The search opens in a tab of its own (`EXPLORER_ORIGIN`: the tab
it was opened from, recorded in `search-<tab id>.json`, `actions.holdTab`), beside a preview of the result selected,
which keeps its share of the width (`searchShare`); the result opened goes to the origin's preview. The changes
pane opens on the right of the terminal, in place of the preview (`actions.sidePane`). Source control opens a review
in a tab of its own (`actions.scm`, `EXPLORER_REVIEW`: the tab reviewed), renamed `(SC <label>)`, and records it in
`review-<tab id>.json`; the `tab.focused` event leads from the tab reviewed to its review (`hook.js --focused`), and
the review's exit, whatever its cause, removes the record and closes the review's previews, so its tab. The selection there shows in
a preview on the right of source control (`beside`, which also tells it that `q` closes it; a commit as a `show`
request, `diffs.showArgs`; another line as a blank one), opened by the first selection and then by `Enter` only, which keeps its share of the width (`keepShare`,
`reviewShare` setting). The editor opens in a tab of its own, in the explorer's workspace; closing it closes the tab. `z` zooms the preview,
source control and the changes pane, which take the zoom off when they lose the focus (focus reports,
`actions.unzoom`): herdr's zoom belongs to the tab and follows the focus, so a zoom left on would show the agent alone
once the focus comes back to it, the explorer hidden. `--search` and `--scm` use the
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
| `EXPLORER_MARKS` | preview | the parts to highlight (JSON, see [Preview requests](#preview-requests)) |
| `EXPLORER_VIEW` | preview | `diff`: the file opens in the Diff view |
| `EXPLORER_KEEP` | preview | `1`: the file opens in a kept tab rather than the temporary one |
| `EXPLORER_TABS` | preview | the tabs to restore, `{ list, current }` (JSON), after a herdr restart or a self-restart |
| `EXPLORER_TAB` | preview | the tab id, which names its request file |
| `EXPLORER_SEEN` | preview | the last request handled, across a restart; on opening, the request file as it was, to take the requests made while the preview starts |
| `EXPLORER_PANE` | search, scm, diff | the pane to act from (the diff's notes go to the terminal next to it) |
| `EXPLORER_HISTORY` | scm | a file whose history to show, in place of `EXPLORER_FILE` |
| `EXPLORER_MODE`, `EXPLORER_QUERY` | search | `files` or `content`, and the first query |
| `EXPLORER_EDITOR` | edit | the terminal editor command of the settings |

## Review notes

The notes of a tab are kept in `notes-<tab id>.json` in the state folder, written to a temporary file then renamed:
a list of `{ file, from, to, kind, text, t, anchor, stale }`, the file's real path (symbolic links resolved) and the
lines 1-based (0 for the whole file). `anchor` holds the text of the lines when the note was taken (up to 200 lines);
`notes.current`, called when the notes or the file are read again, looks for it in the file as it is (the nearest
place to the old one), moves the note there, or marks it `stale`, and saves the list when a note changed. The preview and the Changes pane add to it and poll it every 500 ms (`fs.watchFile`), so that
a note taken in one shows in the other. Sending (`notes.send`) finds the terminal next to the explorer as **Insert into terminal** does,
builds one line with the paths relative to that terminal's folder, types it with `herdr pane send-text` (no newline,
so nothing is submitted, and control characters are removed from the notes), then removes the notes sent. Copying
(`notes.copy`) puts them on the clipboard, one per line, and keeps them. The diff pane finds its tab with
`herdr pane get`.

## Theme

`ui.C` and `ui.GIT` hold the colours of the theme in use, dark or light; `ui.useTheme` swaps them in place, and calls
back the modules that work styles out once (`ui.onTheme`: syntax colours, Markdown, diff colours, scrollbars…). A pane
reads the `theme` setting at start (`ui.settingTheme`): on `auto`, the `background` file of the state folder, dark
without it. `ui.screen` then asks the terminal its background colour (OSC 11, `ESC ] 11 ; ? BEL`; herdr answers for
the outer terminal) when the setting is `auto`. `ui.decode` passes the answer (`ESC ] 11 ; rgb:rrrr/gggg/bbbb`) to the
pane's `reply`; a luminance above one half means light. The pane writes `background` when it changed, so that the next
panes open in the right theme at once, and, when its own theme differs, swaps it and redraws everything (its `theme`
handler, else `resize`). A terminal that does not answer leaves the theme as it was.

## Images

The preview asks the terminal, at start, whether it handles the Kitty graphics protocol (`a=q` query) and the size of
a cell in pixels (`CSI 16 t`); `ui.decode` passes the answers to a `reply` handler instead of reading them as keys.
herdr answers for the outer terminal. The `images` setting chooses: pixels when the terminal answered (`auto`, the
default: until the answer, or for 500 ms, an image is not drawn), half blocks (`blocks`), or pixels without waiting
(`pixels`); `p` overrides it in one pane, and each preview polls `settings.json` every second for a new value. The
explorer sends the same query, only to tell in its settings whether the terminal has the protocol. In pixels, the
image is decoded, scaled to the pane's size in pixels (box filter), sent once as zlib-compressed RGBA in 4096-byte
chunks (`a=t`, image id `0x4846`), then placed over blank rows (`a=p`, `C=1` so that the cursor stays); it is placed
again when its place changes, and deleted when another file shows, under the key help and at exit. In rendered
Markdown, each image gets an id of its own and blank rows tagged with it (`figure`); after each frame the images in
view are placed cut to their visible rows (source rectangle `x,y,w,h`, computed by `image.crop`), a placement is
removed when its image scrolls out, and an image is deleted when it leaves the rows (another file, the source or
Diff view, a resize, `p`). Without the
protocol, it is drawn with half blocks. WebP files go through `dwebp <absolute path> -o -`, whose PNG output is then
decoded like any PNG.

## Preview requests

Each tab has at most one preview, reused for every file. To show a file, the explorer, the search, `peek.js` and
`hook.js --restore` call `actions.preview`, which looks for a pane labelled *Preview* in the tab (source control and
the Changes pane show files only through the search they open). If there is one, it writes `{ file, root, line, marks, view, keep, t }` to
`preview-<tab id>.json` in the state folder, which the preview polls every 200 ms (`fs.watchFile`), loading each
new request at its line; otherwise it opens a new preview pane with the file in its environment. Calls run one at a time, so that clicks made
while a preview opens find it rather than each opening their own. `marks`, from
`peek.js` only, are the parts to highlight, `{ from, to, a, b }` (lines, and columns, 0 for whole lines): the preview
keeps at most 200 well-formed ones (`EXPLORER_MARKS` for a new pane). `view: 'diff'` opens the Diff view
(`EXPLORER_VIEW`), whose changes come from `diff.collect`, the function the Changes pane uses too, run from the
nearest folder of the file still on disk. `keep` opens the file in a kept tab; otherwise it replaces the temporary
tab. The tab list itself (`src/core/tabs.js`: open, keep, close, next, the strip's layout for a width) is pure and
tested; the preview keeps each tab's scroll, view and marks beside it, and saves the list with its pane state, which
`hook.js --restore` gives back as `EXPLORER_TABS`. At most 50 tabs: past it, the one furthest from the new tab goes.

Editing (`i`) keeps a `buffer.Editor` in the tab: the lines, the cursor and selection, and the undo history. Its
pure logic is in `src/core/buffer.js` and tested: reading (UTF-8 only, at most 2 MB, line endings detected),
cursor columns over wide characters and tabs, the changed lines (Myers' diff of the lines' hashes against the text
at opening, at most 1000 differences), find and replace, and saving (a temporary file with the same mode, renamed
over the file, after checking that its mtime and size have not changed). Only the visible lines and 200 around them
are highlighted again after an edit.

The preview watches the folder of the file it shows rather than the file itself, because editors and
agents often replace a file by renaming a new one over it. It reloads 80 ms after a change, keeping
the scroll position.

`peek.js` (an agent's call, or the **peek** action) makes the same request from outside the plugin. It runs in the
agent's pane, where herdr gives `HERDR_PANE_ID` but not the plugin's state folder: it reads that folder's path from
`state-dir` in the config folder (`herdr plugin config-dir`), which every hook run saves, else takes herdr's usual
place. As the action, it takes the focused pane of the tab from `HERDR_PLUGIN_CONTEXT_JSON`, or the tab's first
terminal when the focus is on one of the plugin's panes. Its argument's places (`file:12-14,42:5-18`) are read by
`src/core/places.js`, pure and tested.

## Restore after a herdr restart

herdr restores plugin panes as plain shells, keeping only their label and folder. To recover what
they showed, each pane writes its state to `panes/<pane id>.json`:

- the explorer: the tab folder (`home`) and the current tree root (`root`);
- the preview: `file`, `root` and `line`, and its tabs (`tabs`: `{ list, current }`, each tab's file, kept flag,
  scroll and view).

At startup, `hook.js --restore` closes, in every workspace, the panes labelled as the plugin's whose foreground
process is not `node`. In each tab that had an explorer, it opens a new one, at the saved root when both saved
folders still exist; in each tab that had a preview, it reopens the saved tabs whose file still exists, on the
current one. The other plugin panes are not reopened. It then deletes the state files of tabs and panes that no longer exist, as `--closed` does on the
`tab.closed` and `workspace.closed` events.
herdr keeps pane ids across a restart, which is what lets each pane find its saved state.

`hook.js --agent` follows the agents' turns for the [Last agent turn](usage.md#diff-scope) scope
(`core/scope.js`): when an agent starts working after having been idle or done, or another agent of the tab does, it
writes the tree of the repository's working tree to `turn-<tab id>.json`. The tree is made from a copy of the index
(`GIT_INDEX_FILE`, `git add -A`, `git write-tree`), so the real index is left alone and git rehashes only the files
changed since; its objects stay in the repository until `git gc` prunes them. The copy is kept in `index/` in the
state folder, not in the git folder, which a repository received as an archive could fill with a symbolic link; the
copies unused for a week are removed at startup.

## Updates

The startup and `tab.created` hooks, never the panes, run the update check (`update.check`): unless `updateCheck` is
off, at most once a day, it asks the GitHub releases API for the latest release and saves it in `update.json`, with
the changes its text lists (`update.releaseNotes`: the bullets of release-please's notes, links and emphasis dropped)
and its page. The explorers poll it to show `↑` and the version when it is newer than the version in
`herdr-plugin.toml` and not the one skipped (`skip`). Updating is a `git pull --ff-only` of the clone, one at a time
across tabs (`update.lock`, taken over after 2 minutes); it writes `updated` (`{ from, to, t }`), so that the explorers
restarting on the new code show it for a while. When the manifest, other than its version, or the installer changed,
`update.json` gets `finish` and the explorer shows `! finish`. The panes then restart on the new code (see
[Launcher and self-restart](#launcher-and-self-restart)).

## Refresh and watchers

| Pane            | What triggers a refresh                                                                     |
| --------------- | ------------------------------------------------------------------------------------------- |
| Explorer        | `fs.watch` on the root, recursive unless the root is `$HOME`, a parent of it or the filesystem root (then top level only, or when the recursive watch fails). Changes inside `.git` or in collapsed ignored folders are skipped |
| Explorer        | `fs.watch` on the git folder for `index` and `HEAD` (commit, stage, branch switch)          |
| Explorer        | a 30 s safety rescan. A `.git` appearing later (`git init`, clone) is detected              |
| Explorer        | `settings.json`, `keys.json` and `update.json`, polled every second                         |
| Explorer        | the folder of the neighbouring terminal, polled every 2 s when **Follow** is on             |
| Preview         | the file's folder, the tab's request file every 200 ms, `settings.json` every second (Images) |
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
  `EXPLORER_SEEN`, `EXPLORER_MARKS`, `EXPLORER_VIEW` and `EXPLORER_TABS` for the preview), then exits with code **75**. The launcher starts the script again
  with that environment, in the same terminal, so the pane keeps its id, place and width.
- Any other exit code ends the launcher, and the pane with it.

A process cannot replace itself on Windows, and one that only waited for its successor would keep its memory,
one more at each update. The launcher loads nothing else, so it stays on the code it started with.

The search, source control, changes and editor panes are short-lived and have no launcher. A pane
started by a version without the launcher (0.4.0 and earlier) is the exception, see
[Limitations](troubleshooting.md#limitations).
