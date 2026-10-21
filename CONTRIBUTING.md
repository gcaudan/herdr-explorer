# Contributing

## Development loop

The plugin is plain Node.js (20 or later) with no dependencies, no build step and no `package.json`.

1. Clone the repository and link it to herdr:

   ```sh
   git clone https://github.com/gcaudan/herdr-explorer.git
   cd herdr-explorer
   ./install.sh            # Windows: powershell -ExecutionPolicy Bypass -File .\install.ps1
   ```

   herdr now runs the plugin from your clone (`herdr plugin link`).
2. Edit the code.
3. Reload what you changed:
   - **search, source control, changes, editor**: these panes start a fresh process each time they
     open. Close the pane and open it again.
   - **explorer and preview**: these panes keep running. Close and reopen them (`prefix+f` twice
     toggles the explorer), or open a new tab. They restart by themselves only when the `version` in
     `herdr-plugin.toml` changes.
   - **src/hook.js**: runs on each event or action, so the next one picks up your changes.
   - **herdr-plugin.toml** (hooks, panes, actions): herdr keeps its own copy of the manifest, so run
     `./install.sh` again, or `herdr plugin link .`.
4. A pane script can also run outside herdr for a quick check, for example
   `EXPLORER_FILE=README.md node src/diff.js` or `node src/search.js <folder>`. Features that need herdr
   (opening panes, the neighbouring terminal) then do nothing or report an error.
5. `herdr plugin log` shows the hooks' output and errors. `node src/hook.js --all`, run from a herdr
   pane, adds an explorer to the open tabs that have a single pane.
