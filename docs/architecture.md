# How it works

The plugin is a set of Node.js scripts that use only Node's built-in modules. `herdr-plugin.toml`
declares them to herdr:

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
answers they parse. Inputs reach a pane through `--env` variables (`EXPLORER_ROOT`, `EXPLORER_FILE`,
`EXPLORER_LINE`, `EXPLORER_TAB`, `EXPLORER_PANE`, `EXPLORER_MODE`…).

## Placement

**Explorer** (`hook.js`). When a tab is created, the hook skips the tab unless `autoOpen` is on and
the tab has a single pane that is not already an explorer. `--toggle` closes the tab's explorer if
there is one and opens one otherwise.

1. The hook takes the top-left pane, or the top-right pane when docked on the right.
2. It opens the `tree` pane as a split to the right of that pane, without focus. The root is the
   workspace folder, or the pane's own folder.
3. It resizes the split right away to the saved width plus the 2-column border. The explorer then
   corrects any rounding itself, at 300 ms, 1 s and 2.5 s after starting, and again whenever the
   window size changes.
4. herdr only splits to the right or downwards, so when docked on the left, the hook swaps the
   explorer with the pane to move it left.
5. It gives the focus back to the terminal. If the tab was created in the background, the display
   briefly switches to it, then returns to the tab you were viewing.

**Preview** (`actions.preview`). The first file opens the `preview` pane as a split to the right of
the terminal next to the explorer, giving `explorer | terminal | preview`.

**Popups and full-tab panes**. The search opens as a popup over the active pane. Source control, the
changes pane and the editor open *zoomed* (full tab), targeting the explorer's tab.

The plugin finds "the terminal next to the explorer" by walking `herdr pane neighbor` up to 4 steps
on the docked side, then on the other side, and skips the plugin's own panes (labels Explorer,
Preview, Source Control, Changes, Editor).

**Right-click**. herdr keeps right-clicks for its own menu unless a pane asks for them; the explorer
does so at startup (`herdr pane input --right-click pane`).

**Closing**. When the tab's terminal exits and only the explorer is left, it closes too, and the tab
with it.

## Preview requests

Each tab has at most one preview, reused for every file. To show a file, the explorer or the search
checks whether the tab already has a pane labelled *Preview*:

- **yes**: it writes `{ file, root, line, t }` to `preview-<tab id>.json` in the state folder. The
  preview polls that file every 200 ms (`fs.watchFile`) and loads each new request, scrolling to
  the line.
- **no**: it opens a new preview pane with the file in its environment.

The preview watches the folder of the file it shows rather than the file itself, because editors and
agents often replace a file by renaming a new one over it. It reloads 80 ms after a change, keeping
the scroll position.

## Restore after a herdr restart

herdr restores plugin panes as plain shells, keeping only their label and folder. To recover what
they showed, each pane writes its state to `panes/<pane id>.json`:

- the explorer: the tab folder (`home`) and the current tree root (`root`);
- the preview: `file`, `root` and `line`.

At startup, `hook.js --restore`:

1. lists every pane of every workspace;
2. treats as stale the panes labelled as the plugin's whose foreground process is not `node`;
3. closes them;
4. in each tab that had an explorer, opens a new one, at the saved root when both saved folders
   still exist;
5. in each tab that had a preview, shows the saved file at its line, when the file still exists;
6. deletes the state files of panes that no longer exist.

Source control, changes and editor panes are closed and not reopened. herdr keeps pane ids across a
restart, which is what lets each pane find its saved state.

State files are written to a temporary file, then renamed, so another pane never reads them
half-written.

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
reports and bracketed paste.

## Launcher and self-restart

The explorer and preview panes run through a small launcher (`launch.js`). The pane's first process
stays as the launcher and runs the real script as a child process with an IPC channel.

- The launcher keeps the terminal in raw mode between children, so a `ctrl+c` typed during a restart
  cannot kill the pane, and forwards `SIGTERM`, `SIGHUP` and `SIGINT` to the child.
- If the launcher is killed, the child exits too (IPC `disconnect`).
- `update.watch` polls the `version` line of `herdr-plugin.toml` every 2 s. When it differs from the
  version the process started with, the pane waits 2 s, so that a pull has written every file. Then
  it checks every second until the pane is idle.
- To restart, the child releases the terminal and sends the launcher what the next process must show
  (`EXPLORER_TREE` for the explorer; `EXPLORER_FILE`, `EXPLORER_ROOT`, `EXPLORER_LINE` and
  `EXPLORER_SEEN` for the preview), then exits with code **75**. The launcher starts the script again
  with that environment, in the same terminal, so the pane keeps its id, place and width.
- Any other exit code ends the launcher, and the pane with it.

This design exists because a process cannot replace itself on Windows. A process that only waited for
its successor would keep its memory, one more process at each update. The launcher loads nothing
else, so it stays on the code it started with.

The search, source control, changes and editor panes are short-lived and have no launcher. A pane
started by a version without the launcher (0.4.0 and earlier) is the exception, see
[Limitations](troubleshooting.md#limitations).
