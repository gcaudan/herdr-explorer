# Changes (diff pane)

Opened by **View changes** on a folder (a file's changes show in the preview's [Diff view](preview.md#diff-view)). For a file or
folder, it compares tracked files with HEAD (before the first commit, with nothing), each file's staged changes
apart from its unstaged ones (two sections, the file's name followed by `staged` or `unstaged`), and shows
untracked files in full, up to 200, as unstaged. The
header shows the name and the added and removed line counts, then what the changes are compared with (the
[diff scope](#diff-scope); a click opens `b`'s menu) and `⚙` (or `,`), the **View** menu as in the preview's Diff view:
**Layout** (`v`, saved for the diffs of that width, see [`diffLayout`](../configuration.md#settings)), **Wrap lines** (`w`)
and **Compare with** (`b`), each changed in place; the footer, the keys and how far down the diff is. The diff is inline, or
side by side from 120 columns, with changed words highlighted.

A cursor, a blue bar in the first column, moves over the rows: `up` `k` / `down` `j` `enter`, `pageup` /
`pagedown` `space`, `ctrl+u` / `ctrl+d`, `home` `g` / `end` `G`, or a click on a row; the view follows it, and the
wheel scrolls 3 rows. `n`/`N` put it on the next or previous hunk. `shift+up` / `shift+down` (or `K` / `J`), or a
drag with the mouse, select the rows from the cursor's; `escape` keeps the cursor's alone.

| Key        | Action                                                                                         |
| ---------- | ---------------------------------------------------------------------------------------------- |
| `a`        | a [review note](preview.md#review-notes) on the cursor's line (a removed line: the line now in its place), on the whole hunk from its `@@` row, on the whole file from its name |
| `s` / `u`  | stage / unstage the rows selected: a line, a hunk from its `@@` row, a file from its name ([Staging parts](#staging-parts)) |
| `D`        | discard them from the working tree, once confirmed (`y`)                                       |
| `A` / `C` / `U` | send the tab's notes to the agent / copy them (paths relative to the repository) / remove the last note |
| `m`        | mark the cursor's file viewed: it shows folded, `✓ viewed` after its name, until its content changes (`m` again: no longer) |
| `]` / `[`  | next / previous file not viewed, its name at the top                                           |
| `y`        | copy the cursor's line as `path:line`, relative to the pane's folder                           |
| `b`        | what to compare with: the menu of the [diff scopes](#diff-scope)                               |
| `,`        | the View menu (or `⚙`): Layout, Wrap lines, Compare with                                        |
| `v`        | inline / side by side (kept for the next diffs)                                                |
| `w`        | wrap long lines, on each side in a side by side diff (the preview's **Wrap lines** setting)    |
| `left` `h` / `right` `l` | without wrap, scroll the lines 8 columns sideways, the line numbers staying         |
| `q` `escape` `ctrl+c` | close                                                                               |
| `z`        | zoom the pane (toggle); the zoom goes when the pane loses the focus                            |
| `ctrl+p` / `ctrl+f` | Go to file / Search in files                                                          |
| `?` `f1`   | all the keys                                                                                   |

Lines with a note have a yellow bar in the first column.

## Staging parts

Compared with HEAD (the default [diff scope](#diff-scope)), the Changes pane and the preview's Diff view stage,
unstage and discard parts of files, as `git add -p` does: `s` adds the lines selected of the Unstaged section to the
index, `u` takes those of the Staged section out of it, and `D` takes those of the Unstaged section out of the working
tree, once confirmed. A selection covering both sections only takes, for each key, the lines of its own. The
selection is, in the Changes pane, the cursor's row or the rows selected with `shift` (a file's name row: the whole
file; a hunk's `@@` row: the whole hunk); in the Diff view, the rows selected with the mouse, else the hunk `n` or
`N` went to (a blue bar on its `@@` row), else the hunk at the top (a file's name there: its first hunk, the whole
file when it has none). On a side by side row, the removed line
and the added one both count.

The plugin makes a patch of the lines chosen from the diff shown and hands it to `git apply` (`--cached` for the
index, `-R` to take changes back). Part of an untracked file is first added with `git add -N`; a whole untracked file
is added with `git add`, or sent to the trash by `D` (deleted for good, after a second question, without a trash). A file without lines (a binary file, a mode change) is taken
whole. The other scopes are read-only.


## Diff scope

What the Changes pane and the preview's Diff view compare the files with. `b` (or the header's scope, or **Compare
with** in the View menu) opens its menu; the scope chosen holds for every pane of the tab, until changed:

- **HEAD** (the default): the last commit, each file's staged changes apart from its unstaged ones;
- **Merge base with main**: the point where the branch left the default branch (the remote's `HEAD`, else the first of
  `main`, `master`, `origin/main`, `origin/master` that exists), so every change of the branch, committed or not;
- **Last agent turn**: the working tree as it was when the agent of the tab last started working, so only what it
  changed since. herdr tells the plugin when an agent's status changes; when it starts working after having been idle
  or done (or for another agent of the tab), the plugin records a snapshot of the repository's working tree, tracked
  and untracked files, without touching the index. Untracked files over 10 MB are left out of it, and show as new.
  Before any turn, it says so;
- **Commit or range…**: a commit (its own changes, against its parent), `a..b` or `a...b` (since `b` left `a`), a
  side left out being `HEAD` (`main..`, `..main`), typed in
  the footer (`enter` to apply, `escape` to cancel);
- **Branch…**: a branch, local or remote, compared with the working tree.

Source control's `=` sets it too ([Comparing branches](source-control.md#source-control)).
