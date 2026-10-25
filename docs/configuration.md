# Configuration

Change the settings from the explorer's settings (`,`, the gear icon or *Settings…* in its menu), which write them to
`settings.json`, shared by every pane of every tab. You can also edit the files below by hand.

## Settings

The pane where a setting changes applies it at once; the last column says when the other open panes do. The rows
follow the groups of the settings view: Appearance, Tree, Opening, Preview, Review, New tabs, Advanced (see
[usage.md](usage/explorer.md#settings--or-the-gear-icon)).

| Key | Default | Setting | Effect | Other open panes |
|---|---|---|---|---|
| `theme` | `"auto"` | Theme | `"dark"` (VS Code Dark+ colours), `"light"` (Light+), or `"auto"`: by the terminal's background, as last read (the `background` file), dark until a pane has read it | the explorer at once; a pane opened on auto turns at once when the terminal's background differs; other panes at their next opening |
| `glyphs` | `"nerd"` | Icon set | `"nerd"` (Nerd Font glyphs), `"emoji"`, `"unicode"` or `"ascii"`: the file icons, chevrons and header icons for a terminal without a Nerd Font, see [Without a Nerd Font](#without-a-nerd-font) | explorers within a second; other panes at their next opening |
| `icons` | `""` | Icon theme | `""` for the Nerd Font icons, or `{ "id", "label", "file" }` of a VS Code icon theme | explorers within a second; source control at its next opening |
| `hidden` | `true` | Hidden files; explorer `.` | show dot files in the tree | at their next opening |
| `ignored` | `true` | Git-ignored files; explorer `i` | show git-ignored files, greyed out | at their next opening |
| `compact` | `true` | Compact folders | a folder whose only entry is a folder shows on one row with it (`src/main/java`), not while filtering | at their next opening |
| `follow` | `true` | Follow terminal | reveal the folder of the terminal next to the explorer, moving the root when it leaves the tree | within 2 s |
| `open` | `"preview"` | Enter opens | what `Enter` and a double-click do on a text file: `"preview"` or `"editor"` | explorers within a second |
| `singleClick` | `false` | Preview follows selection | a file selected by a click or the arrow keys shows in the preview; the arrow keys only update a preview already open | explorers within a second |
| `editor` | `""` | Editor | external editor command, see below | at once (read at each use) |
| `wrap` | `false` | Wrap lines; `w`, **View** menu (`⚙`) | wrap long lines in the preview, its Diff view, the Changes pane and source control's diff | within 1 s in the open panes |
| `lineNumbers` | `true` | Line numbers; preview `L`, **View** menu | line number gutter in the preview | other previews at their next opening |
| `changes` | `true` | Changes; preview `c`, **View** menu | the lines added, changed or removed since HEAD, beside the lines of the preview's File view and on its scrollbar, see [Changes in the File view](usage/preview.md#changes-in-the-file-view) | other previews at their next opening |
| `images` | `"auto"` | Images; preview `p` (that pane only), the line under a pixelated image | `"blocks"` (pixelated), `"auto"` (sharp when the terminal has the Kitty graphics protocol) or `"pixels"` (always sharp), see [Sharp images](usage/preview.md#sharp-images) | previews within a second |
| `diffLayout` | `""` | Diff layout (auto, split, inline); `v` in every diff (source control, Changes pane, preview's Diff view), **View** menu (`⚙`) | `""`: inline under 120 columns (a preview beside the terminal), side by side from 120 (zoomed, full tab). `"inline"` or `"split"`, as the settings view sets it: every diff. `{ "narrow": …, "wide": … }`, as `v` and the View menu write it: one layout per size (under 120 columns, from 120), a size left out laid out by width; `v` on a single value keeps it for the other size. The settings view shows both sizes when they differ, and a choice there holds for both | other diffs at their next redraw |
| `reviewPrefix` | `""` | Notes prefix | text typed before the [review notes](usage/preview.md#review-notes) sent to the agent (`A`), such as an instruction | at once (read at each use) |
| `autoOpen` | `true` | Open automatically | open the explorer in every new tab that has a single pane; off: only with `prefix+f` | new tabs only |
| `side` | `"left"` | Dock side | `"left"` or `"right"`: where new explorers dock; an open explorer keeps its side | new tabs only |
| `updateCheck` | `true` | Check for updates | check GitHub once a day for a newer release | explorers within a second |
| `reviewShare` | `0.35` | source control: `<` `>`, dragging the border | share of the review's width taken by source control beside its preview, from 0.15 to 0.8 | the next reviews |
| `searchShare` | `0.35` | search: dragging the border | share of the search's tab taken by the search beside its preview, from 0.15 to 0.8 | the next searches |
| `scmListShare` | `{}` | source control: drag the separator, `<` `>`, `shift+left` `shift+right` | share of the width taken by the list beside the diff, per view: `{ "changes": 0.3, "history": 0.6 }`; a view left out: 38 %, from 34 to 70 columns | other source control panes at their next opening |
| `scmGraphAll` | `true` | source control History `a` | show every local and remote branch in History, or only the current one | other source control panes at their next opening |
| `scmTree` | `false` | source control `t`, **List · Tree** switch | show the Changes view as a tree of folders | other source control panes at their next opening |

A missing or unreadable `settings.json` means all defaults, and keys left out keep their default. `scmListShare`,
`scmGraphAll` and `scmTree` are not in the settings view: only the source control pane changes them.

### Editor command

The editor command is what `e` (**Open in editor**) runs, and `o` (**Open in app**) for the files it does not hand to
the system. It is split at spaces, with quotes kept together (`"subl -w"`, `"code --wait"`), and runs without a
shell, with the path as its last argument.

- **Empty** (default): VS Code when the `code` command is found (on Windows, the plugin also looks
  in `%LOCALAPPDATA%\Programs\Microsoft VS Code\bin`). Otherwise the terminal editor in the Editor
  pane, for files only.
- **A terminal editor**: `vi vim nvim nano micro hx helix kak emacs ne joe mcedit edit`. It runs in
  the Editor pane.
- **Anything else**: started in the background (`zed`, `subl -w`…). On Windows, it must be an
  `.exe`, since `.cmd` and `.bat` launchers cannot run without a shell. `code` is the exception: the
  plugin starts VS Code itself.

Without a terminal editor here, the Editor pane picks one itself (see [Editor pane](usage.md#editor-pane)).

### Icon themes

**Icon theme** lists the file icon themes installed for VS Code, VS Code Insiders, VSCodium, Cursor, Windsurf or
VS Code Server: in each editor's user `extensions` folder (`~/.vscode/extensions`, `~/.vscode-oss/extensions` for
VSCodium, `~/.cursor/extensions`, `~/.vscode-server/extensions`…), the
macOS application bundles, and next to the `code`, `code-insiders`, `codium`, `cursor` or `windsurf` commands on
`PATH`. Each icon takes the colour of its SVG or font glyph; popular icons, mostly from Material Icon Theme, get a
Nerd Font glyph of their own, the others the default glyph. A theme moved by an extension update is found again;
an uninstalled one falls back to the Nerd Font icons.

### Without a Nerd Font

**Icon set** draws the icons with characters any font has, each followed by a space, so the names stay aligned:

| Set | Folders | Files | Chevrons, settings, find, branch |
|---|---|---|---|
| Nerd Font (default) | Nerd Font glyphs | Nerd Font glyphs, or the icon theme's | Nerd Font glyphs |
| emoji | `📁` `📂` | an emoji per type (`📜` JavaScript, `🐍` Python, `📝` Markdown…), `📄` for the others | `▸` `▾` `⚙` `⌕` `⎇` |
| Unicode | `■` closed, `□` open | `●` for a known type, `○` for the others, in the type's colour | `▸` `▾` `⚙` `⌕` `⎇` |
| ASCII | `/` | `-`, in the type's colour | `>` `v` `=` `/` `*` |

With Unicode or ASCII marks, a VS Code icon theme still gives its colours; emoji have their own. The Changes pane
keeps the Nerd Font glyphs whatever the set.

### Width

The explorer is 32 columns wide by default (10 at least). A width set by hand is kept in the `width` file, for
the next explorers and across window resizes (see [Other behaviours](usage/explorer.md#other-behaviours)). Delete the file
to go back to 32 columns.

## Files

herdr gives each plugin a **state folder** (`$HERDR_PLUGIN_STATE_DIR`: `~/.local/state/herdr/plugins/gcaudan.explorer`
on Linux, `%LOCALAPPDATA%\herdr\plugins\gcaudan.explorer` on Windows) and a **config folder**
(`$HERDR_PLUGIN_CONFIG_DIR`, printed by `herdr plugin config-dir gcaudan.explorer`). Without them, nothing is
saved, and the explorer's width falls back to 32 columns.

### State folder

| File                   | Content                                                                                      |
| ---------------------- | -------------------------------------------------------------------------------------------- |
| `settings.json`        | the settings above                                                                           |
| `width`                | explorer width in columns (minimum 10, default 32)                                           |
| `background`           | `dark` or `light`: the terminal's background, as the last pane opened on the auto theme read it |
| `expanded.json`        | expanded folders, per tree root                                                              |
| `search.json`          | last Go to file query, search query, include/exclude globs, case/word/regex options          |
| `update.json`          | `checkedAt`, the repository checked (`repo`), the `latest` release found with its changes (`notes`) and page (`url`), the version skipped (`skip`), the last update done (`updated`: `{ from, to, t }`), and `finish` (an update the installer must finish) |
| `resizing`             | the pane resizing itself, for half a second: the panes it moves let it put them back         |
| `update.lock`          | present while an update runs; taken over after 15 minutes (a killed update)                  |
| `panes/<pane id>.json` | per pane: the explorer's tab folder (`home`) and tree root (`root`), the preview's `file`, `root`, `line` and tabs (`tabs`) |
| `preview-<tab id>.json`| the last file the explorer, the search or `peek.js` asked the tab's preview to show        |
| `notes-<tab id>.json`  | the tab's review notes, removed once sent, each with the text of its lines (`anchor`) and `stale` once they are gone |
| `scope-<tab id>.json`  | the tab's [diff scope](usage/changes.md#diff-scope), `{ kind, rev }`, absent for HEAD                |
| `turn-<tab id>.json`   | the working tree when the tab's agent last started working: `{ pane, top, tree, t, ended }`  |
| `search-<tab id>.json` | the search open for the tab: `{ pane }`, removed when it closes |
| `review-<tab id>.json` | the review open for the tab: `{ pane }`, its source control, removed when the review ends |
| `viewed-<tab id>.json` | the files marked viewed in the tab's Changes pane: `{ real path: hash of the content then }` |
| `index/`               | the copies of the git index the [Last agent turn](usage/changes.md#diff-scope) scope works on; those unused for a week are removed at startup |

`settings.json`, `expanded.json`, `width`, `update.json`, `search.json`, `resizing`, `preview-*.json`, `notes-*.json`, `review-*.json`, `search-*.json`, `scope-*.json`, `turn-*.json`, `viewed-*.json`,
`panes/*.json` and `keys.json` (in the config folder) are written to a temporary file and then renamed, so a pane never reads them half-written. When a tab or a space closes, and again at herdr
startup, the files of tabs and panes that no longer exist are deleted (`notes-*.json`, `preview-*.json`, `review-*.json`, `search-*.json`, `scope-*.json`, `turn-*.json`, `viewed-*.json`, `panes/*.json`),
except those written in the last 10 seconds, which a tab opened meanwhile may already use.

### Config folder

| File        | Content                                                                                           |
| ----------- | ------------------------------------------------------------------------------------------------- |
| `keys.json` | the explorer's key bindings, see below                                                            |
| `state-dir` | the state folder's path, written by the hook for `peek.js`, which runs outside the plugin and gets only the config folder from herdr |

`keys.json` is written to a temporary file and then renamed, like the state files.

### Config folder: `keys.json`

`keys.json` holds the explorer's key bindings. Change them from **Settings → Key bindings**, which
writes the actions you change to the file (see [usage.md](usage/explorer.md#settings--or-the-gear-icon)). Its
last row, **Open keys.json**, creates the file, or completes it with the default keys of the actions
it leaves out, then opens it in the Editor pane. The explorer reloads the file within a second of a save.

```json
{
  "explorer": {
    "newFile": ["a"],
    "delete": ["delete", "d"],
    "preview": "P"
  }
}
```

- One object per pane. Only `"explorer"` is read, with the action names of
  [Keys (defaults)](usage/explorer.md#keys-defaults).
- Each action maps to a key name or a list of key names. An action you leave out keeps its default
  keys, and listing keys for an action replaces all its defaults. A file that is not valid JSON is ignored (the
  defaults apply) and left alone for you to fix; an `"explorer"` entry that is a string, a number or `true` makes
  the explorer pane end at once. Unknown actions and keys that are not
  strings are ignored.
- A key you give to an action is taken away from the action that had it by default. A key listed for
  two actions goes to the last one.
- Only the explorer's keys can be changed: the preview, search and source control keys are fixed.
- In menus, `up` and `down` move, `open` and `expand` run the selected item, the key shown beside an item runs
  it, and the other keys of `close`, `collapse`, `menu` and `settings` close the menu. `escape` always closes a menu.
- In the settings and key bindings pages, the arrows, page keys, `home` and `end` always move and `escape` always
  goes back, so that a rebinding cannot lock you out.
- While the filter field is active, bound keys that are not single characters reach the list, except
  `backspace`, `delete`, `ctrl+u` and `ctrl+d`, which edit the field.

#### Key names

- A printable character is its own name: `a`, `N`, `/`, `,`, `É`. Letters are case-sensitive.
- `space`, `enter`, `tab`, `shift+tab`, `backspace`, `escape`, `insert`, `delete`, `menu`,
  `shift+f10`
- `up`, `down`, `left`, `right`, `home`, `end`, `pageup`, `pagedown`
- `ctrl+up`, `ctrl+down`, `ctrl+left`, `ctrl+right`, `ctrl+home`, `ctrl+end`, `shift+up`,
  `shift+down`, `shift+left`, `shift+right`, `alt+up`, `alt+down`, `alt+left`, `alt+right`
- `ctrl+pageup`, `ctrl+pagedown`, `shift+home`, `shift+end`, `shift+pageup`, `shift+pagedown`,
  `ctrl+shift+left`, `ctrl+shift+right`, `ctrl+shift+home`, `ctrl+shift+end`, `ctrl+delete`, and
  `ctrl+shift+z` where the terminal tells it from `ctrl+z`
- `f1`, `f2`, `f3`, `f4`, `f5`
- `ctrl+a` … `ctrl+z` (note that `ctrl+i` is `tab`, `ctrl+j` and `ctrl+m` are `enter` and `ctrl+h` is
  `backspace` in a terminal)
- `alt+<character>`, `alt+enter`, `alt+backspace`

### herdr config

The herdr key bindings that open the panes live in herdr's own `config.toml`, not in the plugin's
folders. See [installation.md](installation.md#key-bindings).

## Environment

| Variable | Effect |
|---|---|
| `PATH` | where `node`, `git`, `curl`, `code`, `claude`, the editors and the clipboard and trash tools are found (absolute entries only, except for `curl` in `scripts/fetch-bin.sh`) |
| `VISUAL`, `EDITOR` | the Editor pane's terminal editor, after one set in **Editor** |
| `GIT_SSH_COMMAND`, `GIT_SSH` | when set, or `core.sshCommand`, updates leave ssh alone; otherwise they run it in batch mode |
| `WAYLAND_DISPLAY`, `DISPLAY` | on Linux, the preview's paste reads the clipboard with `wl-paste` when `WAYLAND_DISPLAY` is set, else with `xclip` when `DISPLAY` is; with neither (over SSH), only the terminal's own paste reaches the pane |
| `HERDR_BIN_PATH` | the herdr binary, else `herdr` from `PATH` |
| `HERDR_PLUGIN_ID` | the plugin id, `gcaudan.explorer` when unset |
| `HERDR_PLUGIN_ROOT` | the plugin folder, for updates, when the binary does not run from its `bin/` (a build run from cargo's `target/`) |
| `XDG_STATE_HOME`, `LOCALAPPDATA` | where `peek.js` looks for the state folder when `state-dir` is missing (`LOCALAPPDATA` on Windows, also for VS Code) |
| `HERDR_CONFIG_PATH`, `XDG_CONFIG_HOME`, `APPDATA` | the herdr `config.toml` the install scripts add the key bindings to, see [Installation](installation.md) (`XDG_CONFIG_HOME`: `install.sh` only; `APPDATA`: `install.ps1` only) |

herdr sets `HERDR_PLUGIN_STATE_DIR`, `HERDR_PLUGIN_CONFIG_DIR`, `HERDR_PLUGIN_CONTEXT_JSON` and
`HERDR_PLUGIN_EVENT_JSON` for the plugin's own processes (and `HERDR_PLUGIN_CLICKED_URL` for a clicked link), and `HERDR_PANE_ID` in every pane; the plugin only reads them.

The `EXPLORER_*` variables only pass between the plugin's own panes (see
[Pane environment](architecture.md#pane-environment)).
