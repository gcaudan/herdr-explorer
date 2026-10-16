# Installation

Install the plugin [from the herdr plugin index](#from-the-herdr-plugin-index), which manages the checkout and its
updates but adds no key bindings, or [from a git clone](#from-a-git-clone), whose install script adds them and whose
updates are fast-forward pulls.

## Requirements

| Requirement | Used for |
|---|---|
| herdr 0.9.3 or later | the host (`min_herdr_version` in `herdr-plugin.toml`) |
| Node.js 20 or later, as `node` on the `PATH` herdr gives plugins | every pane and hook runs `node src/<script>.js` |
| A [Nerd Font](https://www.nerdfonts.com/) (v3) in the terminal running herdr | icons, chevrons and header icons; without one, they show as empty boxes |
| git (optional) | git status, the changes view, source control, updates. Without it, the explorer and preview still work |
| VS Code or another graphical editor (optional) | the external editor (see [Editor command](configuration.md#editor-command)) |
| A terminal editor (optional) | the [Editor pane](usage.md#editor-pane) |
| The [Claude Code](https://claude.com/claude-code) CLI, `claude` (optional) | commit messages written by Claude |
| `dwebp`, from libwebp (optional) | WebP images in the preview (`webp` package on Linux, `brew install webp` on macOS) |
| A terminal with the Kitty graphics protocol (optional) | sharp images in the preview, with **Settings → Images** (Kitty, Ghostty, WezTerm, Konsole…, with herdr's `terminal.kitty_graphics`, on by default); half blocks otherwise, see [Sharp images](usage.md#sharp-images) |

### Platform tools

Each feature degrades gracefully without its tool.

| Feature | Linux | macOS | Windows |
|---|---|---|---|
| Clipboard | `wl-copy`, else `xclip`, else OSC 52 | `pbcopy` | PowerShell `Set-Clipboard` |
| Trash | `gio trash`, else `trash-put` | `/usr/bin/trash` (macOS 14), else Finder | Recycle Bin (PowerShell) |
| Open in app | `xdg-open` | `open` | `explorer.exe` |
| Reveal | `xdg-open` on the parent folder | `open -R` (selects the file) | `explorer.exe /select` |

Without a trash, deleting asks a second time before removing for good.

Install a Nerd Font, for instance `JetBrainsMono Nerd Font` (from your package manager, from
[nerdfonts.com](https://www.nerdfonts.com/), or with `winget install DEVCOM.JetBrainsMonoNerdFont` on Windows), and
select it in your terminal.

## From the herdr plugin index

```sh
herdr plugin install gcaudan/herdr-explorer
```

herdr manages this checkout and its updates, but does not add key bindings: append the blocks of
[Key bindings](#key-bindings) to herdr's `config.toml`, then run `herdr server reload-config`.

To update, run `herdr plugin update gcaudan.explorer` (`↑` in the explorer copies it): herdr installs the new version into a new folder, so the open explorers and previews keep the old code until you reopen
them (`prefix+f` twice). To uninstall, see [Uninstall](#uninstall).

## From a git clone

```sh
git clone https://github.com/gcaudan/herdr-explorer.git
cd herdr-explorer
./install.sh            # Windows (PowerShell): powershell -ExecutionPolicy Bypass -File .\install.ps1
```

Keep the clone where it is, on `main`: herdr runs the plugin from that folder, and updates pull into it.

### What the install script does

`install.sh` and `install.ps1` do the same thing:

1. Stop with `herdr not found` when `herdr` is not on `PATH`.
2. Run `herdr plugin unlink gcaudan.explorer` (errors ignored), then `herdr plugin link <clone folder>`.
   Running the script again is safe.
3. Open the herdr config, `~/.config/herdr/config.toml` on Linux and macOS or
   `%APPDATA%\herdr\config.toml` on Windows, and create it if missing.
4. Append one `[[keys.command]]` block per action (see [Key bindings](#key-bindings)), unless the config already
   mentions that action. A binding you already changed, for example to another key, is left alone.
5. Run `herdr server reload-config`, ignoring errors.
6. When Claude Code is installed (a `~/.claude` folder), link the [herdr-preview skill](usage.md#show-a-file-from-an-agent-or-a-script)
   into `~/.claude/skills/herdr-preview` (a junction on Windows), so that Claude Code can show you files in the
   preview. A link of that name is replaced; a folder that is not a link is left alone.

The script prints the plugin folder, each key it added and the skill's link.

herdr keeps its own copy of the manifest: after pulling a version that changes it, run the script again (or
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

### Adding the explorer to existing tabs

The explorer opens by itself only in tabs created after the install, and only in a tab that has a single pane
(setting [`autoOpen`](configuration.md#settings)). In an existing tab, press `prefix+f`. The hook script also has a
mode that adds the explorer to every existing tab that has a single pane:

```sh
node src/hook.js --all
```

Run it from the plugin folder, in a herdr pane.

## Updates

### The update check

Once a day at most, from the hook (at herdr startup and when a tab is created, never from the panes), the plugin
asks GitHub for the latest release (`api.github.com/repos/gcaudan/herdr-explorer/releases/latest`, 3 s timeout). A
failed check waits for the next day. Only a version made of digits and dots (a leading `v` stripped) is kept, in
`update.json`. Turn it off with **Settings → Check for updates**.

When a newer release exists, a green `↑` shows in the explorer header, and **↑ X.Y.Z available** under the
settings title (`u` to update). It is compared with the version on disk, so it goes away once you have pulled, and
a clone ahead of the release (development) shows nothing. The `↑` is also hidden while an update runs or a header
message shows.

### Updating

Click the `↑`, or press `u` (or click **↑ X.Y.Z available**) in the settings.

- **Installed from the plugin index**: herdr owns that checkout. Without asking, the explorer copies
  `herdr plugin update gcaudan.explorer` instead (*Update command copied*): run it in a terminal.
- **Linked git clone**: after confirmation, the explorer runs `git pull --ff-only --no-rebase` in the
  plugin folder. *Updating…* shows in the header while it runs, which can take up to a minute. Success messages
  are `Already up to date` and `Updated to X.Y.Z`.

### Refusals

The update checks, in this order (`src/core/update.js`), and stops with the message when a check fails:

| Check | Message |
|---|---|
| take `update.lock` in the state folder (one update at a time across all explorers; a lock older than 2 min is taken over) | `Update in progress` |
| a `git` executable is on `PATH` | `git not found in PATH` |
| `git rev-parse --show-toplevel` is the plugin folder itself, not a copy inside another repository such as dotfiles | `Not a git clone` |
| the current branch is `main` | `Not on main` |
| no merge, rebase, cherry-pick, revert, `git am` or bisect in progress | `A git operation is in progress` |
| `git status --porcelain --untracked-files=no` is empty (untracked files are allowed) | `Local changes in the plugin folder` |

A refusal copies nothing. Then comes `git pull --ff-only --no-rebase`, with a 60 s timeout and no prompt: no
terminal, no Git Credential Manager window, and ssh in batch mode unless you set `GIT_SSH_COMMAND`, `GIT_SSH` or
`core.sshCommand` yourself (see [SECURITY.md](../SECURITY.md#updates)).

The pull itself can also fail, for example when you are offline, a fast-forward is impossible, or an
ssh key needs a passphrase. The message is then git's error followed by `update command copied`: the explorer has
copied `git -C "<plugin folder>" pull --ff-only --no-rebase` to the clipboard, to run in a terminal.

### Finishing an update

When the update changed `herdr-plugin.toml` (beyond its version) or an install script, the message is
`Updated to X.Y.Z: rerun the installer`: every explorer header shows a yellow `!`, and the settings
**! Finish the update** (`u`). Either one copies the installer command (`"<plugin folder>/install.sh"`, or
`powershell -ExecutionPolicy Bypass -File …` on Windows), and the `!` goes away; run it in a terminal. The plugin
never reruns the installer itself, since it unlinks and relinks the plugin while its panes run.

### Pane restart

The explorer and preview panes check the version in `herdr-plugin.toml` every 2 s. When it changes, through the
update or a `git pull` you ran yourself, each pane restarts on the new code 2 s later, in place (same pane, width
and position), once you are not busy:

- the explorer, while no prompt, confirmation, menu, active filter, settings or key help is open and
  no update is running;
- the preview, while no find field is being typed and no mouse selection is being dragged.

A pull that leaves the version unchanged restarts nothing. The explorer keeps its tree root (*Set as root*
included), the preview its file and line. The other panes start a new process each time they open, so they always
run the current code.

## Uninstall

1. Remove the plugin from herdr: `herdr plugin unlink gcaudan.explorer` for a linked clone,
   `herdr plugin uninstall gcaudan.explorer` for a plugin index install.
2. Delete the `[[keys.command]]` blocks whose `command` starts with `gcaudan.explorer.` from your
   herdr `config.toml`, then run `herdr server reload-config`.
3. Delete the skill's link, `~/.claude/skills/herdr-preview`, if the install script made it.
4. Optionally, delete the clone and the plugin's state and config folders (see
   [Files](configuration.md#files)).