6. Run `node --test` before pushing (see [Tests](#tests)).

Keep the conventions of the existing code:

- CommonJS modules with `'use strict'`, two-space indentation and single quotes. Comments and user-facing text are
  in English; comments explain *why*.
- Use Node's built-in modules only, and keep every feature working on Linux, macOS and Windows: CI runs the tests on
  all three.
- Every external program runs with an argument list (`run()` in `src/core/exec.js`, `execFile`, `spawn`), never a
  shell; the one exception is an npm-installed `claude.cmd` on Windows. Resolve it with `which()`, which searches
  only the absolute folders of `PATH`, and give paths after `--` or as absolute paths, so that they never read as
  options.
- Every git call in a user repository uses `gitEnv()`. A command that writes uses `safeEnv()` in
  `src/scm/state.js`. See [SECURITY.md](SECURITY.md).
- Text from files, names or git goes through `ui.line()`/`clean()`/`cleanLine()` before reaching the
  terminal. Text typed into a terminal must hold no control character.
- Shared state files are written to a temporary file, then renamed.
- Panes stay usable when the state or config folder is missing.
- A new pane, hook or action is also declared in `herdr-plugin.toml`. A change to the manifest (other than its
  version) or to the install scripts makes users run the installer again after updating: the update only gives them
  the command.
- Update the docs in the same change as the code; `test/docs.test.js` checks their links.

## Project structure

The scripts herdr runs stay at the root of `src/`: the manifest names them, and a self-restart runs the same path
again. Modules they share go in `src/core/` (terminal, programs, actions, state) and `src/render/` (highlighting,
Markdown, images, graph). The explorer and source control panes keep their own modules in `src/explorer/` and
`src/scm/`: the entry file starts the pane and hands the modules a shared context; the pure ones (`tree.js`,
`parse.js`) run nothing when loaded, so that tests can require them.

| File                  | Role                                                                                         |
| --------------------- | -------------------------------------------------------------------------------------------- |
| `herdr-plugin.toml`   | manifest: id, version, min herdr version, startup hook, `tab.created` event, panes, actions  |
| `install.sh`, `install.ps1` | link the clone to herdr, add the three key bindings and, with Claude Code, link the skill |
| `src/hook.js`         | startup restore, `tab.created`, and the toggle, search and source control actions. Opens and places the explorer |
| `src/launch.js`       | launcher that keeps the explorer and preview panes alive across self-restarts (exit code 75) |
| `src/explorer.js`     | explorer pane: startup, watchers, refresh, width, follow, root, menu items, file actions, update UI, render |
| `src/explorer/tree.js` | folders read and cached, sorting, hidden and ignored entries, rows (expanded, filter, changed only), git status and decorations; pure |
| `src/explorer/render.js` | header, rows, fields and prompts, filter and changed-files lines, menu drawing           |
| `src/explorer/settings.js` | the settings view and its pages: icon themes, key bindings                         |
| `src/explorer/input.js` | keys, mouse, menu keys, filter, preview following the selection                         |
| `src/preview.js`      | preview pane: text, Markdown, images (Kitty graphics or half blocks), hex dump, Diff view; find, selection, review notes, live reload, request file |
| `src/search.js`       | Go to file (fuzzy) and Search in files (text/regex, globs) popup                             |
| `src/scm.js`          | source control pane: wires the modules of `src/scm/` and starts the screen                  |
| `src/scm/parse.js`    | git output parsers: status, log, refs, remotes, worktrees, stashes, comparisons, numstat; pure |
| `src/scm/context.js`  | settings, state `S`, the frame drawn, messages and questions, late-bound calls            |
| `src/scm/state.js`    | git calls (hardened), loaders, refresh, diffs and comparisons, view switches              |
| `src/scm/ops.js`      | git operations (stage, commit, push, merge, rebase, tag, cherry-pick, reset…) and the `m` menu |
| `src/scm/render.js`   | rows, header, tabs, commit box, status, help, diff and comparison, branch picker          |
| `src/scm/input.js`    | keys, mouse, selection                                                                     |
| `src/diff.js`         | diff collection (`collect`), parser and renderer (inline, side by side, word highlights), shared with the preview's Diff view; and the Changes pane: cursor, notes, base |
| `src/edit.js`         | Editor pane: runs the terminal editor on a file                                              |
| `src/peek.js`         | shows a file in the tab's preview from outside the plugin (an agent's pane) and the peek action |
| `skills/herdr-preview/` | Claude Code skill that calls `src/peek.js`                                                 |
| `src/core/actions.js` | file and pane actions: open, preview, edit, insert, copy, reveal, create, rename, duplicate, trash, stage, unstage, discard; opens the plugin's other panes |
| `src/core/buffer.js`  | the preview's editing: text buffer, cursor over wide characters, undo, line endings, find, changed lines, atomic save |
| `src/core/tabs.js`    | the preview's tab list: open (temporary or kept), close, next, the strip's layout          |
| `src/core/notes.js`   | review notes of a tab: stored in the state folder, typed into the agent's prompt             |
| `src/core/icons.js`   | Nerd Font, emoji, Unicode and ASCII icons, and VS Code icon theme loading                    |
| `src/core/ui.js`      | terminal helpers: colours, text width, lines, input decoding, text field, scrollbar, screen, restart |
| `src/core/exec.js`    | safe program execution, git hardening, links to network shares, settings, pane state, key bindings |
| `src/core/which.js`   | `which()`: programs found in the absolute folders of `PATH` only; loads before the state folder is known (`peek.js`) |
| `src/core/update.js`  | daily release check, self-update by fast-forward pull, version watch                         |
| `src/render/highlight.js` | table-driven syntax highlighter (VS Code Dark+ colours)                                      |
| `src/render/markdown.js` | Markdown renderer for the terminal, including tables, alerts and Mermaid flowcharts          |
| `src/render/image.js` | image header sniffing and pure-JS decoders (PNG, GIF, BMP, JPEG, TIFF, ICO), WebP through `dwebp`, scaling, half-block rendering |
| `src/render/graph.js` | lanes of the history's branch graph                                                          |
| `hook.js`, `explorer.js`, … | one-line stand-ins for the panes and hooks of a 0.5.x manifest, until its installer is run again; to remove in a later release |
| `release-please-config.json`, `.release-please-manifest.json` | release automation                       |
| `test/`               | `node --test` suite, see [Tests](#tests)                                                     |
| `.github/workflows/ci.yml` | runs the tests on every pull request and push to `main`                                 |
| `.github/workflows/release.yml` | runs release-please on every push to `main`, merges the release pull request and publishes the release |
| `CHANGELOG.md`        | generated by release-please                                                                  |

## Tests

```sh
node --test
```

The tests use `node:test` and `node:assert`, and need neither herdr nor a terminal. `.github/workflows/ci.yml` runs
them on every pull request and every push to `main`, on Linux, macOS and Windows with Node 20 and 24.

| File | Covers |
| --- | --- |
| `syntax.test.js` | every script of the root, `src/` and its folders passes `node --check` |
| `docs.test.js` | relative links and `#anchors` of `README.md`, `SECURITY.md`, `CONTRIBUTING.md` and `docs/*.md` |
| `ui.test.js` | terminal width of text (emoji, flags, wide characters) and the segment helpers |
| `editor.test.js` | in-place editing (`src/core/buffer.js`) |
| `explorer-tree.test.js` | the explorer's rows, their order and the filter (`src/explorer/tree.js`) |
| `graph.test.js` | the lanes of the history graph |
| `icons.test.js` | icon widths in each icon set |
| `markdown-images.test.js` | images of rendered Markdown in pixels |
| `preview-tabs.test.js` | the preview's tabs |
| `review.test.js` | review notes and the lines of a diff they point at |
| `scm-parse.test.js` | the git output parsers (`src/scm/parse.js`) |

`docs.test.js` computes anchors as GitHub does: the heading in lower case, punctuation other than `-` and `_` dropped,
spaces turned into dashes. When you rename a heading, update the links to it, including the one `src/preview.js`
opens from the preview itself (`docs/usage.md#sharp-images`).

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/). The release notes are built from
them:

| Type     | Changelog section |
| -------- | ----------------- |
| `feat`   | Features          |
| `fix`    | Fixes             |
| `perf`   | Performance       |
| `docs`   | Documentation     |
| `revert` | Reverts           |

Other types (`test`, `chore`, `ci`, `refactor`…) are left out of the changelog.

## Release process

Releases are automated with [release-please](https://github.com/googleapis/release-please), run by
`.github/workflows/release.yml` on every push to `main`. Nothing is done by hand:

1. Merge pull requests into `main` with Conventional Commit titles, once CI passes.
2. When the merged commits include a type of the changelog table above (`feat`, `fix`, `perf`,
   `docs`, `revert`), release-please opens a release pull request titled `chore: release X.Y.Z`. It
   bumps `version` in `herdr-plugin.toml` (through `extra-files`) and in
   `.release-please-manifest.json`, and updates `CHANGELOG.md`.
3. The same run merges that pull request (squash), tags `vX.Y.Z` (no component in the tag) and publishes the
   GitHub release. It then checks that the tag, the `version` in `herdr-plugin.toml`, the manifest
   and the latest release agree, and that the version has only digits and dots, as the update
   check requires.
4. Installed explorers find the release within a day through the update check (`releases/latest`)
   and offer the update. Linked clones restart on the new code once pulled.

Every merged `feat`, `fix`, `perf`, `docs` or `revert` therefore ships a release. Group changes in one pull request, or merge
them together, when they should go out as one version. Merging the release pull request by hand
still works: the next run publishes the release.

Do not edit the version, `.release-please-manifest.json` or `CHANGELOG.md` by hand: release-please maintains them,
and the update check and the panes' self-restart both depend on the version.
