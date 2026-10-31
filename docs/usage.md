# Usage

The plugin has six panes, the first five described on a page of their own:

| Pane            | Opens as                     | Opened by                                                     |
| --------------- | ---------------------------- | ------------------------------------------------------------- |
| [Explorer](usage/explorer.md) | split, left or right of tab  | new tab with a single pane (**Open automatically**), `prefix+f` |
| [Preview](usage/preview.md) | split, right of the terminal | `Enter`/double-click on a file, `p`, `d` (its diff), search results, [`peek`](#show-a-file-from-an-agent-or-a-script) |
| [Search](usage/search.md) | tab of its own               | `ctrl+p`, `ctrl+f`, `prefix+a`                                |
| [Source Control](usage/source-control.md) | tab of its own, the review   | `s`, `prefix+d`                                               |
| [Changes](usage/changes.md) | right of the terminal        | `d` or **View changes** on a folder                           |
| [Editor](#editor-pane) | tab of its own               | `E`, **Edit in terminal**, `e` with a terminal editor         |

Its four herdr actions can be bound to keys (see [Key bindings](installation.md#key-bindings)):
`gcaudan.explorer.toggle` opens the explorer, or closes it when it is open (`prefix+f`), `gcaudan.explorer.search` opens Go to file
(`prefix+a`), `gcaudan.explorer.scm` source control (`prefix+d`), and `gcaudan.explorer.peek` previews the changed
file written last (left unbound by [setup](installation.md#setup)).

The explorer's key help (`?`) lists the keys these actions are bound to, and shows the plugin's version and commit
at its foot.

Key names in these pages are those of [configuration.md](configuration.md#key-names). Only the explorer's
keys can be rebound.

## Show a file from an agent or a script

`bin/herdr-explorer peek` shows a file in the preview of the tab it runs in, scrolled to a line, opening the preview
next to the terminal when there is none, without taking the focus. Run it from a herdr pane, such as the terminal of
an agent:

```sh
manifest=$(herdr plugin list --plugin gcaudan.explorer --json | grep -o '"manifest_path":"[^"]*"' | cut -d'"' -f4 | sed -e 's/[\][\]/\\/g' -e 's/^[\][\]?[\]//')
dir="${manifest%?herdr-plugin.toml}"
"$dir/bin/herdr-explorer" peek src/app.js:42
```

- The argument is `file`, or `file:places`, relative to the current folder or absolute. Places are separated by
  commas: a line (`42`), lines (`12-14`) or columns of a line (`42:5-18`, counted in characters from 1, both included);
  `42:5`, as compilers write it, is line 42. A name that exists as given is taken whole, a colon included. The
  preview scrolls to the first place and highlights them all: whole lines on the line highlight's colour, columns on the selection's. `n` and `N` go from one to the next (when no
  find is open), `escape` clears them; a Markdown file shows as source, so that they match its lines.
- `--url` reads the link clicked in a terminal from `HERDR_PLUGIN_CLICKED_URL` (see **Links in the terminal** below).
- `--last` shows the changed file of the repository (modified, added or untracked) written last.
- `--diff` before the file (or before `--last`) shows its changes ([Diff view](usage/preview.md#diff-view)) rather than the file: the view goes to the
  first place, and the rows of the new version's lines the places cover carry a blue bar (whole lines only). A
  deleted file is accepted there.
- It prints `Preview: src/app.js:12-14,42:5-18`, or the reason on stderr after `peek: `, with exit code 1 (2 for a
  wrong argument or place, or when run outside a herdr pane, without `HERDR_PANE_ID`).
- `herdr plugin list` gives the plugin's folder (`manifest_path`) whether it was installed from the index or from a
  clone, so the command works in both cases.

**Peek action**: `gcaudan.explorer.peek` runs `peek --last` for the focused tab, in the folder of its focused
terminal: bind it to a key to see what an agent just wrote (see [Key bindings](installation.md#key-bindings)).

**Links in the terminal** (herdr 0.9.4 or later, with herdr's mouse capture on): `ctrl`+click on a `file://`
hyperlink (OSC 8, as `ls --hyperlink` or some agents print them) shows the file in the tab's preview, at the line of a
`#L12` or `#L12-L14` fragment. It runs `peek --url` through the `gcaudan.explorer.open-link` action. A link to
another host is refused. herdr makes links only of URLs, so a plain `src/app.js:12` in the text cannot be clicked.

**Claude Code skill**: [`skills/herdr-preview`](../skills/herdr-preview/SKILL.md) tells Claude Code when and how to
call it, to show you the code it talks about or a file it just wrote. [Setup](installation.md#setup) links it into
`~/.claude/skills`; or link it yourself:

```sh
ln -s "$dir/skills/herdr-preview" ~/.claude/skills/herdr-preview
```

## Editor pane

The Editor pane opens in a tab of its own, named after the tab it was opened from (`(Editor 2)`), and takes the focus.
It runs a terminal editor on the file and closes when the editor exits, handing back to that tab. The editor is
the settings' editor command when it is a terminal editor, else `$VISUAL`, else `$EDITOR`, else the
first of `nano`, `nvim`, `vim`, `vi` found (`edit`, `nvim`, `vim`, `nano` on Windows).

## Scrollbars

A list that does not fit shows a scrollbar in its last column: a rail `│`, a thumb `┃` sized to the part shown,
and `▴` `▾` buttons from 4 rows tall. The preview's File view tints it where the lines changed since HEAD are
([Changes in the File view](usage/preview.md#changes-in-the-file-view)).

- A press on the thumb grabs it: the list follows the mouse until the button is released, even outside the
  column. A press elsewhere on the rail moves the thumb under the mouse, then drags it the same way.
- A click on `▴` or `▾` scrolls one row; held, it repeats (after 400 ms, then every 50 ms) until the end of the
  list, or until the mouse leaves the button.
- The wheel still scrolls the list. The explorer's scrollbar ignores clicks while a menu is open, the preview's
  while a selection is being dragged, and source control's while the branch picker is open.

An empty list says why in grey, centred, with the keys that go on from there under it: *Empty folder*
(`n` new file, `N` folder), *No match* (`esc` clear), *No results* in the search, *No changes* in the
source control list (`ctrl+p` go to file, `2` history), *Not a git repository* (`i` git init here).

## Sharp images

How to get images in real pixels rather than half blocks is in [Preview → Sharp images](usage/preview.md#sharp-images).
