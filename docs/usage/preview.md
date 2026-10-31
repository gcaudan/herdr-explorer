# Preview

The preview, one per herdr tab, keeps its files in tabs, as VS Code's editors. A file opened by a single click,
by following the selection, with `]`/`[`, from the search or by an agent (`peek`) replaces the **temporary** tab,
its name in italics; `enter`, a double-click, the explorer's `p` or its menu's **Preview** open a **kept** tab, which
the next file does not replace. In the preview, `t`, a double-click on the tab, `i`, `e`, `E`, `o` or a review note keep the
temporary tab. Each tab keeps its scroll, line, File or Diff view, comparison, Markdown mode and marks; the tabs come
back after a herdr restart (those whose file still exists). Past 50 tabs, the one furthest from the tab opened
closes, unless it has unsaved edits: those tabs never close this way, the list growing past 50 when all the others
have them. It reloads by itself when the file changes on disk,
keeping its scroll position.

Header: with several tabs, the tabs (the current one bold on a lighter background, `×` to close each), then the
details when they fit; with one, the file path relative to the root, then details (lines, size, image format and size; in the Diff view, the
added and removed lines). On the right:

- in the Diff view, what the changes are compared with, as in the Changes pane: `HEAD`, `main base`, `last turn`,
  `abc123` or `vs feature` ([diff scope](changes.md#diff-scope); the rest shows in the content itself); a click opens `b`'s menu;
- the **File · Diff** switch, the active choice filled: a click on the other one switches ([Diff view](#diff-view));
- `⚙` (or `,`, as the explorer's settings), which opens the **View** menu of the view's options, as the explorer's settings show them: a choice
  in blue, or `● on` / `○ off`. **Markdown** (rendered or raw, `m`), **Wrap lines** (`w`), **Line numbers** (`L`)
  and **Changes** (`c`) for text, **Edit here** (`i`), **Image** (sharp or pixelated, `p`); in the Diff view, **Layout** (inline or split, `v`), **Wrap lines** (`w`)
  and **Compare with** (the [diff scope](changes.md#diff-scope), `b`). The key beside each does the same outside the menu. `left` / `right`,
  `enter`, `space` or a click changes the option in place, the menu staying open; `up` / `down` move, `escape`, `q`,
  `,` or a click elsewhere close it. The footer's actions are not repeated there, and the footer leaves out what the
  header's controls do (`d`, `,`): the key help (`?`) lists every key.

A narrow pane drops the reminder, then the switch, then the details; `⚙` stays. Tabs that do not fit are marked
`‹` / `›` (a click shows the next one that way), and the strip scrolls only when the current tab would leave it. The footer lists the main keys, which
a click runs, and the scroll percentage.

## Keys

| Key                          | Action                                                   |
| ---------------------------- | -------------------------------------------------------- |
| `escape`                     | clear the selection, then the marks, then the find, else close |
| `q` `ctrl+c`                 | close the find, else close                               |
| `up` `k` / `down` `j` `enter`| scroll one line                                          |
| `pageup` / `pagedown` `space` | scroll one page                                         |
| `ctrl+u` / `ctrl+d`          | scroll half a page                                       |
| `?` `f1`                     | all the keys                                             |
| `tab` / `shift+tab`, `ctrl+pagedown` / `ctrl+pageup` | next / previous tab              |
| `t`                          | keep the temporary tab                                   |
| `x`                          | close the tab (the last one: the preview)                |
| `home` `g` / `end` `G`       | top / bottom                                             |
| `left` `h` / `right` `l`     | scroll 8 columns sideways (when not wrapping), also in the Diff view |
| `/`                          | find in file                                             |
| `n` / `N`                    | next / previous match; without a find, next / previous mark of [`peek`](../usage.md#show-a-file-from-an-agent-or-a-script), else hunk in the Diff view |
| `d`                          | the file or its changes ([Diff view](#diff-view))        |
| `b` / `v`                    | Diff view: what to compare with ([diff scope](changes.md#diff-scope)) / inline or split (saved by width) |
| `s` / `u` / `D`              | Diff view, compared with HEAD: stage / unstage / discard the lines selected with the mouse, else the hunk `n` went to ([Staging parts](changes.md#staging-parts)) |
| `,`                          | the View menu of the view's options (or `⚙`)            |
| `w`                          | toggle line wrap (saved), also of the Diff view's lines  |
| `L`                          | toggle line numbers (saved)                              |
| `c`                          | toggle the [changes since HEAD](#changes-in-the-file-view) beside the lines and on the scrollbar (saved) |
| `]` / `[`                    | next / previous block of changes since HEAD, a third of the way down |
| `m`                          | Markdown: rendered / source                              |
| `p`                          | image: sharp / pixelated, in this pane only (see [Sharp images](#sharp-images)) |
| `r`                          | reload                                                   |
| `i`                          | edit here, in the preview (see [Editing in the preview](#editing-in-the-preview)) |
| `e`                          | open in the external editor                              |
| `E`                          | edit in the terminal editor                              |
| `o`                          | open in app (allowed types only), else in the editor     |
| `y` / `Y`                    | copy the absolute / relative path; `Y` adds the selected lines (`src/app.js:12-14`) |
| `H`                          | history of the file, in source control                   |
| `a`                          | take a review note, see [Review notes](#review-notes)    |
| `A` / `C` / `U`              | send the notes to the agent / copy them / remove the last note |
| `z`                          | zoom the pane (toggle), until the pane loses the focus   |
| `<` / `>`                    | 4 columns narrower / wider (hold to repeat)              |
| `shift+left` `shift+right`   | move the inner border 4 columns that way                 |
| `ctrl+p`                     | Go to file                                               |
| `ctrl+f`                     | Search in files                                          |

## Editing in the preview

`i` (or **Edit here** in the View menu) edits the tab's file in place, from the File view: a text file in UTF-8, up
to 2 MB (otherwise *Cannot edit: over 2 MB*, *binary file* or *not UTF-8*); Markdown edits as source. Lines do not wrap while editing, the terminal's
cursor shows as a bar, and the highlighting follows what you type.

| Key | Action |
|---|---|
| arrows, `home` / `end`, `pageup` / `pagedown`, `ctrl+left` / `ctrl+right`, `ctrl+home` / `ctrl+end` | move (by word, to the file's ends with `ctrl`); with `shift`, select |
| `enter` | new line, keeping the indentation |
| `tab` / `shift+tab` | indent / unindent, with the file's own indentation |
| `alt+backspace` / `ctrl+delete` | delete the word before / after the cursor |
| `ctrl+s` | save |
| `ctrl+z` / `ctrl+y` `ctrl+shift+z` | undo / redo (typed text grouped by word) |
| `ctrl+c` / `ctrl+x` / `ctrl+v` | copy / cut / paste, the whole line when nothing is selected; `ctrl+a` selects all |
| `ctrl+f` | find: `enter` or `down` next, `up` previous |
| `ctrl+r` | replace: `tab` switches field, `enter` replaces one, `alt+a` all (`ctrl+h` is backspace in terminals) |
| `ctrl+g` | go to a line |
| `escape` | clear the selection, then leave editing |
| `ctrl+p`, `ctrl+pagedown` / `ctrl+pageup` | Go to file, next / previous tab |
| `f1` | the editing keys |

A click places the cursor, a double or triple click selects a word or the line, a drag selects (scrolling at the
edges), the wheel scrolls; text pasted from the terminal goes in as typed.

- **Saving** keeps the file's line endings (LF or CRLF; mixed ones become the most frequent, which a message says),
  its last newline or its lack of one, its byte order mark and its mode; the file is written to a temporary file renamed over it.
- **Changes since opening** show in the gutter: a green `▎` for added lines, blue for modified ones, a red `▔`
  where lines were removed.
- **Unsaved changes**: leaving editing, closing the tab (its `×` shows `●`) or the preview asks Save, Don't save or
  Cancel. Other tabs keep working meanwhile. A pane closed by herdr itself loses them.
- **Changed on disk**: without edits of yours, the file reloads silently (which empties the undo history and the
  gutter); with edits, a bar offers `alt+r` to reload (losing them) or `alt+k` to keep yours, and saving over the
  newer file then asks first. The plugin waits for editing to end before restarting on an update.

## Review notes

Notes are remarks on lines of a file, gathered while reading, then typed together into the agent's prompt. They
belong to the tab: the preview and the [Changes pane](changes.md#changes-diff-pane) of a tab share them.

- `a` opens a field in the footer for a note on the selected lines (select them with the mouse), else on the line
  the preview was opened at (a search result), else on the whole file. `tab` (`shift+tab` backwards) gives it a
  kind, `fix`, `question`, `nit` or `verify` (or none again), shown before the text; `enter` saves it, `escape` cancels.
- Each note shows in yellow under the last line it covers, whose numbers turn yellow; notes on the whole file, and
  every note in rendered Markdown, show at the top.
- A note keeps the text of its lines, and follows them when the file changes: lines added or removed above move it
  (to the nearest place when the same lines are in several). Once its lines are changed or gone, it is **stale**: it
  shows dimmed at the top, `stale` before its text, and stays at its old line numbers until the lines come back. In a file over 4 MB, notes stay where they are.
- `A` types the tab's notes, sorted by file and line, into the terminal next to the explorer, on one line, without
  submitting it, then clears them: `Review notes: src/app.js:12-14 (fix): rename it; README.md: add an example`, a
  stale one marked `(stale)`. The **Notes prefix** setting goes before them (an instruction, such as `Fix these:`).
  The paths are relative to the terminal's folder. The footer shows `A send N notes` while there are some.
- `C` copies the tab's notes instead, one per line under `Review notes:`, the paths relative to the preview's folder,
  and keeps them.
- `U` removes the last note taken.

The notes live in the plugin's state folder until they are sent, so they survive closing the preview. Closing the
tab deletes them.

## Find

`/` opens a field in the footer. Matching ignores case unless the query has a capital letter, and typing jumps to
the first match from the current position. `enter` or `down` goes to the next match (`enter` also leaves the field
when there are matches), `up` to the previous one, and `escape` closes the find. The footer shows `current/total`.
Every match is highlighted, the current one brighter, also in wrapped lines and in rendered Markdown.

## Mouse

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

## Changes in the File view

In a git repository, the File view marks the lines changed since HEAD (staged or not) in the first column of the
line numbers, or in a column of its own when they are hidden (`L`) and the file has changes, with the edit mode's
signs: `▎` in green an added line, in blue a changed one, `▔` in red removed lines just above, `▁` at the end of the
file. An untracked file is added in full; an ignored file, or one outside a repository, shows nothing. The same
colours tint the scrollbar where the changes are, removed lines over changed ones over added ones: a click there
goes to them. `]` and `[` go to the next and previous block of changes, a third of the way down. They are read again
at each reload and when the pane gets the focus back (after a commit, for example); not in rendered Markdown (`m`
shows the source), nor while editing (`i` marks the changes since the file was opened). `c` or **Changes** in the
View menu hides them (saved, see [`changes`](../configuration.md#settings)).

## Diff view

`d` (or **File · Diff** in the header) shows the file's git changes in place of the file, as the Changes pane does:
compared with HEAD, its staged changes apart from its unstaged ones, an untracked file in full, a deleted one as removed lines. The
header shows the added and removed line counts and what they are compared with (a click opens `b`'s menu); the View
menu (`⚙`, `,`) chooses the **Layout**, inline or split (side by side, `v`), **Wrap lines** (`w`) and what to **Compare
with** (the [diff scope](changes.md#diff-scope), `b`). The layout is saved for every
diff, one for narrow diffs and one for wide ones: inline beside the terminal and split once zoomed (`z`), for
example. `n` and `N` jump
between hunks, `a` notes the lines of the rows selected with the mouse (or of the top row), and it reloads with the
file. Find (`/`) works in the file only.

The explorer opens it with `d` and **View changes** on a file, and among the changed files with `]`, `[` and the
selection; `peek --diff` too.

## Commit shown

Source control shows the commit, stash, tag or pull request selected in its review's preview, over the preview's
tabs and read only: its header, the files changed, then the diff. Only the keys that make sense there work: moving,
`n`/`N` between hunks, `v` layout, `w` wrap, `,` the View menu, `z` zoom, the width keys and the key help. `q` and
`escape` go back to the tab underneath, or close the preview when it has none (always in a review).

## What it shows

- **Text**: syntax highlighting for JS/TS, JSON, Python, shell, PowerShell, Rust, Go, C/C++, C#, Java,
  Kotlin, Swift, PHP, Ruby, Lua, SQL, HTML/XML, CSS, YAML, INI/TOML, Markdown, diff, Dockerfile and
  Makefile. Past 8 MB, only the start shows; past 1 MB, and on lines over 2000 characters, the text stays plain.
- **Markdown** (`md markdown mdx mkd`): headings, inline styles, lists, quotes, highlighted code
  blocks, GFM tables and task lists, alerts, and simple Mermaid flowcharts. Besides `[ ]` and `[x]`, a task can be
  `[?]` to check (a yellow `?`), `[-]` dropped (a grey `⊟`, its text struck through) or `[/]` and `[~]` in progress (a
  blue `◐`): statuses of herdr-explorer and Obsidian, which GitHub shows as typed.
- **Markdown images**: a local image of a paragraph (Markdown image syntax, or `<img src>`), relative to the Markdown file,
  is drawn under it, 60 columns and 20 rows at most: in pixels or with half blocks, like an image file (see
  [Sharp images](#sharp-images)). Links to the web are never fetched, nor images outside the project (outside the
  Markdown file's folder when the file itself is outside the project).
- **Images** (`png apng jpg jpeg jpe jfif gif bmp dib ico cur webp tif tiff svg`): PNG, GIF (first frame), BMP,
  JPEG (baseline and progressive), TIFF and ICO/CUR are decoded by the plugin, WebP by `dwebp` when it is installed
  (libwebp, see [Requirements](../installation.md#requirements)). When herdr can show images (the Kitty graphics
  protocol), the image is drawn in real pixels, fitted to the pane; otherwise with half blocks (see
  [Sharp images](#sharp-images)). For other formats, such as SVG or a lossless or
  arithmetic-coded JPEG, or over 50 MB or 50 megapixels, the preview shows the format and size only. TIFF: strips (not tiles), one
  plane, uncompressed, PackBits, LZW or Deflate.
- **Binary** (a NUL byte in the first 8000 bytes): a hex dump of the first 64 KB.

## Sharp images

Images are drawn either pixelated, with half blocks (two coloured squares per cell), which works in any terminal, or
sharp, in real pixels with the Kitty graphics protocol. By default, they are sharp when the terminal has the
protocol, which needs two things: herdr running in a terminal
that has it, such as [Kitty](https://sw.kovidgoyal.net/kitty/), [Ghostty](https://ghostty.org/),
[WezTerm](https://wezterm.org/) or Konsole, and herdr's `terminal.kitty_graphics` left on (its default). The
plugin cannot install a terminal: install one of them, then start herdr in it.

**Settings → Images** chooses for every preview:

| Value | Images |
|---|---|
| sharp if possible (`auto`, default) | in pixels when the terminal answers the protocol's query (awaited 500 ms at most), else half blocks |
| pixelated (`blocks`) | half blocks |
| always sharp (`pixels`) | in pixels without waiting for the answer, for a terminal that has the protocol but does not answer; nothing shows where it does not have it |

The setting tells whether this terminal answered. `p` switches the image shown between sharp and pixelated, in
this pane only, until the setting changes; made sharp in a terminal that did not answer, it warns that the image
may not show. Under a pixelated image, a line tells how to see it sharp: `p` and
**sharp by default** when the terminal has the protocol, else a link to this section. Images in rendered Markdown
follow the same setting, and `p` (or **Images** in the **View** menu) switches them all; scrolled partly out of the
pane, an image shows its visible part.
