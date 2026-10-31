# Search

The search opens in a tab of its own, named after the tab it was opened from (`(Search 2)`), with a preview on the
right at full height that shows the selected result, at its line (in Search in files, the line found marked), or
what to do while there is none. `q` in that preview closes it for the rest of the search. Asked again for the same
tab, from it or from the search's own tab, the open search is focused. Esc closes it and its tab, back to the tab it
was opened from. ⏎ on a result opens it in the preview of that tab and selects it in its explorer, its folders
expanded, then closes the search. The search keeps 35 % of the width, which dragging the border changes and the next searches keep
(`searchShare` setting).

It has two tabs, **Go to file** (`ctrl+p`) and **Search in files** (`ctrl+f`). Queries, globs and the case,
word and regex options are saved for the next time, shown selected so that typing replaces them. **hidden** starts
off each time.

## Go to file

Fuzzy matching on paths, scored like fzf and VS Code, the best 1000 kept. Each result shows the file name, its folder on the right,
and the matched characters highlighted. `path:line` (or `path:line:column`) opens the file at that line. The file
list is git's tracked and untracked files that are not ignored. Outside a repository, when git lists no files, or
with **hidden** on, the plugin walks the folder instead (50000 files at most), without `.git`, and without
`node_modules` and dot folders unless **hidden** is on. Search in files uses the same file list.

## Search in files

Text search across the files, run 150 ms after the last key, results grouped by file, with each file's count on the right. Chips
toggle **Aa** (match case), **ab** (whole word) and **.\*** (regex), blue when on. The **in** (include)
and **not in** (exclude) fields take comma-separated globs:
`*` and `?` within a segment, `**` across segments, `{a,b}`, `[abc]`, `[!abc]`. A pattern without a leading
`./` or `/` matches at any depth, and a folder matches its contents; globs ignore case on Windows only. Files over 2 MB and binary files are skipped,
and the search stops at 5000 matches.

## Keys

| Key                         | Action                                                       |
| --------------------------- | ------------------------------------------------------------ |
| `escape` `ctrl+c`           | close                                                        |
| `f1`                        | all the keys (`?` is typed in the fields)                    |
| `ctrl+p` / `ctrl+f`         | switch tab                                                   |
| `alt+c` `alt+w` `alt+r`     | toggle match case, whole word, regex                         |
| `alt+h`                     | toggle **hidden**: include ignored and dot files             |
| `tab` / `shift+tab`         | Search in files: next / previous field (query, include, exclude) |
| `up` `ctrl+k` / `down` `ctrl+n` | move                                                     |
| `pageup` / `pagedown`       | move one page                                                |
| `ctrl+home` / `ctrl+end`    | first / last result                                          |
| `alt+left` / `alt+right`    | fold / unfold the file's results                             |
| `enter`                     | open in the preview at the line                              |
| `alt+enter` `ctrl+e`        | open in the editor                                           |
| `ctrl+o`                    | open in app (allowed types only), else in the editor         |
| `ctrl+t`                    | insert the path into the terminal                            |
| `ctrl+y`                    | copy the path, relative to the folder searched               |

Other keys edit the focused field (see [Prompts](explorer.md#prompts)). `ctrl+e` is there because Windows Terminal keeps
`alt+enter` for full screen; it no longer moves to the end of the field (`end` does). Opening a result (in the preview of the tab the search was opened
from) or inserting its path closes the search, back to that tab; on failure, the search stays and shows the message.

Go to file counts the files on the right of the field (`12 of 840`, or `840` with no query). In Search in files,
the rule above the results counts them (`37 results in 9 files`), and shows `indexing…`, `searching…` and the
messages.

## Mouse

Click a tab, a chip or a field. Click a result to select it, and double-click it to open it. In
Search in files, clicking a file's header row folds or unfolds it. The wheel scrolls 3 rows.
