# Installation

Install the plugin [from the herdr plugin index](#from-the-herdr-plugin-index), which manages the checkout and its
updates but adds no key bindings, or [from a git clone](#from-a-git-clone), whose install script adds them and which
updates itself to each new release. Either way, the plugin runs as one binary, `bin/herdr-explorer`, downloaded from
the release of its version (see [The binary](#the-binary)).

## Requirements

| Requirement | Used for |
|---|---|
| herdr 0.9.4 or later, as `herdr` on `PATH` (or its path in `HERDR_BIN_PATH`) | the host (`min_herdr_version` in `herdr-plugin.toml`) |
| Node.js 20 or later, as `node` on the `PATH` herdr gives plugins | the manifest's hooks, actions and short-lived panes, `node src/<script>.js` : small shims that start the binary (the explorer and preview panes run the binary directly) |
| `curl`, and `sha256sum` or `shasum` (`curl.exe` and PowerShell on Windows, both built in on Windows 10 and later) | downloading the binary and checking it, at install and at each update |
| Linux or macOS on x86_64 or arm64, or Windows on x86_64 or arm64 | the platforms with a release binary; elsewhere, build it with [dev-build](#building-the-binary) |
| A [Nerd Font](https://www.nerdfonts.com/) (v3) in the terminal running herdr | icons, chevrons and header icons; without one, choose emoji, Unicode or ASCII icons in the settings ([Without a Nerd Font](configuration.md#without-a-nerd-font)) |
| git (optional), in an absolute folder of `PATH` (relative entries are ignored) | git status, the changes view, source control, `peek --last`, updates. Without it, the explorer and preview still work |
| VS Code or another graphical editor (optional) | the external editor (see [Editor command](configuration.md#editor-command)) |
| A terminal editor (optional): `$VISUAL`, `$EDITOR`, else the first of `nano`, `nvim`, `vim`, `vi` (`edit`, `nvim`, `vim`, `nano` on Windows) | the [Editor pane](usage.md#editor-pane) |
| The [Claude Code](https://claude.com/claude-code) CLI, `claude` (optional) | commit messages written by Claude |
| `dwebp`, from libwebp (optional) | WebP images in the preview (`webp` package on Linux, `brew install webp` on macOS) |
| A terminal with the Kitty graphics protocol (optional) | sharp images in the preview, with **Settings → Images** (Kitty, Ghostty, WezTerm, Konsole…, with herdr's `terminal.kitty_graphics`, on by default); half blocks otherwise, see [Sharp images](usage/preview.md#sharp-images) |

### Platform tools

Each feature degrades gracefully without its tool.

| Feature | Linux | macOS | Windows |
|---|---|---|---|
| Clipboard | `wl-copy`, else `xclip`, else OSC 52 | `pbcopy` | PowerShell `Set-Clipboard` |
| Paste (preview edit mode) | `wl-paste`, else `xclip` | `pbpaste` | PowerShell `Get-Clipboard` |
| Trash | `gio trash`, else `trash-put` | `/usr/bin/trash` (macOS 14), else Finder | Recycle Bin (PowerShell) |
| Open in app | `xdg-open` | `open` | `explorer.exe` |
| Reveal | `xdg-open` on the parent folder | `open -R` (selects the file) | `explorer.exe /select` |

Without a trash, deleting asks a second time before removing for good.

Install a Nerd Font, for instance `JetBrainsMono Nerd Font` (from your package manager, from
[nerdfonts.com](https://www.nerdfonts.com/), or with `winget install DEVCOM.JetBrainsMonoNerdFont` on Windows), and
select it in your terminal. Or keep your font and set **Icon set** to emoji, Unicode or ASCII in the explorer's
settings (`,`).

## From the herdr plugin index

```sh
herdr plugin install gcaudan/herdr-explorer
```

herdr runs the manifest's build step, `scripts/fetch-bin.sh` (`scripts\fetch-bin.ps1` on Windows), which puts the
binary in `bin/`. herdr manages this checkout and its updates, but does not add key bindings: append the blocks of
[Key bindings](#key-bindings) to herdr's `config.toml`, then run `herdr server reload-config`. Nor does it link the
[herdr-preview skill](usage.md#show-a-file-from-an-agent-or-a-script) for Claude Code.

To update, run `herdr plugin update gcaudan.explorer` (`↑` in the explorer copies it): herdr installs the new version into a new folder, so the open explorers and previews keep the old code until you reopen
them (`prefix+f` twice). To uninstall, see [Uninstall](#uninstall).

## From a git clone

Clone the latest release's tag (the version on the
[releases page](https://github.com/gcaudan/herdr-explorer/releases/latest)), not `main`: the binary is downloaded for
the version of the clone, and only a release has one.

```sh
git clone --branch v<X.Y.Z> https://github.com/gcaudan/herdr-explorer.git
cd herdr-explorer
./install.sh            # Windows (PowerShell): powershell -ExecutionPolicy Bypass -File .\install.ps1
```

Keep the clone where it is: herdr runs the plugin from that folder, and updates move it to each new release's tag.

### What the install script does

`install.sh` and `install.ps1` do the same thing:

1. Stop with `herdr not found` when `herdr` is not on `PATH`.
2. Put the binary in `bin/` with `scripts/fetch-bin` (see [The binary](#the-binary)); stop, with herdr left as it
   was, when that fails.
3. Run `herdr plugin unlink gcaudan.explorer` (errors ignored), then `herdr plugin link <clone folder>`.
   Running the script again is safe.
4. Open the herdr config, and create it if missing: the file `HERDR_CONFIG_PATH` names when set, else
   `$XDG_CONFIG_HOME/herdr/config.toml` (`~/.config/herdr/config.toml` without it) on Linux and macOS, or
   `%APPDATA%\herdr\config.toml` on Windows.
5. Append one `[[keys.command]]` block per action (see [Key bindings](#key-bindings)), unless the config already
   mentions that action. A binding you already changed, for example to another key, is left alone.
6. Run `herdr server reload-config`, ignoring errors.
7. When Claude Code is installed (a `~/.claude` folder), link the [herdr-preview skill](usage.md#show-a-file-from-an-agent-or-a-script)
   into `~/.claude/skills/herdr-preview` (a junction on Windows, which needs no admin right), so that Claude Code can
   show you files in the preview and updates reach the skill. A link of that name is replaced; a folder that is not a
   link is left alone, with a message.

The script prints the plugin folder, each key it added and the skill's link.

herdr keeps its own copy of the manifest: after updating to a version that changes it, run the script again (or
`herdr plugin link .`) so that herdr picks up new hooks, such as the startup hook. The update tells you when (see
[Finishing an update](#finishing-an-update)).

### Key bindings

```toml
[[keys.command]]
key = "prefix+f"
type = "plugin_action"
command = "gcaudan.explorer.toggle"
description = "file explorer"

[[keys.command]]
key = "prefix+a"
type = "plugin_action"
command = "gcaudan.explorer.search"
description = "go to file"

[[keys.command]]
key = "prefix+d"
type = "plugin_action"
command = "gcaudan.explorer.scm"
description = "source control"
```

`prefix+f` means `ctrl+b`, then `f`, with herdr's default prefix. To use other keys, edit the `key` lines, then run
`herdr server reload-config`.

The install script leaves out the **peek** action, which shows the changed file written last in the preview (see
[Show a file from an agent or a script](usage.md#show-a-file-from-an-agent-or-a-script)). To bind it too:

```toml
[[keys.command]]
key = "prefix+v"
type = "plugin_action"
command = "gcaudan.explorer.peek"
description = "preview the last changed file"
```

### The binary

The panes and the hook all run `bin/herdr-explorer` (`bin\herdr-explorer.exe` on Windows); the scripts of the
manifest, `src/<script>.js`, only start it. `scripts/fetch-bin.sh` (`scripts\fetch-bin.ps1`) puts it there:

1. It reads `version` in `herdr-plugin.toml`, and does nothing when `bin/.dev` exists (a binary you built, see
   below) or when the binary already gives that version.
2. It downloads the release asset for this machine (`herdr-explorer-<arch>-<os>`) and the release's `SHA256SUMS`
   from `https://github.com/<owner>/<repo>/releases/download/v<version>/`: the clone's `origin` when it is on
   GitHub, so a fork uses its own releases, else `gcaudan/herdr-explorer`. Windows on arm64 gets the x86_64 binary,
   which runs in emulation.
3. It checks the asset against its line of `SHA256SUMS`, then puts it in place by renaming; the old binary is kept
   aside as `herdr-explorer.old-<date>-<pid>`, which the binary removes when it next starts.

One fetch runs at a time (`bin/.fetch.lock`, taken over after 2 min). On any failure, no curl, no download, a hash
that does not match, `bin/` is left as it was and the script says why. A shim that finds no binary, or one of
another version, runs the same script before it starts the binary. The explorer and preview panes start the binary
directly and fetch nothing: after a `git pull` of the clone, without `herdr plugin update`, the hook's shim fetches
the new binary at the next tab event, but an explorer or preview that restarts first stays on the old one until
it is reopened.

### Building the binary

On a platform without a release binary, or to run your own changes, build it with [Rust](https://rustup.rs) (rustup
installs the toolchain of `rust-toolchain.toml`):

```sh
scripts/dev-build.sh    # Windows (PowerShell): powershell -ExecutionPolicy Bypass -File .\scripts\dev-build.ps1
```

It runs `cargo build --release` (cargo from `PATH`, else `$CARGO_HOME/bin`, by default `~/.cargo/bin`, where rustup
installs it without touching your shell's profile), copies the binary to `bin/` and writes `bin/.dev`: fetch-bin, the
updates and the shims then never replace it. Run it again after each change; delete `bin/.dev` (and run
`scripts/fetch-bin.sh`) to go back to the release binary.

### Adding the explorer to existing tabs

The explorer opens by itself only in tabs created after the install, and only in a tab that has a single pane
(setting [`autoOpen`](configuration.md#settings)). In an existing tab, press `prefix+f`.

## Updates

### The update check

Once a day at most, from the hook (at herdr startup and when a tab is created, never from the panes), the plugin
asks GitHub for the latest release (`api.github.com/repos/<owner>/<repo>/releases/latest`, 3 s timeout). The
repository is the clone's `origin` when it is on GitHub, so a fork follows its own releases; otherwise
`gcaudan/herdr-explorer`. A failed check waits for the next day. Only a version made of digits and dots (a leading `v` stripped) is kept, in
`update.json`. Turn it off with **Settings → Check for updates**.

When a newer release exists, a green `↑ X.Y.Z` pill shows in the explorer header, and **↑ X.Y.Z available** under
the settings title (`u` to update). It is compared with the version on disk, so it goes away once you have updated, and
a clone ahead of the release (development) shows nothing. The pill is also hidden while an update runs, and for a
version you skipped (see below), which shows dimmed in the settings only, as **X.Y.Z skipped** (`u` to update
anyway). The check keeps the release's changes (the bullets of its notes) and its page, for the dialog below.

### Updating

Click the pill, or press `u` (or click **↑ X.Y.Z available**) in the settings.

- **Installed from the plugin index**: herdr owns that checkout. Without asking, the explorer copies
  `herdr plugin update gcaudan.explorer` instead (*Update command copied*): run it in a terminal.
- **Linked git clone**: a dialog shows the installed and new versions and the release's first three changes, with
  four answers: `y` **Update**, `r` **Notes** (the release page in the browser), `s` **Skip version** (no pill until
  the next release), and `n` **Later**, selected at first. **Update** first downloads the new version's binary
  and checks it against its `SHA256SUMS`, then moves the clone to the release's tag `vX.Y.Z` (`git fetch --tags`,
  then `git merge --ff-only vX.Y.Z` on a branch the tag follows, else `git checkout --detach vX.Y.Z`), and puts the
  binary in `bin/` (left alone when `bin/.dev` exists). *Updating…* shows in the header while it runs, which can
  take a few minutes on a slow network. Success messages are `Already up to date` and `Updated to X.Y.Z`; after an update done here, the restarted explorers show
  *Updated to X.Y.Z* and, for half a minute, a blue `✓ what's new` pill that opens the release notes.

### Refusals

The update checks, in this order (`src/core/update.rs`), and stops with the message when a check fails:

| Check | Message |
|---|---|
| take `update.lock` in the state folder (one update at a time across all explorers; a lock older than 15 min is taken over) | `Update in progress` |
| the binary runs from the `bin/` of a plugin folder | `Not a git clone` |
| a `git` executable is on `PATH` | `git not found in PATH` |
| `git rev-parse --show-toplevel` is the plugin folder itself, not a copy inside another repository such as dotfiles | `Not a git clone` |
| no merge, rebase, cherry-pick, revert, `git am` or bisect in progress | `A git operation is in progress` |
| `git status --porcelain --untracked-files=no` is empty (untracked files are allowed) | `Local changes in the plugin folder` |
| a release binary exists for this platform | `No release binary for this platform` |
| `curl` is on `PATH` | `curl not found in PATH` |
| the binary and `SHA256SUMS` download | `Cannot download the X.Y.Z binary` |
| the binary matches its line of `SHA256SUMS` | `<asset> is not in the X.Y.Z SHA256SUMS`, `The X.Y.Z binary does not match its SHA256SUMS` |

The last four checks apply only when the binary must be fetched: no `bin/.dev`, and a binary of another version.
A refusal changes and copies nothing. Then come git's fetch and move, each with a 60 s timeout and no prompt: no
terminal, no Git Credential Manager window, and ssh in batch mode unless you set `GIT_SSH_COMMAND`, `GIT_SSH` or
`core.sshCommand` yourself (see [SECURITY.md](../SECURITY.md#updates)). While they run, the update holds
`bin/.fetch.lock`, so that a pane starting meanwhile waits and then finds the code and the binary of the same version.

git can still fail, for example when you are offline or an ssh key needs a passphrase. The message is then git's
error followed by `update command copied`: the explorer has copied the commands that do the rest, to run in a
terminal, for instance `git -C "<plugin folder>" fetch --tags && git -C "<plugin folder>" merge --ff-only vX.Y.Z &&
sh "<plugin folder>/scripts/fetch-bin.sh" "<plugin folder>"` (chained with `; if ($?) { … }` on Windows). When the
clone moved but the binary could not be put in `bin/`, the clone is moved back, and the message is
`The X.Y.Z binary could not be put in bin`, with the same kind of command copied.

### Finishing an update

When the update changed `herdr-plugin.toml` (beyond its version) or an install script, the message is
`Updated to X.Y.Z: rerun the installer`: every explorer header shows a yellow `! finish` pill, and the settings
**! Finish the update** (`u`). Either one opens a dialog that shows the installer command (`"<plugin folder>/install.sh"`,
or `powershell -ExecutionPolicy Bypass -File …` on Windows); `c` **Copy command** copies it, and the pill then goes
away in every explorer; run it in a terminal. The plugin
never reruns the installer itself, since it unlinks and relinks the plugin while its panes run.

### Pane restart

The explorer and preview panes check the version in `herdr-plugin.toml` every 2 s. When it changes, through the
update or a `git pull` you ran yourself (then run `scripts/fetch-bin.sh` too, for the binary of the new version), each pane restarts on the new code 2 s later, in place (same pane, width
and position), once you are not busy:

- the explorer, while no prompt, confirmation, menu, active filter, settings or key help is open and
  no update is running;
- the preview, while no find field or review note is being typed, no mouse selection is being dragged and no tab has
  unsaved edits.

A pull that leaves the version unchanged restarts nothing. The explorer keeps its tree root (*Set as root*
included), the preview its tabs, file and line. The other panes start a new process each time they open, so they always
run the current code.

## Uninstall

1. Remove the plugin from herdr: `herdr plugin unlink gcaudan.explorer` for a linked clone,
   `herdr plugin uninstall gcaudan.explorer` for a plugin index install.
2. Delete the `[[keys.command]]` blocks whose `command` starts with `gcaudan.explorer.` from your
   herdr `config.toml`, then run `herdr server reload-config`.
3. Delete the skill's link, `~/.claude/skills/herdr-preview`, if the install script made it.
4. Optionally, delete the clone and the plugin's state and config folders (see
   [Files](configuration.md#files)).
