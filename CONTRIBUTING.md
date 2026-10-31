# Contributing

## Development loop

The plugin is one Rust binary, `bin/herdr-explorer`, with no crate besides the standard library. Every command of
the manifest runs it.

1. Install Rust with [rustup](https://rustup.rs): `rust-toolchain.toml` pins the toolchain, its components and the
   5 release targets, and rustup installs them on the first build.
2. Clone the repository and link it to herdr:

   ```sh
   git clone https://github.com/gcaudan/herdr-explorer.git
   cd herdr-explorer
   herdr plugin link .
   ```

   herdr now runs the plugin from your clone. After the build below, `bin/herdr-explorer setup` adds the key
   bindings and links the skill (see [Setup](docs/installation.md#setup)).
3. Build your binary:

   ```sh
   scripts/dev-build.sh    # Windows: powershell -ExecutionPolicy Bypass -File .\scripts\dev-build.ps1
   ```

   It runs `cargo build --release` (cargo from `PATH`, else `$CARGO_HOME/bin`, by default `~/.cargo/bin`), copies
   the result to `bin/herdr-explorer` and writes `bin/.dev`:
   `scripts/fetch-bin.sh` (also run by herdr's build step and the launcher) then leaves your binary alone. Delete `bin/.dev`, then run
   `scripts/fetch-bin.sh`, to go back to the release binary.
4. Edit the code under `src/`, run `scripts/dev-build.sh` again, then reload what you changed:
   - **search, source control, changes, editor**: these panes start a fresh process each time they
     open. Close the pane and open it again.
   - **explorer and preview**: these panes keep running, and restart by themselves on your binary once
     idle: dev-build writes `bin/.dev` again after each build, as a new `version` in `herdr-plugin.toml`
     would. They keep their place, width and what they show.
   - **the hook**: runs on each event or action, so the next one picks up your binary.
   - **herdr-plugin.toml** (hooks, panes, actions, build step): herdr keeps its own copy of the manifest, so run
     `herdr plugin link .` again.
5. To keep the herdr you work in on another install of the plugin, try your build in a herdr of its own:
   `scripts/test-herdr.sh` builds, links this clone into a herdr with its own config (a copy of yours), plugins,
   sessions and state, under `~/.cache/herdr-explorer-test` (`HX_TEST_HOME`), and attaches to it. `--no-build`
   skips the build, `--stop` stops its server; other arguments go to herdr. A herdr that links this clone too runs the same `bin/`.
6. A pane can also run outside herdr for a quick check, for example
   `EXPLORER_FILE=README.md bin/herdr-explorer pane diff` or `bin/herdr-explorer pane search <folder>`. Features that
   need herdr (opening panes, the neighbouring terminal) then do nothing or report an error.
7. `herdr plugin log` shows the hooks' output and errors.
8. Run `scripts/check.sh` before pushing (see [Tests](#tests)).

Keep the conventions of the existing code:

- Standard library only, in `[dependencies]` as in `[dev-dependencies]`: what the code needs from a system (terminal,
  folder watching, Windows' console) is declared in `src/sys/`. Keep every feature working on Linux, macOS and
  Windows: CI checks all three.
- `cargo fmt` (`rustfmt.toml`) and `cargo clippy -- -D warnings`, for the host and the 5 release targets.
  Comments and user-facing text are in English; comments explain *why*.
- The port follows the JS of 0.8.0 (`src/*.js` before the rewrite), defects included: a difference on purpose is noted
  where it is made, with its reason.
- Every external program runs with an argument list (`core::exec`), never a shell; the one exception is an
  npm-installed `claude.cmd` on Windows. Resolve it with `core::which`, which searches only the absolute folders of
  `PATH`, and give paths after `--` or as absolute paths, so that they never read as options.
- Every git call in a user repository runs in git's hardened environment (`core::exec`). A command that writes uses
  the safe one of `scm::state`. See [SECURITY.md](SECURITY.md).
- Text from files, names or git goes through `ui::clean`/`clean_line` before reaching the terminal. Text typed into
  a terminal must hold no control character.
- A path from the tree, a search or Markdown goes through `remote_link` (`core::exec`) before it is read, followed
  or handed to a program: a link to a network share must never be opened (see [SECURITY.md](SECURITY.md#windows)).
- Shared state files are written atomically (`core::exec`): a temporary file, then renamed.
- Panes stay usable when the state or config folder is missing.
- In a menu, an arrow never runs an action: `left`/`right` only change an option that has a value.
- A new pane, hook or action is also declared in `herdr-plugin.toml`. A change to the manifest (other than its
  version) reaches a linked clone only after `herdr plugin link .` again: herdr keeps its own copy.
- Update the docs in the same change as the code; the tests check their links.

## Project structure

herdr runs `bin/herdr-explorer` with a subcommand, as the manifest names it: `pane <id>` for the panes, `hook` for
the events and actions, `peek`, and `setup` run by the user. The explorer and preview panes are their own launcher, which relaunches the binary on
a self-restart.
The crate is at the root (`Cargo.toml`, `src/`, `tests/`); each module's first lines say what it holds and which JS
file it ports. Its modules form four layers (entries, panes, shared, base) that `tests/layers.rs` checks: see [How it
works](docs/architecture.md#how-it-works).

| Path                  | Role                                                                                         |
| --------------------- | -------------------------------------------------------------------------------------------- |
| `herdr-plugin.toml`   | manifest: id, version, min herdr version, build step (fetch-bin), startup hook, events (`tab.created`, `tab.closed`, `workspace.closed`, `pane.agent_status_changed`, `tab.focused`), panes, actions, `file://` link handler |
| `scripts/fetch-bin.sh`, `.ps1` | download the release binary of the manifest's version, checked against `SHA256SUMS`, into `bin/` |
| `scripts/dev-build.sh`, `.ps1` | build the binary into `bin/`, marked as a development one (`bin/.dev`) |
| `scripts/test-herdr.sh` | run a herdr of its own on this clone, apart from the herdr in use |
| `scripts/check.sh`    | every check, see [Tests](#tests)                                                             |
| `src/cli.rs`, `main.rs` | the command line: subcommands, `--version`                                                 |
| `src/hook.rs`         | startup restore, herdr's events (new, closed and focused tabs, agent turns) and the toggle, search and source control actions. Opens and places the explorer |
| `src/launch.rs`       | launcher that keeps the explorer and preview panes alive across self-restarts (exit code 75) |
| `src/peek.rs`         | shows a file in the tab's preview from outside the plugin (an agent's pane) and the peek action |
| `src/explorer/`       | explorer pane: tree (pure), drawing, settings view, keys and mouse, update and setup dialogs |
| `src/preview/`        | preview pane: text, Markdown, images, hex dump, Diff view; find, selection, edit mode, review notes, live reload |
| `src/search/`         | Go to file (fuzzy) and Search in files (text/regex, globs)                                   |
| `src/scm/`            | source control pane: git output parsers (pure), state and loaders, operations, drawing, keys and mouse |
| `src/changes/`        | Changes pane: a file's or folder's changes against the tab's scope, staged by line, with review notes |
| `src/diff/`           | diff collection, parser and layout (inline, side by side, word highlights), shared by the Changes pane, the preview and source control |
| `src/edit.rs`         | Editor pane: runs the terminal editor on a file                                              |
| `src/core/`           | shared core: actions, text buffer, tabs, notes, scopes, staging, icons, terminal helpers (`ui`, `screen`), safe program execution (`exec`, `which`), GitHub's pull requests (`gh`), places, viewed files, updates, setup, workers, time (`clock`), regular expressions (`regex`), paths |
| `src/render/`         | syntax highlighter, Markdown, image decoders and rendering, branch graph                     |
| `src/codec/`          | inflate, deflate, SHA-1, SHA-256, base64                                                     |
| `src/unicode/`        | Unicode classes (generated tables) and the natural order of names |
| `src/sys/`            | the terminal, the process and folder watching per system (Linux, macOS, Windows)             |
| `src/json.rs`         | JSON read and written as the JS did                                                          |
| `Cargo.toml`, `build.rs` | the crate, with no dependency; `build.rs` embeds the version of `herdr-plugin.toml` and the commit built (shown in the key help)      |
| `rust-toolchain.toml` | the toolchain and the 5 release targets                                                      |
| `skills/herdr-preview/` | Claude Code skill that calls peek                                                          |
| `tools/fake-herdr/`   | the fake herdr the tests put first on `PATH`                                                 |
| `release-please-config.json`, `.release-please-manifest.json` | release automation                       |
| `tests/`              | integration tests, see [Tests](#tests)                                                       |
| `.github/workflows/ci.yml` | runs the checks on every pull request and push to `main`                                |
| `.github/workflows/release.yml` | runs release-please on every push to `main`, builds and attaches the binaries, publishes the release |
| `CHANGELOG.md`        | generated by release-please                                                                  |

## Tests

```sh
scripts/check.sh
```

It runs, stopping at the first failure: `cargo fmt --check`, `cargo clippy` for the host and for the 5 release
targets (compiled without linking), then `cargo test`. It runs `$CARGO`, by default `~/.cargo/bin/cargo`, and does
not look in `PATH`. The tests need
neither herdr nor a terminal: those that run a pane, the hook or peek remove herdr's variables and put a fake herdr
(`tools/fake-herdr/`) first on `PATH`. `.github/workflows/ci.yml` runs the same checks on every pull request and
every push to `main`, on Linux (x86_64 and arm64), macOS and Windows. It also checks that the shell scripts (`sh -n scripts/*.sh`) and the PowerShell scripts (`scripts/*.ps1`, Windows
PowerShell 5.1's parser) parse. Each night it runs a long pass of the mutation bench.

| File (`tests/`     ) | Covers |
| --- | --- |
| `modules.rs` | the module fixtures recorded from the JS of 0.8.0 (`tests/fixtures/modules/`     ), replayed against the port. `HX_RECORD=1` writes what the port returns instead, for a change of behaviour its commit explains |
| `screens.rs` | screen captures of the panes in a terminal, started directly, against those recorded from the JS (Linux). Each scenario is a `tests/fixtures/screens/<pane>/<name>.keys` file, whose directives `tests/support/pty.rs` documents; `HX_ONLY=<prefix>` runs only the scenarios whose name starts with it, `HX_RECORD=1` records them again. The version and commit at the foot of the key help are compared as `<build>`, so that a capture does not change with each commit |
| `shims.rs` | every command of the manifest runs the binary or a script |
| `smoke.rs` | the manifest's commands as herdr runs them (Linux): each pane started and quit, the hook and peek, the restart on code 75 and the folder announced through `EXPLORER_IPC_FILE` |
| `docs.rs` | relative links and `#anchors` of `README.md`, `SECURITY.md`, `CONTRIBUTING.md` and the `.md` files under `docs/` |
| `layers.rs` | imports follow the layers of [How it works](docs/architecture.md#how-it-works): never up a layer, never from one pane to another. A new top-level module must be added to a layer there |
| `show.rs` | a commit, stash or tag shown by the preview's Diff view |
| `ui.rs` | terminal width of text (emoji, flags, wide characters), the segment helpers and the menus |
| `editor.rs` | in-place editing (`core::buffer`) |
| `explorer_tree.rs` | the explorer's rows, their order and the filter (`explorer::tree`) |
| `graph.rs` | the lanes of the history graph |
| `icons.rs` | icon widths in each icon set |
| `image.rs`, `image_render.rs` | WebP through `dwebp`, and where the image rendering parts from the JS on purpose |
| `markdown.rs` | rendered Markdown: anchors, footnote lines, inline tags, lists; Rust's raw strings in the highlighter |
| `markdown_images.rs` | images of rendered Markdown in pixels |
| `places.rs` | the places of peek's argument (`core::places`) |
| `remote_link.rs` | links to a network share found without being followed, and refused by the file actions (skipped where links cannot be created, such as Windows without developer mode) |
| `review.rs` | review notes and the lines of a diff they point at |
| `scm_parse.rs` | the git output parsers (`scm::parse`) |
| `scm_tool.rs` | the source control tool of the settings (`scmCommand`), run with its arguments at the root |
| `stage.rs` | staging, unstaging and discarding parts of a diff |
| `state.rs` | the state files left behind by closed tabs and panes |
| `actions.rs`, `full_tab.rs` | the file actions and the programs they start, and the actions on herdr's side |
| `hook.rs`, `launch.rs` | the hook and peek run as the binary, and the launcher's restarts |
| `watch.rs` | the folder watcher |
| `worker.rs` | a worker job that panics: its fallback result is still sent, in a binary of its own |
| `mutate.rs` | the decoders and parsers (images, zlib, JSON, base64, diffs, git output, Markdown, highlighting, regular expressions) on the fixtures' inputs altered at random: no panic, no call over 2 s. `MUTATE_ROUNDS` sets the calls per target, `MUTATE_SEED` replays a run |
| `fetch_bin.rs` | `scripts/fetch-bin.sh` against a local release, without the network |

The module fixtures and the screen captures were recorded from the JS before the switch to Rust and are not
recorded again: a change of behaviour on purpose updates them by hand, with its reason in the commit. They are
compared byte for byte, so `.gitattributes` keeps LF everywhere and never converts `tests/fixtures/`. The fake herdr
logs each call to `$FAKE_HERDR_DIR/calls.log` and answers with the file `$FAKE_HERDR_DIR/<arg1>-<arg2>`, else
`{"result":null}`. Unit tests sit
next to the code they test (the update check, the codecs, the viewed files…). The links of the docs are
checked with GitHub's anchors: the heading in lower case, punctuation other than `-` and `_` dropped, spaces turned
into dashes. When you rename a heading, update the links to it, including the one the preview opens from itself
(`docs/usage/preview.md#sharp-images`; `docs/usage.md` keeps a *Sharp images* heading for the binaries already
released, which open that page).

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
`.github/workflows/release.yml` on every push to `main`. The only step done by hand is merging the release pull
request:

1. Merge pull requests into `main` with Conventional Commit titles, once CI passes.
2. When the merged commits include a type of the changelog table above (`feat`, `fix`, `perf`,
   `docs`, `revert`), release-please opens or updates a release pull request titled `chore: release X.Y.Z`. It
   bumps `version` in `herdr-plugin.toml` (through `extra-files`) and in
   `.release-please-manifest.json`, and updates `CHANGELOG.md`. The same run checks that `herdr-plugin.toml`
   on that branch parses and carries the manifest's version.
3. Merge the release pull request (squash) once the CI of `main` passes: its commit holds the same code as `main`,
   only the version and the changelog differ. The run of that merge tags `vX.Y.Z` (no
   component in the tag) and creates the GitHub release as a draft, then checks that the version has only digits
   and dots and that `herdr-plugin.toml` at the tag carries it, as the update check requires. The `build` job
   compiles the binary of the tag for the 5 targets (`cargo build --release --locked`) and attaches each one; `publish` then attaches `SHA256SUMS` and
   publishes the release, once all 5 binaries are there.
4. Installed explorers find the release within a day through the update check (`releases/latest`)
   and offer the update: they download its binary, check it against `SHA256SUMS`, move the clone to the tag and
   restart on the new code.

Until the release pull request is merged, every new `feat`, `fix`, `perf`, `docs` or `revert` joins it: merge it
when the changes should go out as one version.

Do not edit the version, `.release-please-manifest.json` or `CHANGELOG.md` by hand: release-please maintains them,
and the update check and the panes' self-restart both depend on the version.
