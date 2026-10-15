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
| `singleClick` | `false` | Single-click preview | a single click on a file shows it in the preview | at their next opening |
| `follow` | `false` | Follow terminal | reveal the folder of the terminal next to the explorer, within the tab's folder | within 2 s |
| `side` | `"left"` | Dock side | `"left"` or `"right"`: where new explorers dock; an open explorer keeps its side | new tabs only |
| `autoOpen` | `true` | Open automatically | open the explorer in every new tab that has a single pane; off: only with `prefix+f` | new tabs only |
| `icons` | `""` | Icon theme | `""` for the Nerd Font icons, or `{ "id", "label", "file" }` of a VS Code icon theme | explorers within a second; source control at its next opening |
| `wrap` | `false` | Wrap lines; preview `w`, Wrap chip | wrap long lines in the preview | other previews at their next opening |
| `lineNumbers` | `true` | Line numbers; preview `L` | line number gutter in the preview | other previews at their next opening |
| `diffLayout` | `""` | Diff layout; diff `s` (source control and diff pane) | `"inline"` or `"split"` (side by side); `""` chooses by width | other diffs at their next opening |
| `scmListShare` | `{}` | source control: drag the separator, `<` `>`, `shift+left` `shift+right` | share of the width taken by the list beside the diff, per view: `{ "changes": 0.3, "history": 0.6 }`; a view left out: 38 %, from 34 to 70 columns | other source control panes at their next opening |
| `scmGraphAll` | `true` | source control History `a` | show every local and remote branch in History, or only the current one | other source control panes at their next opening |
| `scmTree` | `false` | source control `t`, List/Tree button | show the Changes view as a tree of folders | other source control panes at their next opening |
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

When no terminal editor is configured, the Editor pane uses `$VISUAL`, `$EDITOR`, then `nano`,
`nvim`, `vim`, `vi` (`edit`, `nvim`, `vim`, `nano` on Windows).

### Icon themes

**Icon theme** lists the file icon themes installed for VS Code, VS Code Insiders,
VSCodium, Cursor, Windsurf or VS Code Server. It searches each editor's user `extensions` folder
(`~/.vscode/extensions`, `~/.cursor/extensions`…), the macOS application bundles, and the extensions
next to the `code`, `code-insiders`, `codium`, `cursor` or `windsurf` commands found on `PATH`. Each
icon is drawn in the colour of its SVG or font glyph. Popular icons, mostly from Material Icon Theme,
get a Nerd Font glyph of their own, and the others use the default glyph in the theme's colour. When
an extension update moves the theme, the plugin finds it again. When the theme is uninstalled, the
explorer goes back to the Nerd Font icons. While choosing, a sample tree under the list previews the
theme under the selection, and `Esc` goes back without a change.

### Width

The explorer is 32 columns wide by default (10 at least). A width set by dragging its border, or with
`<` `>` or `shift+left` `shift+right`, is kept for the next explorers, and restored when the window or another pane is resized. Delete the `width` file to go back
to 32 columns.

## Files

herdr gives each plugin a **state folder** (`$HERDR_PLUGIN_STATE_DIR`) and a **config folder**
(`$HERDR_PLUGIN_CONFIG_DIR`). Their location is chosen by herdr: the state folder is
`~/.local/state/herdr/plugins/gcaudan.explorer` on Linux and
`%LOCALAPPDATA%\herdr\plugins\gcaudan.explorer` on Windows, and
`herdr plugin config-dir gcaudan.explorer` prints the config folder. Without them, nothing is saved, and
the explorer's width falls back to 32 columns.

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

The plugin writes these files to a temporary file and then renames it, so a pane never reads a
half-written file. At herdr startup, the state files of panes that no longer exist are deleted.

### Config folder: `keys.json`

`keys.json` holds the explorer's key bindings. Change them from **Settings → Key bindings**, which
writes the actions you change to the file (see [usage.md](usage.md#settings--or-the-gear-icon)). Its
last row, **Open keys.json**, creates the file, or completes it, with every explorer action and its
current keys, then opens it in the Editor pane. The explorer reloads the file within a second of a save.

```json
{
  "explorer": {
    "newFile": ["a"],
    "delete": ["delete", "d"],
    "preview": "P"
  }
}
```

- One object per pane. Only `"explorer"` is read.
- Each action maps to a key name or a list of key names. An action you leave out keeps its default
  keys, and listing keys for an action replaces all its defaults. Unknown actions are ignored.
- A key you give to an action is taken away from the action that had it by default. A key listed for
  two actions goes to the last one.
- Only the explorer's keys can be changed: the preview, search and source control keys are fixed.
- In menus, the keys of `up`, `down`, `open`, `expand`, `close`, `collapse`, `menu` and `settings`
  apply. `escape` always closes a menu.
- While the filter field is active, only bound keys that are not single characters reach the list.

#### Actions and default keys

| Action             | Default keys            | Action             | Default keys            |
| ------------------ | ----------------------- | ------------------ | ----------------------- |
| `up`               | `up`, `k`               | `goToFile`         | `ctrl+p`                |
| `down`             | `down`, `j`             | `searchInFiles`    | `ctrl+f`                |
| `pageUp`           | `pageup`                | `sourceControl`    | `s`                     |
| `pageDown`         | `pagedown`              | `settings`         | `,`                     |
| `top`              | `home`, `g`             | `toggleHidden`     | `.`                     |
| `bottom`           | `end`, `G`              | `toggleIgnored`    | `i`                     |
| `open`             | `enter`, `space`        | `newFile`          | `n`                     |
| `expand`           | `right`, `l`            | `newFolder`        | `N`                     |
| `collapse`         | `left`, `h`             | `rename`           | `R`, `f2`               |
| `close`            | `escape`, `q`           | `delete`           | `delete`                |
| `menu`             | `m`, `menu`, `shift+f10`| `edit`             | `e`                     |
| `refresh`          | `r`                     | `openInEditor`     | `o`                     |
| `filter`           | `/`                     | `preview`          | `p`                     |
| `copyPath`         | `y`                     | `copyRelativePath` | `Y`                     |
| `parentRoot`       | `backspace`             | `help`             | `?`, `f1`               |
| `halfPageUp`       | `ctrl+u`                | `halfPageDown`     | `ctrl+d`                |
| `openInApp`        | `O`                     | `narrower`         | `>`, `shift+left`¹      |
| `wider`            | `<`, `shift+right`¹     |                    |                         |

¹ Docked on the right, `shift+left` and `shift+right` swap: each one pushes the inner border its way.

#### Key names

- A printable character is its own name: `a`, `N`, `/`, `,`, `É`. Letters are case-sensitive.
- `space`, `enter`, `tab`, `shift+tab`, `backspace`, `escape`, `insert`, `delete`, `menu`,
  `shift+f10`
- `up`, `down`, `left`, `right`, `home`, `end`, `pageup`, `pagedown`
- `ctrl+up`, `ctrl+down`, `ctrl+left`, `ctrl+right`, `ctrl+home`, `ctrl+end`, `shift+up`,
  `shift+down`, `alt+up`, `alt+down`, `alt+left`, `alt+right`
- `f1`, `f2`, `f3`, `f4`, `f5`
- `ctrl+a` … `ctrl+z` (note that `ctrl+i` is `tab`, `ctrl+m` is `enter` and `ctrl+h` is `backspace`
  in a terminal)
- `alt+<character>`, `alt+enter`, `alt+backspace`

### herdr config

The herdr key bindings that open the panes live in herdr's own `config.toml`, not in the plugin's
folders. See [installation.md](installation.md#what-the-install-script-does).
