---
name: herdr-preview
description: Show a file, at a line and with parts highlighted, or its git changes, in the herdr explorer's preview pane next to the terminal. Use it inside herdr (HERDR_PANE_ID is set) to point the user at code you are talking about, or at a file you just wrote or changed, instead of pasting it.
---

# Show a file in the herdr preview

The [herdr-explorer](https://github.com/gcaudan/herdr-explorer) plugin has a preview pane in each herdr tab. This
command shows a file there, scrolled to a place and with the places you name highlighted, opening the preview next to
your terminal when there is none. It does not take the focus, so the user keeps typing to you.

Only inside herdr: check that `HERDR_PANE_ID` is set, else skip this skill.

```sh
dir=$(dirname "$(herdr plugin list --plugin gcaudan.explorer --json | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).result.plugins[0].manifest_path))')")
node "$dir/shims/peek.js" src/app.js:42
```

The argument is `file`, or `file:places`, the file relative to your working directory or absolute. Places are
separated by commas, the preview scrolls to the first one and highlights them all:

| Place     | Highlights                                                            |
| --------- | --------------------------------------------------------------------- |
| `42`      | line 42                                                               |
| `12-14`   | lines 12 to 14                                                        |
| `42:5-18` | columns 5 to 18 of line 42, both included, counted in characters from 1 |

For example `src/app.js:12-14,30,42:5-18`. `42:5` (a single column, as compilers write it) is line 42. Count the
columns on the line as it is in the file, indentation included: highlight a name, a call or a condition rather than
its whole line when that is what you talk about.

- `--diff` before the file shows its git changes rather than the file, the rows of the places' lines marked (whole
  lines): `peek.js --diff src/app.js:40-52`. It also works on a deleted file. Use it after changing a file, to show
  what you changed; show the file itself to explain existing code.
- `--last` shows the changed file of the repository written last (`--diff --last`: its changes).
- It prints `Preview: <file>[:<places>]` (`Preview (diff): …` with `--diff`) on success; otherwise
  `peek: <reason>` on stderr, with exit code 1 (2 for a wrong argument or place). On failure, tell the user in one
  line and go on: do not retry.
- The user goes through the places with `n` and `N`; `escape` clears them. A Markdown file shows as source, so that
  the places match its lines.
- One file per call: each call replaces the preview's temporary tab (tabs the user kept stay open). To walk the user
  through several files, call it again for each one as you explain it.

When to use it:

- The user asks where something is, or what a function does: show it at its line while you answer.
- You have just written or changed a file and want the user to look at it.
- Not for every file you read for yourself: each call changes what the user sees.
