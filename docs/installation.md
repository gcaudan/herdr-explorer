# Installation

There are two ways to install the plugin:

- **From the herdr plugin index**: `herdr plugin install gcaudan/herdr-explorer`, then add the key
  bindings yourself (see [From the herdr plugin index](../README.md#from-the-herdr-plugin-index)). herdr manages that checkout and its
  updates.
- **From a git clone**, described below. The install script links the clone to herdr and adds the
  key bindings. Updates are fast-forward pulls of the clone.

Both need herdr 0.9.3 or later and `node` on `PATH`.

## From a git clone

### Linux and macOS

```sh
git clone https://github.com/gcaudan/herdr-explorer.git
cd herdr-explorer
./install.sh
```

- Optional tools (see [Platform tools](../README.md#platform-tools)):
  - VS Code with `code` in `PATH`, or another editor set in the settings;
  - on Linux, `wl-copy` (Wayland) or `xclip` (X11) for the copy actions, and `gio` or `trash-put` for the trash;
  - on macOS, nothing: `pbcopy` and the trash are built in.
- Install a Nerd Font (for instance `JetBrainsMono Nerd Font`, from your package manager or
  [nerdfonts.com](https://www.nerdfonts.com/)) and select it in your terminal.

### Windows (PowerShell)

```powershell
git clone https://github.com/gcaudan/herdr-explorer.git
cd herdr-explorer
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

- The clipboard and the recycle bin go through PowerShell, which is built in. VS Code is found in `PATH` or in its
  default user install folder. Any other editor must be an `.exe`: a `.cmd` launcher cannot run without a shell.
- Install a Nerd Font with `winget install DEVCOM.JetBrainsMonoNerdFont`, then select it in Windows Terminal.

Keep the clone where it is, on `main`: herdr runs the plugin from that folder, and updates pull into it.

### What the install script does

`install.sh` and `install.ps1` do the same thing:

1. Stop with `herdr not found` when `herdr` is not on `PATH`.
2. Run `herdr plugin unlink gcaudan.explorer` (errors ignored), then `herdr plugin link <clone folder>`.
   Running the script again is safe.
3. Open the herdr config, `~/.config/herdr/config.toml` on Linux and macOS or
   `%APPDATA%\herdr\config.toml` on Windows, and create it if missing.
4. Append one `[[keys.command]]` block per action, unless the config already mentions that action:

   | Key        | Command                    | Description      |
   | ---------- | -------------------------- | ---------------- |
   | `prefix+f` | `gcaudan.explorer.toggle`  | file explorer    |
   | `prefix+a` | `gcaudan.explorer.search`  | go to file       |
   | `prefix+d` | `gcaudan.explorer.scm`     | source control   |

   A binding you already changed, for example to another key, is left alone.
5. Run `herdr server reload-config`, ignoring errors.

`prefix+f` means `ctrl+b`, then `f`, with herdr's default prefix. The actions can be bound to other keys in
`config.toml`.

herdr keeps its own copy of the manifest: after pulling a version that changes it, run the script again (or
`herdr plugin link .`) so that herdr picks up new hooks, such as the startup hook. The update tells you when (see
[Finishing an update](#finishing-an-update)).

The script prints the plugin folder and each key it added.

### Adding the explorer to existing tabs

The explorer opens by itself only in tabs created after the install. In an existing tab, press
`prefix+f`. The hook script also has a mode that adds the explorer to every existing tab that has a
single pane:

```sh
node src/hook.js --all
```

Run it from the plugin folder, in a herdr pane.

## Updates

### The update check

Once a day at most, the plugin asks GitHub for the latest release
(`api.github.com/repos/gcaudan/herdr-explorer/releases/latest`, 3 s timeout). The check runs from the
hook, both at herdr startup and when a tab is created, never from the panes. A failed check is not
retried before the next day. Only a version made of digits and dots (a leading `v` stripped) is
kept, in `update.json`. Turn it off with **Settings… → Check for updates**.

When a newer release exists, a green `↑` shows in the explorer header and the settings menu gets
**Update to X.Y.Z…**. The version found is compared with the version on disk, so the `↑` goes away
once you have pulled. A clone ahead of the latest release, as in development, shows nothing. The `↑` is also hidden while an
update runs and while a header message is shown.

### Updating

Click the `↑`, or choose **Update to X.Y.Z…** in the settings, then confirm with `y`.

- **Installed from the plugin index**: herdr owns that checkout. Instead of pulling, the explorer
  copies `herdr plugin update gcaudan.explorer` to the clipboard (*Update command copied*). Run it
  in a terminal. That update installs into a new folder, so panes already open keep running the old
  code until you reopen them.
- **Linked git clone**: the explorer runs `git pull --ff-only --no-rebase` in the plugin folder.
  *Updating…* shows in the header while it runs, which can take up to a minute.

Only one update runs at a time across all explorers. A lock file in the state folder handles this,
and is taken over after 2 minutes if a previous update was killed.

### Refusals

The update checks, in this order (`src/update.js`), and stops with the message when a check fails:

| Check | Message |
|---|---|
| take `update.lock` in the state folder (one update at a time across all explorers; a lock older than 2 min is taken over) | `Update in progress` |
| a `git` executable is on `PATH` | `git not found in PATH` |
| `git rev-parse --show-toplevel` is the plugin folder itself, not a copy inside another repository such as dotfiles | `Not a git clone` |
| the current branch is `main` | `Not on main` |
| no merge, rebase, cherry-pick, revert, `git am` or bisect in progress | `A git operation is in progress` |
| `git status --porcelain --untracked-files=no` is empty (untracked files are allowed) | `Local changes in the plugin folder` |

A refusal copies nothing. Then comes `git pull --ff-only --no-rebase`, with a 60 s timeout. Every git call runs
with `GIT_TERMINAL_PROMPT=0` and `GCM_INTERACTIVE=never` (no Git Credential Manager window), and outside Windows in
a detached session with no terminal, so nothing can prompt in the pane. The pull uses
`GIT_SSH_COMMAND="ssh -o BatchMode=yes"` unless you set `GIT_SSH_COMMAND`, `GIT_SSH` or `core.sshCommand`
yourself.

The pull itself can also fail, for example when you are offline, a fast-forward is impossible, or an
ssh key needs a passphrase. In that case the explorer shows git's error and copies
`git -C "<plugin folder>" pull --ff-only --no-rebase` to the clipboard, so that you can run it in a
terminal.

Success messages are `Already up to date` and `Updated to X.Y.Z`.

### Finishing an update

When the update changed `herdr-plugin.toml` (beyond its version) or one of the install scripts, the
message is `Updated to X.Y.Z: rerun the installer`. A yellow `!` then shows in every explorer header,
and the settings get **Finish the update…**. Clicking either copies the installer command to the
clipboard (`"<plugin folder>/install.sh"`, or the `powershell -ExecutionPolicy Bypass -File …`
command on Windows). The `!` goes away once the command is copied. Run the command in a terminal.
The plugin never reruns the installer itself, because the installer unlinks and relinks the plugin
while its panes are running.

### Pane restart

The explorer and preview panes watch the version in `herdr-plugin.toml`, checked every 2 s. When it
changes, through the update or a `git pull` you ran yourself, each pane restarts on the new code
2 s later, in place: same pane, width and position. A pull that leaves the version unchanged restarts
nothing. It waits until you are not busy:

- the explorer, while no prompt, confirmation, menu or active filter is open and no update is running;
- the preview, while no find field is being typed and no mouse selection is being dragged.

The explorer keeps its tree root (*Set as root* included). The preview keeps its file and line. The search, source control,
changes and editor panes are new processes each time they open, so they always run the current code.

## Uninstall

1. Remove the plugin from herdr. For a linked clone:

   ```sh
   herdr plugin unlink gcaudan.explorer
   ```

   For a plugin index install: `herdr plugin uninstall gcaudan.explorer`.
2. Delete the `[[keys.command]]` blocks whose `command` starts with `gcaudan.explorer.` from your
   herdr `config.toml`, then run `herdr server reload-config`.
3. Optionally, delete the clone and the plugin's state and config folders (see
   [Files](configuration.md#files)).
