# Limitations and troubleshooting

## Limits

| Where             | Limit                                                                                         |
| ----------------- | --------------------------------------------------------------------------------------------- |
| Explorer filter   | reads at most 20,000 entries. Git-ignored folders are not searched                            |
| Explorer refresh  | at most one refresh every 250 ms, plus a full rescan every 30 s                               |
| Explorer watch    | recursive, except when the root is `$HOME`, one of its parents or the filesystem root (top level only, and the 30 s rescan covers the rest) |
| Preview           | reads the first 8 MB of a file (*start only* in the header); images up to 50 MB              |
| Preview           | syntax highlighting up to 1 MB. Lines longer than 2,000 characters stay plain                 |
| Preview           | binary when a NUL byte is in the first 8,000 bytes. Hex dump of the first 64 KB               |
| Preview Markdown  | rendered at most 120 columns wide. Paragraphs over 20,000 characters lose inline styles       |
| Preview images    | 50 MB per file, 50 million pixels, 256 MB of decompressed PNG data                            |
| Preview editing   | UTF-8 text files up to 2 MB, without NUL bytes                                                |
| Go to file        | 1,000 results shown. Without git (or with **hidden**), the walk stops at 50,000 files         |
| Search in files   | files over 2 MB are skipped, as are binary files (NUL in the first 8 KB)                      |
| Search in files   | 5,000 matches in all. Lines over 1,000 characters are cut around the first match |
| Search popup      | 30 file previews kept in memory                                                               |
| Changes pane      | 200 untracked files shown in full, then a count. 200,000 diff rows, then *diff too long, truncated* |
| Source control    | History: last 500 commits. Comparison: 300 commits per side. Fetch, pull and push time out after 120 s |
| Claude message    | 60,000 characters of diff sent (then *[diff truncated]*), at most 20 untracked files, 90 s timeout, 200-character message |
| Update            | check once a day, 3 s timeout. `bin/.fetch.lock` taken over after 2 min |
| Program output    | 64 MB read from any git command                                                               |

## Limitations

- **Narrow windows**: herdr enforces a minimum of 10 % of the width per pane, so the explorer is then wider than
  requested; this is also why it closes instead of collapsing into a narrow strip.
- **Opening a tab**: the split shows half-and-half for a few tens of milliseconds before taking its width, since
  `herdr plugin pane open` cannot set the size at creation.
- **Several panes**: in a tab with several panes, `prefix+f` places the explorer next to the top-left (or top-right)
  pane, which may not span the full height.
- **After a herdr restart**, only the explorer and the preview come back. An explorer with nothing saved opens at
  the tab terminal's folder; a preview whose file is gone stays closed.
