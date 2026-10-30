# Installation

Install the plugin [from the herdr plugin index](#from-the-herdr-plugin-index), which manages the checkout and its
updates, or [from a git clone](#from-a-git-clone), which you update yourself. Then [set it up](#setup): the explorer
offers it on its first start. Either way, the plugin runs as one binary, `bin/herdr-explorer`, downloaded from
the release of its version (see [The binary](#the-binary)).

## Requirements

| Requirement | Used for |
|---|---|
| herdr 0.9.4 or later, as `herdr` on `PATH` (or its path in `HERDR_BIN_PATH`) | the host (`min_herdr_version` in `herdr-plugin.toml`) |
| `curl`, and `sha256sum` or `shasum` (`curl.exe` and PowerShell on Windows, both built in on Windows 10 and later) | downloading the binary and checking it, at install and at each update |
| Linux or macOS on x86_64 or arm64, or Windows on x86_64 or arm64 | the platforms with a release binary; elsewhere, build it with [dev-build](#building-the-binary) |
| A [Nerd Font](https://www.nerdfonts.com/) (v3) in the terminal running herdr | icons, chevrons and header icons; without one, choose emoji, Unicode or ASCII icons in the settings ([Without a Nerd Font](configuration.md#without-a-nerd-font)) |
| git 2.31 or later (optional), in an absolute folder of `PATH` (relative entries are ignored) | git status, the changes view, source control, `peek --last`, updates. Without it, the explorer and preview still work |
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
binary in `bin/`. herdr manages this checkout and its updates, but adds no key bindings: accept the explorer's offer
on its first start, or run [setup](#setup).

To update, run `herdr plugin update gcaudan.explorer` (`↑` in the explorer copies it): herdr installs the new version into a new folder, so the open explorers and previews keep the old code until you reopen
them (`prefix+f` twice). To uninstall, see [Uninstall](#uninstall).

## From a git clone

Clone the latest release's tag (the version on the
[releases page](https://github.com/gcaudan/herdr-explorer/releases/latest)), not `main`: the binary is downloaded for
the version of the clone, and only a release has one.

```sh
git clone --branch v<X.Y.Z> https://github.com/gcaudan/herdr-explorer.git
cd herdr-explorer
sh scripts/fetch-bin.sh     # Windows (PowerShell): powershell -ExecutionPolicy Bypass -File scripts\fetch-bin.ps1
herdr plugin link .
bin/herdr-explorer setup    # or accept the explorer's offer on its first start
```

Keep the clone where it is: herdr runs the plugin from that folder. To update it, see [Updating](#updating).

## Setup

herdr's plugin index adds no key bindings, nor links the skill for Claude Code: the binary does, with

```sh
<plugin folder>/bin/herdr-explorer setup    # Windows: <plugin folder>\bin\herdr-explorer.exe setup
```

1. Open the herdr config, and create it if missing: the file `HERDR_CONFIG_PATH` names when set, else
   `%APPDATA%\herdr\config.toml` on Windows, else `$XDG_CONFIG_HOME/herdr/config.toml` (`~/.config/herdr/config.toml`
   without it), macOS included.
2. Append one `[[keys.command]]` block per action (see [Key bindings](#key-bindings)), unless the config already
   binds that action. A binding you already changed, for example to another key, is left alone, and a key the
   config already binds to something else is skipped, with a message (keys compared without regard to case, as
   herdr reads them: `Prefix+F` is `prefix+f`). A config that cannot be read, or that writes
   `keys.command` inline (which a block appended would make invalid), is not changed: the blocks to copy are
   printed instead. When it added one, run `herdr server reload-config`, ignoring errors.
3. When Claude Code is installed (a `~/.claude` folder), link the [herdr-preview skill](usage.md#show-a-file-from-an-agent-or-a-script)
   into `~/.claude/skills/herdr-preview` (a symbolic link, a junction on Windows, which needs no admin right), so
   that Claude Code can show you files in the preview. A link of that name is replaced; a folder that is not a link
   is left alone, with a message.

It prints each key it added (or `already bound`) and the skill's link. Running it again is safe.

On its first start, when the config binds none of the plugin's actions, the explorer offers the same thing once:
**Set up herdr-explorer**, `y` **Set up** or `n` **Later** (never offered again: `setup-offered` in the
[state folder](configuration.md#files)). At each start, the explorer also points an existing skill link at its own
plugin folder, as `herdr plugin update` installs into a new one.

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

Setup leaves out the **peek** action, which shows the changed file written last in the preview (see
[Show a file from an agent or a script](usage.md#show-a-file-from-an-agent-or-a-script)). To bind it too:

```toml
[[keys.command]]
key = "prefix+v"
type = "plugin_action"
command = "gcaudan.explorer.peek"
description = "preview the last changed file"
```

### The binary

Every command of the manifest runs `bin/herdr-explorer` (`bin\herdr-explorer.exe` on Windows, which herdr finds by
itself). `scripts/fetch-bin.sh` (`scripts\fetch-bin.ps1`) puts it there:

1. It reads `version` in `herdr-plugin.toml`, and does nothing when `bin/.dev` exists (a binary you built, see
   below) or when the binary already gives that version.
2. It downloads the release asset for this machine (`herdr-explorer-<arch>-<os>`) and the release's `SHA256SUMS`
   from `https://github.com/<owner>/<repo>/releases/download/v<version>/`: the clone's `origin` when it is on
   GitHub, so a fork uses its own releases, else `gcaudan/herdr-explorer`. Windows on arm64 gets the x86_64 binary,
   which runs in emulation.
3. It checks the asset against its line of `SHA256SUMS`, then puts it in place by renaming; the old binary is kept
   aside as `herdr-explorer.old-<date>-<pid>`, which the binary removes when it next starts.

One fetch runs at a time (`bin/.fetch.lock`, taken over after 2 min). On any failure, no curl, no download, a hash
that does not match, `bin/` is left as it was and the script says why. The manifest's `[[build]]` steps run it at
`herdr plugin install` and `update`, and the explorer and preview panes, their own
launcher, run it before they restart on new code
([Launcher and self-restart](architecture.md#launcher-and-self-restart)). While the binary is missing (a clone without
`herdr plugin install`), no pane, hook or action starts: run
`scripts/fetch-bin.sh` in the plugin folder.

### Building the binary

On a platform without a release binary, or to run your own changes, build it with [Rust](https://rustup.rs) (rustup
installs the toolchain of `rust-toolchain.toml`):

```sh
scripts/dev-build.sh    # Windows (PowerShell): powershell -ExecutionPolicy Bypass -File .\scripts\dev-build.ps1
```

It runs `cargo build --release` (cargo from `PATH`, else `$CARGO_HOME/bin`, by default `~/.cargo/bin`, where rustup
installs it without touching your shell's profile), copies the binary to `bin/` and writes `bin/.dev`: fetch-bin then never replaces it, and the open explorer and preview
panes restart on the new binary once idle. Run it again after each change; delete `bin/.dev` (and run
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
a clone ahead of the release (development) shows nothing. A version you skipped (see below) shows dimmed in the
settings only, as **X.Y.Z skipped** (`u` to update anyway). The check keeps the release's changes (the bullets of its
notes) and its page, for the dialog below.

### Updating

The explorer never installs an update itself: herdr's plugin index is the only channel. Click the pill, or press `u`
(or click **↑ X.Y.Z available**) in the settings: a dialog shows the installed and new versions and the release's
first three changes, with four answers: `c` **Copy command** (copies `herdr plugin update gcaudan.explorer`, *Update
command copied: run it, then reopen the panes*), `r` **Notes** (the release page in the browser), `s` **Skip
version** (no pill until the next release), and `n` **Later**, selected at first.

`herdr plugin update` runs the new version's build step, which fetches its binary into `bin/`: when it fails
(offline, no `curl`), herdr keeps the installed version. After an
update, the explorers show, for a minute, a blue `✓ what's new` pill that opens the release notes.

A linked git clone is yours to update: `git pull` (or `git checkout` the new tag), then `sh scripts/fetch-bin.sh`
for the binary of the new version. Its explorer and preview panes then restart by themselves (see below).

### Pane restart

The explorer and preview panes watch the version in `herdr-plugin.toml`. When it changes, for instance
after a `git pull` in a linked clone (then run `scripts/fetch-bin.sh` too, for the binary of the new version), each pane restarts on the new code 2 s later, in place (same pane, width
and position), once you are not busy:

- the explorer, while no prompt, confirmation, dialog, menu, active filter, settings or key help is open;
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
3. Delete the skill's link, `~/.claude/skills/herdr-preview`, if setup made it.
4. Optionally, delete the clone and the plugin's state and config folders (see
   [Files](configuration.md#files)).
