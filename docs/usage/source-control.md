# Source control

A review of the tab: source control opens in a tab of its own, named after the tab reviewed (`(SC 2)`), for the
repository of the explorer's root (with `prefix+d` and no explorer: the focused terminal's folder). The tab reviewed
stays as it was; while the review is open, coming back to that tab (switching agent, for instance) leads to the
review, and `s` or `prefix+d` there focuses it. `q` ends the review and closes its tab, with the preview it holds, back to the tab reviewed (herdr adds the
review's tab at the end of the bar). Outside a repository, it shows *Not a git repository*, and `i` runs `git init` there after
confirmation. Opened with `s` in the tree, it selects the selected file in Changes and shows its diff (on a
folder, its first changed file); a file without changes leaves the selection at the top, with *No changes in …*
in the status line.

## Header and tabs

The header shows the repository name, the branch (click it for the branch picker), its state against the
upstream (*up to date*, `↑ahead ↓behind`, *not published*), then the ongoing operation (MERGING, REBASING,
CHERRY-PICKING, REVERTING, APPLYING, BISECTING). On the right, one main button shows what the branch needs:
**↑ Publish branch** without an upstream, **↓ Pull 3** when it is behind, **↑ Push 2** when it is ahead,
**⇅ ↓1 ↑2** when it has diverged (pulling then says to merge or rebase in the terminal), nothing when up to date;
its icon spins while it runs. Then come **⇣ Fetch** and **Actions ▾**, the repository's menu (`m`), with shorter
labels below 80 columns. A menu hangs under the button that opened it, which stays lit while it is open.

Tabs: **Changes**, **History**, **Stashes**, **Refs** (keys `1` to `4`), the active one underlined, Changes and
Stashes with their count. In Changes, a **List · Tree** switch, the active choice filled, changes the layout.

The review has its own preview, on the right at full height, which shows the selection as it moves: a file's
changes (Diff view); a commit, stash or tag, read only (its header, the files changed, then the diff, with `⚙` or `,`
for its **Layout**, `v`, and **Wrap lines**, `w`); on any other line, *Select a file, commit, stash or tag*. `Enter` on a file, commit, stash or tag focuses the preview. `q` in the
preview closes it, whatever it shows: source control then takes the whole tab until the next `Enter`, which opens it
again. Source control keeps 35 % of the width, which `<` and `>` change and the next reviews keep (`reviewShare`
setting).

## Commit box (Changes)

The commit box has the message field on its own line, then the buttons. The first says what it will
do: **✓ Commit 2 staged**, **✓ Commit all 6** with nothing staged, **✓ Amend**, **✓ Continue rebase**
or **✓ Commit merge**. Then come **✧ Claude** (write the message), **☐ Amend** and, during an operation,
**✕ Abort**. In a narrow list, the labels are shorter.

- With nothing staged, committing offers to stage everything first.
- Amend fills an empty field with the last commit's subject. An empty message keeps the last message.
- During a merge, an empty message uses the one git prepared.
- Continuing is refused when it would run `exec` lines or a merge strategy other than git's own (see
  [Security](../../SECURITY.md#continue)). A `git am` is continued or aborted in the terminal.
- During a rebase, **Continue** runs `git rebase --continue`. During a cherry-pick or revert, it continues when the
  message is empty, and otherwise commits with your message, then goes on with the next commit picked.
- **✧** sends the file list and the diff (staged, or else all changes; 60000 characters at most) to `claude -p`,
  run from the temporary folder without tools, sessions, MCP servers or project settings, and fills the field with
  a one-line Conventional Commits message. When Claude is missing, fails or takes over 90 s,
  the message is built from the file names (`add foo.js`, `update a.js and 2 other files`).

## Keys

The status line shows the keys of the moment, as many as fit, which a click runs; `?` opens the full-pane key
help of the view (see [Key help](explorer.md#key-help--or-f1)).

| Key                        | Action                                                          |
| -------------------------- | --------------------------------------------------------------- |
| `q` `escape` `ctrl+c`      | end the review (`q` and `escape` close a diff shown first)      |
| `?` `f1`                   | all the keys                                                    |
| `<` / `>`                  | narrower / wider source control beside the preview (kept)       |
| `shift+left` `shift+right` | move the separator 4 columns that way (kept)                    |
| `up` `k` / `down` `j`      | move (the preview on the right follows)                         |
| `pageup` / `pagedown`, `home` `g` / `end` `G` | move by page / to the ends               |
| `ctrl+u` / `ctrl+d`        | move half a page                                                |
| `left` `h` / `right` `l`   | fold / unfold a section or folder                               |
| `enter`                    | file, commit, stash, tag: focus the preview (opened again after a `q` there) · section/folder: fold · branch: checkout |
| `z`                        | zoom the pane (toggle); the zoom goes when the pane loses the focus |
| `tab`                      | focus the diff                                                  |
| `1`–`4`                    | switch view                                                     |
| `r`                        | refresh                                                         |
| `f` / `L` / `P`            | fetch / pull (fast-forward only) / push. The first push publishes the branch on `origin` (else the first remote) and sets its upstream |
| `ctrl+p` / `ctrl+f`        | Go to file / Search in files, in the repository                 |
| `b`                        | branch picker                                                   |
| `m` `menu` `shift+f10`     | the [menu](#git-operations) of the selected branch or commit and of the repository (also **Actions ▾** in the header) |
| `T`                        | create a tag on HEAD                                            |
| `c`                        | type the commit message (from any view: back to Changes)        |
| `ctrl+g`                   | write the message with Claude (same)                            |
| `alt+a`                    | toggle amend                                                    |

**Changes view**: `s`/`space` stage, `u` unstage, `a` stage all, `A` unstage all, `d` discard (after
confirmation), `t` list/tree (kept for the next panes), `X` abort the ongoing operation. These keys act on the
selected file, folder or section; the files staged or unstaged are highlighted for a moment where they land. In
*Changes*, `d` puts tracked files back to the index and, like the explorer's *Discard changes*, sends untracked
files to the trash, with the folders left empty (without a trash, they are deleted for good after a second
question); in *Staged Changes*, the index and the
file go back to the last commit, and files added since leave the index for the trash. Conflicts cannot be discarded. On a file, as
in the explorer and the preview, `y` / `Y` copy its absolute / relative path, `e` opens it in the editor, `E`
edits it in the terminal editor and `o` opens it in its app (else the editor).

**Stashes view**: `n` new stash, named on a row above the list (the message is optional): `enter`
stashes the untracked files too, `alt+enter` or `ctrl+o` leaves them. `a` apply, `o` pop, `d` drop.

**File history**: from the explorer (`H` or **File history**) or the preview (`H`), the History view shows only the
commits that changed the file, each linked to the previous one in the graph, and a commit's diff is limited to the
file. The file's path shows on the right of the tabs, with its former name when it was renamed (`← old.js`, or the
count of names): a click on it, or `x`, goes back to every file. Renames are followed: `git log --follow` finds the
names the file had on the current branch, and the history lists the commits of all of them, so a file that later
took one of those names shows there too.

**History view**: the branch graph, the branches and remote branches at the commit (in their lane's colour) and
its tags (yellow), then the subject. A branch and the remote ones of the same name at the same commit show as one,
marked with a cloud (`☁`, `^` with the ASCII icons); the current branch comes first, in bold, then the other
branches, the remote ones alone and the tags, 3 at most then `+N` (the commit's preview lists them all in its first
line). On a narrow pane the subject gives way first; on the right, the author (from 90 columns), the age and the short hash. Each branch
has its own lane and colour, linked to the commit it started from and to the merge that brought it back; its
commits are marked ┿ on its line (┯ for a branch tip, ┷ for the first commit), and the checked-out commit's
subject is bold. The current branch's line changes colour where it leaves the default branch (the remote's HEAD,
else `main` or `master`), so that its own commits stand apart even when it is a straight line ahead of it. It shows every local and remote branch, or with `a` only the current one (kept for the next
panes). The graph takes 24 columns at most: lanes further right are cut. On a commit, `t` creates a tag and `C`
cherry-picks it onto the current branch (refused when the branch already has it).

**Refs view**: local branches, remote branches, tags, remotes and worktrees; the empty ones are named
together on the last line. `enter` checks out a branch (a remote branch switches to its local branch, or
creates one tracking it). `D` deletes a local branch, and offers `-D` when it is not fully merged,
saying how many commits would be lost (those on no other branch). `M` merges the selected branch into the current
one, `R` rebases the current branch onto it.

**Comparing branches**: from 100 columns, the selected branch is compared on the right with the current branch:
commits ahead and behind, the commits only on each side (`=` marks a change also on the other side, as after a
cherry-pick), and the files it changed since it left the current branch. `=` opens the comparison at any width and focuses it; there, `up`/`down` select a commit,
`enter` shows it, and `q` or `escape` on that commit goes back to the comparison. `B` makes the selected branch the
base of the comparisons instead of the current branch (`B` again on it, or on the current branch, goes back). `=` also
sets the [diff scope](changes.md#diff-scope) of the tab to `base...branch`, so that the Changes pane and the preview's Diff view
show the branch's changes too; a branch merely selected leaves the scope alone.

## Git operations

`m` (or the menu key, `shift+f10`, the header's **Actions ▾**) opens a menu: first the actions of the selected branch (Refs)
or commit (History), then those of the repository, each with its key when it has one:

| Item | What it runs |
|---|---|
| Merge into / Rebase onto (`M` / `R` on a branch), Merge a branch… / Rebase onto a branch… | `git merge` / `git rebase` with the chosen branch, after a question; the menu items open the branch picker to choose it |
| Create tag… (`t` on a commit, `T` for HEAD) | a lightweight tag named in place (`enter`); `ctrl+o` asks for a message and makes an annotated tag |
| Cherry-pick (`C` on a commit) | `git cherry-pick`, after a question |
| Force push (with lease)… | `git push --force-with-lease`, never a plain `--force`; the question says how many commits of the upstream would be lost |
| Undo last commit… | `git reset --soft HEAD~1`: the changes stay staged and the message comes back in the commit box; the question warns when the commit is already pushed |
| Reset --hard… (to HEAD, or to the selected commit) | `git reset --hard`; the question counts the changed files and the commits lost (those on no other branch, tag or remote branch). Untracked files are kept |

The menu also has **Check out** and **Delete** on a branch, and **Fetch**, **Pull** and **Push**.

A merge, rebase or cherry-pick that stops on a conflict switches to Changes, where the commit box continues it and
`X` aborts it. They use the `ort` strategy (git 2.33 or later), whatever the repository's `branch.<name>.mergeOptions`.

**Fetch, pull, push** never prompt and stop after 2 minutes; a repository whose own config sets network keys
is refused (see [Messages](../troubleshooting.md#source-control)).

**While typing the message**: `enter` commits or continues, `escape`/`tab` leaves the field, `ctrl+g`
writes with Claude, and `alt+a` toggles amend.

**Diff focused**: `up` `k` / `down` `j` `enter`, `pageup` / `pagedown` `space`, `home` `g` / `end` `G`,
`ctrl+u` / `ctrl+d` scroll. `n`/`N` jump between hunks, `v` switches inline/side by side (side by side by default
from 120 columns; kept for the next diffs), `w` wraps long lines (the **Wrap lines** setting, shared with the preview
and the Changes pane), and without wrap `left` `h` / `right` `l` scroll them 8 columns sideways, the line numbers
staying. `,` or the `⚙` of the diff's title opens its **View** menu, **Layout** (`v`) and **Wrap lines** (`w`), each
changed in place as in the preview. `tab`, `q` or `escape` go back to the list (below 100 columns, `q` and `escape`
close the diff).

**Branch picker** (`b`): a menu that hangs under the header's branch (highlighted while it is open),
as wide as its branches; the picker of *Merge a branch…* and *Rebase onto a branch…* hangs where the menu
was. It starts with *New branch…*, then local and remote branches. `up`/`down`
(`k`/`j`), `pageup`/`pagedown`, `enter` switch, `n` new branch, `D` delete (not a remote or the
current branch), `q` closes. `/` or any other character starts the filter, where `k` `j` `n` `D` `q`
are typed; `escape` clears the filter, then closes. A new branch is named on the picker's first row,
from the filter or else the current branch's prefix (`feat/`): `enter` creates it and switches to it,
`alt+enter` or `ctrl+o` only creates it, `escape` cancels.

**Questions**: discarding, deleting a branch, aborting an operation, popping or dropping a stash, staging
everything to commit, `git init` and the [git operations](#git-operations) ask in a card over the pane, with the keys of the explorer's
[confirmations](explorer.md#confirmations).

## Mouse

On the selected row of Changes, buttons appear: **↶** discard and **+** stage on unstaged changes, **+** on
conflicts, **−** unstage on staged changes. Click the header and commit-box buttons, the tabs and the branch.
Click a section or folder to fold it, double-click a row to open it as `enter` does (a branch: checkout), and
click the diff to focus it. The wheel scrolls the list, the diff or the picker under the mouse.

The pane refreshes when the git folder changes, when it gets the focus, and every 5 s, which picks up
working-tree edits.