- **Remotes**: source control cannot fetch, pull or push with a remote that is a local folder or uses a remote helper
  (`hg::`, `codecommit::`…), see [Security](../SECURITY.md#network-fetch-pull-push): use the terminal for those. It
  cannot ask for a password either: use an SSH agent or a credential helper. A partial clone cannot fetch its
  missing objects from the explorer or source control.
- **Submodules**: their changes are not shown, and switching branch, pulling or discarding does not update them (run
  `git submodule update` in the terminal), since their own config could run commands.
- **Conflicts** show as a diff with their markers, not a 3-way merge.
- **Search** matches line by line: no multi-line regex.
- **Preview**: highlighting is approximate (a tokenizer, not a parser). Progressive, lossless, hierarchical or
  arithmetic-coded JPEG, WebP and SVG images show their size only; GIF and APNG show their first frame; image quality
  is limited to two pixels per cell. Mermaid diagrams are limited to flowcharts (`flowchart`, `graph`), drawn as
  boxes in their order of appearance, without subgraphs or styles. A selection is cleared when the file reloads, the
  pane is resized, or wrap, line numbers or the Markdown view are toggled.
- ***Insert into terminal*** quotes for PowerShell and POSIX shells, not for `cmd.exe`: there, a name containing
  `&`, `|`, `<`, `>`, `^` or `%` could still run a command once `Enter` is pressed.
- ***Open in app*** opens HTML and SVG files in the browser, and Office documents in Office, which asks before
  enabling macros.
- **Updates**: the explorer installs none itself. It copies `herdr plugin update gcaudan.explorer`; a linked git
  clone is yours to update (see [Updating](installation.md#updating)).
- **Platforms**: release binaries exist for Linux and macOS on x86_64 and arm64, and Windows on x86_64 (arm64 runs
  it in emulation). Elsewhere (32-bit, armv7, BSD…), build the binary with `scripts/dev-build.sh`, which needs Rust.
- **Old panes**: an explorer or preview started by a version without the launcher (0.4.0 and earlier) keeps its old
  waiting `node` processes after updating, until it is closed. Close and reopen it (`prefix+f` twice for the
  explorer) to start from the launcher. A pane opened by a version of 0.8.0 or earlier still runs
  `node src/<pane>.js` from the previous version's folder, which herdr removes once no server uses it: close and
  reopen it too.

## Common problems

- **Nothing opens at all, or a pane closes right after opening**: `bin/herdr-explorer` is missing and
  could not be downloaded: run `scripts/fetch-bin.sh` (or `scripts\fetch-bin.ps1`) in the plugin folder to see why (see [Installation](#installation)). The hooks also exit silently when they cannot find `herdr` in an absolute folder of `PATH` (set
  `HERDR_BIN_PATH` to its path).
- **No git colours, no source control**: `git` is not on `PATH`, or the folder is not inside a repository.
- **The explorer does not open in new tabs**: it opens only in a tab that has a single pane, and only when *Open
  automatically* is on. Press `prefix+f` to open it by hand.
- **`prefix+f`, `prefix+a` or `prefix+d` do nothing**: check the `[[keys.command]]` blocks in `config.toml` (a plugin
  index install does not add them: run `bin/herdr-explorer setup` in the plugin folder, see
  [Setup](installation.md#setup)), run `herdr server reload-config`, and check that `herdr plugin list` shows
  `gcaudan.explorer`.
- **Explorer panes show a shell after a herdr restart**: herdr did not run the startup hook, usually because the
  plugin was linked before the hook existed. Run `herdr plugin link .` in the plugin folder, then restart herdr
  (`herdr server stop`, then `herdr`). `herdr plugin log list --plugin gcaudan.explorer` shows whether the hook
  ran, and its errors.
- **Icons show as boxes or question marks**: the terminal font is not a Nerd Font (v3). Install one, or set
  **Icon set** to emoji, Unicode or ASCII in the settings (see [Without a Nerd
  Font](configuration.md#without-a-nerd-font)). Emoji that overlap the names: the terminal draws them one column
  wide, choose Unicode or ASCII.
- **Images are blocky**: the terminal lacks the Kitty graphics protocol, or did not answer its query in time (see
  [Sharp images](usage/preview.md#sharp-images)). For a terminal that has the protocol but does not answer, set **Images** to
  *always sharp* in the settings.
- **The explorer does not refresh**: press `r`. With your home folder (or above it) as the root, or when a folder
  of the tree cannot be read, deeper changes show within 30 s (see [Limits](#limits)).
- **Copy says `Clipboard unavailable` or `(OSC 52)`**: on Linux, install `wl-copy` (Wayland) or `xclip`. Without a
  clipboard tool (over SSH, for example), the plugin asks the outer terminal to set its clipboard with OSC 52, which
  works only if herdr and your terminal pass it on.
- **VS Code or another graphical editor opens behind the terminal** (KDE Wayland): KWin's focus stealing prevention
  keeps a window started from a terminal in the background, as with `code <file>` typed in Konsole. Lower *Focus
  stealing prevention* in the KWin window behavior settings, or switch to the window by hand.
- **No `↑` although a release is out**: `updateCheck` is off, the check already ran today, the release was skipped
  (**Settings**: `u` updates anyway), or the clone is ahead of the release. Delete `update.json` to check again at the next tab creation.
- **Cannot rename or delete the project folder on Windows** (*access denied* or *in use*): the plugin's panes work
  in that folder, so that new spaces, tabs and panes made from them start there, and Windows keeps a folder in use
  from being renamed or deleted. Close the explorer (and the other plugin panes of the tab) first.
- **The explorer ignores `keys.json`**: a file that is not valid JSON is ignored. When the `"explorer"` entry of
  `keys.json` (config folder) is a string, a number or `true` instead of an object, the explorer uses the default keys
  and shows `Cannot use keys.json: …`. Fix the file, or delete it to go back to the default keys.
- **A setting edited by hand has no effect**: some are read only when a pane starts (see
  [Settings](configuration.md#settings)): close and reopen the explorer.

## Messages

### Installation

| Message                                             | Meaning and fix                                                                     |
| --------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `setup: herdr's config folder not found`            | (Windows) neither `HERDR_CONFIG_PATH` nor `APPDATA` is set: set one, or add the [key bindings](installation.md#key-bindings) by hand |
| `<config> sets keys.command inline: not changed; add these lines to it:`, `<config> cannot be read (…): not changed; …` | setup left the config alone: copy the printed `[[keys.command]]` blocks into it |
| `key prefix+f already bound in <config>, skipped`   | another command of the config uses that key: change the `key` line of the block, see [Key bindings](installation.md#key-bindings) |
| `skill <path> exists and is not a link: left alone` | `~/.claude/skills/herdr-preview` is a real folder. Remove or rename it, then run the script again |
| `fetch-bin: no release binary for <system> <machine>: build one with scripts/dev-build.sh (Rust needed)` | no release binary for this platform: see [Building the binary](installation.md#building-the-binary) |
| `fetch-bin: curl not found: install it, then run scripts/fetch-bin.sh again`, `fetch-bin: neither sha256sum nor shasum found` | install the tool, then run the script again |
| `fetch-bin: cannot download <url> (a release just tagged may still be building: …)` | the release's binaries are built a few minutes after its tag: retry then. Otherwise offline, or no release of that version (a clone of `main` between two releases, or a fork without releases): clone a release tag, or build the binary |
| `fetch-bin: <asset> is not in SHA256SUMS`, `fetch-bin: <asset> does not match its SHA256SUMS: not installed` | the download is incomplete or the release is broken: nothing was replaced. Run the script again later |
| `fetch-bin: cannot create <folder>/bin/.fetch.lock` | `bin/` cannot be written: fix its rights                                            |
| `fetch-bin: no version in <folder>/herdr-plugin.toml` | the manifest is missing or damaged: restore it with git                           |
| `dev-build: cargo not found: install Rust (https://rustup.rs)` | install Rust, then run `scripts/dev-build.sh` again                         |

### Explorer and preview

| Message                                          | Meaning and fix                                                                    |
| ------------------------------------------------ | ---------------------------------------------------------------------------------- |
| `Not a git repository`                           | changes (`]`, `[`), history and source control (`s`) need a repository. `prefix+d` opens source control anyway, and there `i` runs `git init` |
| `No changes`                                     | `]` or `[` found no changed file                                                   |
| `Deleted: Discard changes in the menu restores it` | the action needs the file on disk, and the row is a deleted file                 |
| `No trash: delete for good?`                     | no trash tool was found. Install `gio` (GLib) or `trash-put` (`trash-cli`) on Linux to keep deletions recoverable |
| `Link to a network share: delete the link?`      | (Delete) the entry is a link to `\\host\share`: it never goes to the trash, which would follow it; only the link is deleted, see [Security](../SECURITY.md#windows) |
| `No terminal next to the explorer`               | **Insert into terminal**, or sending review notes, found no terminal pane beside the explorer |
| `Not inserted: control character in name`, `Not typed: control character` | the text contains a control character, which could run a command: it is never typed into a terminal |
| `Cannot insert`, `Cannot type into the terminal` | `herdr pane send-text` failed                                                      |
| `Not opened: file type not allowed`              | **Open in app** accepts only documents, media and `txt md csv log json xml html htm` |
| `Cannot open`                                    | `xdg-open` / `open` / `explorer.exe` could not start                               |
| `File manager not found`                         | `xdg-open`, Finder (`osascript`, else `open -R`) or `explorer.exe` could not start. On Linux, install `xdg-open` |
| `<cmd> not found` / `<cmd> not found (an .exe is needed)` | fix the editor command in the settings. On Windows, give an `.exe` (VS Code's `code.cmd` is handled) |
| `VS Code not found`                              | the editor command is `code` but VS Code was not found                             |
| `No editor: set one in the settings`             | opening a folder needs an external editor. Set **Editor**                          |
| `No terminal editor found: set $EDITOR.`         | (Editor pane) no `nano`/`nvim`/`vim`/`vi` found (`edit`/`nvim`/`vim`/`nano` on Windows). Set `$EDITOR` or the editor command |
| `Cannot start <editor>: <code>`                  | (Editor pane) the editor could not start                                           |
| `Cannot start <cmd>: a batch file needs cmd`     | (Editor pane, Windows) a `.cmd` or `.bat` editor or source control tool is refused: give an `.exe` |
| `Cannot open the editor` / `the preview` / `the search` / `source control`, `Cannot show changes` | herdr refused to open the pane: check that it runs and is 0.9.4 or later |
| `Cannot preview`                                 | herdr did not report the pane's tab, or the request file could not be written     |
| `Invalid name`                                   | empty, absolute, containing a control character, or leaving the folder (`..`)     |
| `Already exists`                                 | choose another name                                                                |
| `Cannot create` / `rename` / `duplicate` / `delete: <code>` | file system error (`EACCES`, `EPERM`…)                                  |
| `N untracked files kept: no trash`               | **Discard changes** (or `D`, deleting for good declined) could not trash untracked files. Install a trash tool |
| `Cannot stage` / `Cannot unstage` / `Cannot discard` | the git command failed. Try in a terminal to see git's error                   |
| `Staged N lines; cannot stage <file>: …`, `Discarded N lines; cannot delete <file>: <code>` | part of the selection went through, the rest failed (git error, or file system error such as `EACCES`). Nothing is undone |
| `Cannot delete <file>: outside the repository` / `through a symbolic link` / `no longer a file` / `no longer a folder` / `a folder, never deleted for good` | (discard) the file changed since the diff was read: a folder on its path became a link (to outside the repository or elsewhere in it), or it became a folder. It is left alone; a folder is never deleted for good, only trashed |
| `Busy…`                                          | `s`, `u` or `D` pressed again before the last one ended                            |
| `<theme>: not found`                             | the icon theme was uninstalled. Choose another one                                 |
| `<key>: already bound`, `<key> taken from <action>` | (Key bindings) the key already had that action, or moved from another one |
| `No config folder`, `Cannot write keys.json`     | herdr gave no config folder, or `keys.json` cannot be written (fix or delete an unreadable file) |
| `Cannot write keys.json: <why>`, `keys.json not completed: <why>` | `keys.json` is not an object, or one of its entries is not: fix it by hand, the plugin leaves it alone |
| `Action failed`                                  | unexpected error in an action                                                      |
| `File deleted`, `Folder`, `Not a regular file`, `Cannot read: <code>` | (Preview) the file is gone, is a folder, a pipe or device, or unreadable |
| `Link to a network share: not opened` | (Preview) the file is a symbolic link (or sits behind one) to `\\host\share`, never followed, see [Security](../SECURITY.md#windows) |
| `No preview`, `<format> W×H (… no preview)`, `(too large for preview)`, `Unknown image format (…)`, `Empty file` | (Preview) image over a limit, in an unsupported format or encoding (for example `arithmetic, no preview`), or empty. `o` opens it |
| `install dwebp (libwebp) to see WebP images`     | (Preview) WebP needs `dwebp` on `PATH`                                             |
| `The terminal did not answer the Kitty graphics query: the image may not show` | (Preview) pixels were asked for in a terminal that did not answer. See [Sharp images](usage/preview.md#sharp-images) |
| `Find works in the file: d`                      | (Preview) `/` in the Diff view. `d` goes back to the file                          |
| `No default branch: compared with HEAD`          | (Diff view, Changes pane) no `origin/HEAD`, `main` or `master`, or no common commit with it |
| `No agent turn recorded in this repository`      | (Diff view, Changes pane) scope **Last agent turn**, but the tab's agent has not started working in this repository since the plugin was installed. `b` picks another scope |
| `The agent turn is still being recorded`         | (Diff view, Changes pane) the agent just started working and the snapshot of the working tree is not written yet: in a moment, `r` in the preview, or **Last agent turn** chosen again with `b` in the Changes pane |
| `Cannot edit this file here (e: in the terminal)` | (Preview) editing needs a text or Markdown file in the File view                  |
| `Cannot edit: over 2 MB` / `binary file` / `not UTF-8` / `<code>` | (Preview) see [Limits](#limits)                                   |
| `Cannot save: <code>`                            | (Preview) the file cannot be written (`EACCES`, `EROFS`…)                           |
| `Mixed line endings: saved as LF` / `CRLF`       | (Preview) the file mixes LF and CRLF: saving writes the more frequent one everywhere (LF on a tie) |
| `Notes need the explorer`, `Notes need herdr`    | (Preview, Changes pane) the pane does not know its herdr tab, whose terminal receives the notes |

### Updates

| Message                                     | Meaning and fix                                                           |
| ------------------------------------------- | ------------------------------------------------------------------------- |
| `Update command copied: run it in a terminal` | run `herdr plugin update gcaudan.explorer` in a terminal: the panes restart on the new version by themselves |
| `X.Y.Z skipped: Settings to update anyway`  | no `↑` for that version; `u` in the settings still opens the update       |
| `Key bindings added`                        | the setup offered on the first start ran (see [Setup](installation.md#setup)) |

### Search

| Message                       | Meaning                                                    |
| ----------------------------- | ---------------------------------------------------------- |
| `indexing…`                   | the file list is loading                                   |
| `(first 50000 files only)`    | the folder walk hit its cap. Narrow the root               |
| `Invalid regex: …`            | fix the regular expression, or turn **.\*** off            |
| `Invalid include glob`, `Invalid exclude glob` | fix the pattern in that field                  |
| `No results`                  | `alt+h` also searches ignored and dot files                 |
| `(file too large)`, `(binary file)`, `(not a file)`, `(cannot read)` | side preview not available for that file |

### Source control

| Message                                                            | Meaning and fix                                                     |
| ------------------------------------------------------------------ | ------------------------------------------------------------------- |
| `git not found in PATH`                                            | install git 2.31 or later, in an absolute folder of `PATH`: an older git counts as none |
| `<Operation> failed: <git error>`                                  | git's first error line. A rejected push shows the reason            |
| `Busy: <operation>`                                                | wait for the running command                                        |
| `timed out or not run`                                             | the command could not start, or hit its time limit                  |
| `Fetching` / `Pulling` / `Pushing` / `Publishing` / `Force refused: the repository sets <key>; use the terminal` | the repository's own config sets network settings (`http.*`, `url.*.insteadOf`, `remote.*.proxy`, `remote.*.vcs`, `core.alternateRefsCommand`). Check them, then run the command in a terminal |
| `No upstream branch to pull from`                                  | push first (publishes the branch), or set an upstream               |
| `<branch> and <upstream> have diverged (…): merge or rebase in the terminal` | pull only fast-forwards                                  |
| `Could not delete N files`                                         | discarding could not delete these files, or refused them: their folder leads outside the repository through a symbolic link |
| `Detached HEAD: switch to a branch to push`                        | check out a branch                                                  |
| `No remote: add one with git remote add`                           | add a remote                                                        |
| `Invalid remote name: …`                                           | a remote name starting with `-` is refused                          |
| `Type a commit message first, or ctrl+g to have one written`       | type a message                                                      |
| `Resolve the merge conflicts and stage the files first`            | fix the conflicts, then stage them                                  |
| `Merge conflict: edit the file and stage it, or abort the merge`   | conflicted files cannot be discarded                                |
| `Continue refused: the <op> would run <…>; check it and continue in the terminal` | the rebase todo has `exec` lines, or a non-builtin merge strategy is set |
| `Stopped on a new conflict: resolve it, stage it, then continue`   | normal during a rebase                                              |
| `git am in progress: continue or abort it in the terminal`         | `git am` is not handled by the pane                                 |
| `No merge, rebase, cherry-pick or revert in progress`              | nothing to abort                                                    |
| `<branch> is the base of the comparisons: select another branch`   | `B` on another branch changes the base                              |
| `<file> is deleted`                                                | `e`, `E` and `o` need the file on disk                              |
| `Claude not available: message from the file names`               | the `claude` CLI is missing or failed. The fallback message is used |
| `Branch name starting with "-": not switched`, `A branch name cannot start with "-"` | rename the branch in a terminal                  |
| `Remote branches are not deleted from here`, `Cannot delete the current branch` | use a terminal, or switch first                        |
| `Select a file first`                                              | `y`, `Y`, `e`, `E` or `o` on a row that is not a file               |
| `Nothing to stage`, `Nothing staged`, `Nothing to commit`, `No changes to describe`, `Already staged`, `Not staged` | informational |

### peek

`peek` prints `peek: <reason>` on stderr and exits with code 1 (2 for a wrong use).

| Message                                                                 | Meaning and fix                                                    |
| ----------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `herdr not found in PATH`                                               | `herdr` is not in an absolute folder of `PATH`                     |
| `cannot find the plugin's state folder: open a new tab once so that the explorer saves it` | the hook has not saved the state folder yet. Open a tab |
| `run it from a herdr pane` (code 2)                                     | `HERDR_PANE_ID` is not set                                          |
| `not in a git repository`, `git status failed`, `no changed file`       | `--last` outside a repository, or with no modified, added or untracked file |
| `no such file: <path>`, `not a file: <path>`                            | the path does not exist (allowed with `--diff`), or is a folder    |
| `not a local file: <link>`                                              | (`--url`) the link is not a `file://` URL of this host, or no link was clicked |
| `link to a network share, not shown: <path>`                            | the file is a link (or sits behind one) to `\\host\share`, never followed, see [Security](../SECURITY.md#windows) |
| `wrong place: <place>`, `usage: …` (code 2)                             | a place such as `14-12`, `0` or `12:18-5`, or wrong arguments       |
| `Cannot preview`, `Cannot open the preview`                             | see [Explorer and preview](#explorer-and-preview)                  |
