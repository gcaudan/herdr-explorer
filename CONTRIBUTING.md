# Contributing

## Development loop

The plugin is plain Node.js with no dependencies, no build step and no `package.json`.

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
6. Run `node --test` before pushing. The tests in `test/` check that every script parses, the
   terminal width of text (emoji, flags, wide characters) and the relative links of the docs.
   `.github/workflows/ci.yml` runs them on every pull request, on Linux, macOS and Windows with
   Node 20 and 24.

Keep the conventions of the existing code:

- Every external program goes through an absolute path (`which`) with an argument list, never a shell.
- Every git call in a user repository uses `gitEnv()`. A command that writes uses `safeEnv()` in
  `scm.js`. See [SECURITY.md](SECURITY.md).
- Text from files, names or git goes through `ui.line()`/`clean()`/`cleanLine()` before reaching the
  terminal.
- Shared state files are written to a temporary file, then renamed.
- Panes stay usable when the state or config folder is missing.

## Project structure

| File                  | Role                                                                                         |
| --------------------- | -------------------------------------------------------------------------------------------- |
| `herdr-plugin.toml`   | manifest: id, version, min herdr version, startup hook, `tab.created` event, panes, actions  |
| `install.sh`, `install.ps1` | link the clone to herdr and add the three key bindings                                 |
| `src/hook.js`         | startup restore, `tab.created`, and the toggle, search and source control actions. Opens and places the explorer |
| `src/launch.js`       | launcher that keeps the explorer and preview panes alive across self-restarts (exit code 75) |
| `src/explorer.js`     | explorer pane: tree, git decorations, filter, menus, settings, width, follow, update UI      |
| `src/actions.js`      | file and pane actions: open, preview, edit, insert, copy, reveal, create, rename, duplicate, trash, stage, unstage, discard; opens the plugin's other panes |
| `src/preview.js`      | preview pane: text, Markdown, images, hex dump; find, selection, live reload, request file   |
| `src/highlight.js`    | table-driven syntax highlighter (VS Code Dark+ colours)                                      |
| `src/markdown.js`     | Markdown renderer for the terminal, including tables, alerts and Mermaid flowcharts          |
| `src/image.js`        | image header sniffing and pure-JS decoders (PNG, GIF, BMP, baseline JPEG, ICO), half-block rendering |
| `src/search.js`       | Go to file (fuzzy) and Search in files (text/regex, globs) popup                             |
| `src/scm.js`          | source control pane: changes, commit, branches, remotes, history, stashes, refs, Claude messages |
| `src/graph.js`        | lanes of the history's branch graph                                                          |
| `src/diff.js`         | diff parser and renderer (inline, side by side, word highlights), and the Changes pane       |
| `src/edit.js`         | Editor pane: runs the terminal editor on a file                                              |
| `src/icons.js`        | Nerd Font icons and VS Code icon theme loading                                               |
| `src/ui.js`           | terminal helpers: colours, text width, lines, input decoding, text field, scrollbar, screen, restart |
| `src/exec.js`         | safe program execution, git hardening, settings, pane state, key bindings                    |
| `src/update.js`       | daily release check, self-update by fast-forward pull, version watch                         |
| `hook.js`, `explorer.js`, … | one-line stand-ins for the panes and hooks of a 0.5.x manifest, until its installer is run again; to remove in a later release |
| `release-please-config.json`, `.release-please-manifest.json` | release automation                       |
| `test/`               | `node --test` suite: scripts parse, text width, key hints, graph lanes, doc links            |
| `.github/workflows/ci.yml` | runs the tests on every pull request and push to `main`                                 |
| `.github/workflows/release.yml` | runs release-please on every push to `main`, merges the release pull request and publishes the release |
| `CHANGELOG.md`        | generated by release-please                                                                  |

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

Other types (`chore`, `ci`, `refactor`…) are left out of the changelog.

## Release process

Releases are automated with [release-please](https://github.com/googleapis/release-please), run by
`.github/workflows/release.yml` on every push to `main`. Nothing is done by hand:

1. Merge pull requests into `main` with Conventional Commit titles, once CI passes.
2. When the merged commits include a type of the changelog table above (`feat`, `fix`, `perf`,
   `docs`, `revert`), release-please opens a release pull request titled `chore: release X.Y.Z`. It
   bumps `version` in `herdr-plugin.toml` (through `extra-files`) and in
   `.release-please-manifest.json`, and updates `CHANGELOG.md`.
3. The same run merges that pull request, tags `vX.Y.Z` (no component in the tag) and publishes the
   GitHub release. It then checks that the tag, the `version` in `herdr-plugin.toml`, the manifest
   and the latest release agree, and that the version has only digits and dots, as the update
   check requires.
4. Installed explorers find the release within a day through the update check (`releases/latest`)
   and offer the update. Linked clones restart on the new code once pulled.

Every merged `feat`, `fix`, `perf`, `docs` or `revert` therefore ships a release. Group changes in one pull request, or merge
them together, when they should go out as one version. Merging the release pull request by hand
still works: the next run publishes the release.

Do not edit the version by hand. The update check and the panes' self-restart both depend on it.
