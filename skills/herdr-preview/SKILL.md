---
name: herdr-preview
description: Use inside herdr (HERDR_PANE_ID is set) when you point the user at code you are explaining, or at a file you just wrote or changed — shows it in the explorer's preview pane next to the terminal instead of pasting it.
---

# Show a file in the herdr preview

`peek.js` shows a file in the preview pane of the current herdr tab, scrolled to the first place you name and with
every place highlighted. It opens the preview when there is none and leaves the focus on the terminal, so the user
keeps typing to you.

## Steps

1. **Check the host**: `HERDR_PANE_ID` is set. Unset: skip this skill and answer as usual.
2. **Pick the view**:
   - the file itself, to explain existing code (the user asks where something is or what it does);
   - `--diff`, to show what you just changed.

   Show only files you want the user to look at: each call changes what they see.
3. **Run it**, with the narrowest places that match what you talk about (see [Places](#places)):

   ```sh
   dir=$(dirname "$(herdr plugin list --plugin gcaudan.explorer --json | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>console.log(JSON.parse(s).result.plugins[0].manifest_path))')")
   node "$dir/src/peek.js" src/app.js:12-14,42:5-18
   node "$dir/src/peek.js" --diff src/app.js:40-52
   ```

4. **Read the result**: `Preview: <file>[:<places>]` (`Preview (diff): …`) means done. Otherwise it prints
   `peek: <reason>` on stderr and exits 1 (2 for a wrong argument or place): tell the user in one line and go on,
   one attempt only.

To walk through several files, call it once per file as you reach it in your explanation: each call replaces the
preview's temporary tab, and tabs the user kept stay open.

## Places

The argument is `file` or `file:places`, the file relative to your working directory or absolute, the places
separated by commas.

| Place     | Highlights                                                              |
| --------- | ----------------------------------------------------------------------- |
| `42`      | line 42                                                                 |
| `12-14`   | lines 12 to 14                                                          |
| `42:5-18` | columns 5 to 18 of line 42, both included, counted in characters from 1 |

- Columns: count them on the line as it is in the file, indentation included, to highlight a name, a call or a
  condition rather than its whole line. `42:5` (a single column, as compilers write it) means line 42.
- Markdown shows as source: give source line numbers.
- `--diff` marks the rows of the places' lines (whole lines only), and also accepts a deleted file.
- `--last` takes the repository's changed file written last, in place of a file (`--diff --last`: its changes).
