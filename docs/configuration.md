# Configuration

Change the settings from the explorer's settings (`,` or the gear icon), which write them to
`settings.json`. You can also edit the files below by hand.

## Settings

The pane where a setting changes applies it at once; the last column says when the other open panes do.

| Key | Default | Setting | Effect | Other open panes |
|---|---|---|---|---|
| `hidden` | `true` | Hidden files | show dot files in the tree | at their next opening |
| `ignored` | `true` | Git-ignored files | show git-ignored files, greyed out | at their next opening |
| `open` | `"preview"` | Enter opens | what `Enter` and a double-click do on a text file: `"preview"` or `"editor"` | at their next opening |
| `editor` | `""` | Editor | external editor command, see below | at once (read at each use) |
| `singleClick` | `false` | Preview follows selection | a file selected by a click or the arrow keys shows in the preview; the arrow keys only update a preview already open | at their next opening |
| `follow` | `false` | Follow terminal | reveal the folder of the terminal next to the explorer, moving the root when it leaves the tree | within 2 s |
| `side` | `"left"` | Dock side | `"left"` or `"right"`: where new explorers dock; an open explorer keeps its side | new tabs only |
| `autoOpen` | `true` | Open automatically | open the explorer in every new tab that has a single pane; off: only with `prefix+f` | new tabs only |
| `icons` | `""` | Icon theme | `""` for the Nerd Font icons, or `{ "id", "label", "file" }` of a VS Code icon theme | explorers within a second; source control at its next opening |
| `wrap` | `false` | Wrap lines; preview `w`, **View** menu | wrap long lines in the preview | other previews at their next opening |
| `lineNumbers` | `true` | Line numbers; preview `L`, **View** menu | line number gutter in the preview | other previews at their next opening |
| `images` | `"auto"` | Images; preview `p` (that pane only), the line under a pixelated image | `"blocks"` (pixelated), `"auto"` (sharp when the terminal has the Kitty graphics protocol) or `"pixels"` (always sharp), see [Sharp images](usage.md#sharp-images) | previews within a second |
| `diffLayout` | `""` (unset) | Diff layout; `s` in every diff (source control, Changes pane, preview's Diff view), **Inline · Split** switch, **View** menu | `{ "narrow": …, "wide": … }`, each `"inline"` or `"split"` (side by side): the layout of diffs under 120 columns (a preview beside the terminal) and from 120 (zoomed, full tab); unset, inline when narrow and split when wide. A single `"inline"` or `"split"`, from older versions, holds for both until one is chosen | other diffs at their next opening |
| `scmListShare` | `{}` | source control: drag the separator, `<` `>`, `shift+left` `shift+right` | share of the width taken by the list beside the diff, per view: `{ "changes": 0.3, "history": 0.6 }`; a view left out: 38 %, from 34 to 70 columns | other source control panes at their next opening |
| `scmGraphAll` | `true` | source control History `a` | show every local and remote branch in History, or only the current one | other source control panes at their next opening |
| `scmTree` | `false` | source control `t`, **List · Tree** switch | show the Changes view as a tree of folders | other source control panes at their next opening |
| `updateCheck` | `true` | Check for updates | check GitHub once a day for a newer release | explorers within a second |

A missing or unreadable `settings.json` means all defaults, and keys left out keep their default.

### Editor command

The editor command is split at spaces, with quotes kept together (`"subl -w"`, `"code --wait"`). It
runs without a shell, with the path as its last argument.

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
VS Code Server: in each editor's user `extensions` folder (`~/.vscode/extensions`, `~/.cursor/extensions`…), the
macOS application bundles, and next to the `code`, `code-insiders`, `codium`, `cursor` or `windsurf` commands on
`PATH`. Each icon takes the colour of its SVG or font glyph; popular icons, mostly from Material Icon Theme, get a
Nerd Font glyph of their own, the others the default glyph. A theme moved by an extension update is found again;
an uninstalled one falls back to the Nerd Font icons.

### Width

The explorer is 32 columns wide by default (10 at least). A width set by hand is kept in the `width` file, for
the next explorers and across window resizes (see [Other behaviours](usage.md#other-behaviours)). Delete the file
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
| `expanded.json`        | expanded folders, per tree root                                                              |
| `search.json`          | last Go to file query, search query, include/exclude globs, case/word/regex options          |
| `update.json`          | `checkedAt`, `latest` release found, and `finish` (an update the installer must finish)      |
| `resizing`             | the pane resizing itself, for half a second: the panes it moves let it put them back         |
| `update.lock`          | present while an update runs                                                                 |
| `panes/<pane id>.json` | per pane: the explorer's tab folder (`home`) and tree root (`root`), the preview's `file`, `root` and `line` |
| `preview-<tab id>.json`| the last file the explorer or search asked the tab's preview to show                         |

`settings.json`, `expanded.json`, `update.json` and `panes/*.json` are written to a temporary file and
then renamed, so a pane never reads them half-written. At herdr startup, the state files of panes
that no longer exist are deleted.

### Config folder

| File        | Content                                                                                           |
| ----------- | ------------------------------------------------------------------------------------------------- |
| `keys.json` | the explorer's key bindings, see below                                                            |
| `state-dir` | the state folder's path, written by the hook for `peek.js`, which runs outside the plugin and gets only the config folder from herdr |

### Config folder: `keys.json`

`keys.json` holds the explorer's key bindings. Change them from **Settings → Key bindings**, which
writes the actions you change to the file (see [usage.md](usage.md#settings--or-the-gear-icon)). Its
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
  [Keys (defaults)](usage.md#keys-defaults).
- Each action maps to a key name or a list of key names. An action you leave out keeps its default
  keys, and listing keys for an action replaces all its defaults. Unknown actions and keys that are not
  strings are ignored.
- A key you give to an action is taken away from the action that had it by default. A key listed for
  two actions goes to the last one.
- Only the explorer's keys can be changed: the preview, search and source control keys are fixed.
- In menus, the keys of `up`, `down`, `open`, `expand`, `close`, `collapse`, `menu` and `settings`
  apply. `escape` always closes a menu.
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
| `PATH` | where `node`, `git`, `code`, `claude`, the editors and the clipboard and trash tools are found (absolute entries only) |
| `VISUAL`, `EDITOR` | the Editor pane's terminal editor, after one set in **Editor** |
| `GIT_SSH_COMMAND`, `GIT_SSH` | left alone by updates, which otherwise run ssh in batch mode |
| `HERDR_BIN_PATH` | the herdr binary, else `herdr` from `PATH` |

The `EXPLORER_*` variables only pass between the plugin's own panes (see
[Pane environment](architecture.md#pane-environment)).
