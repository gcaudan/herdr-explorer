# Security

The explorer opens by itself on any folder, including repositories an agent has just cloned and nobody has reviewed.
It runs git, reads files and writes to your terminal in those folders. This document lists what it does so that a
hostile repository cannot run code, leak credentials or take over your terminal through it.

## Reporting a vulnerability

Report it privately, from the repository's **Security** tab, **Report a vulnerability**
([new advisory](https://github.com/gcaudan/herdr-explorer/security/advisories/new)), not in a public issue. Only the
latest release gets fixes.

## Threat model

The plugin runs with your user's rights. It treats as untrusted:

- file names, file contents and images;
- a repository's own git configuration and the files under its `.git` folder;
- text produced by tools: git output, Claude's answer.

Opening a folder, browsing it, previewing its files or viewing its changes must not run code from it. Commands that
change something run only on an explicit key or menu choice, and destructive ones ask first.

It trusts your own settings (a source control tool set there, `scmCommand`, is a program of your choosing: it runs the
repository's hooks and filters, none of the protections below apply to it), your global and system git configuration, the programs in the absolute folders of your
`PATH`, herdr, and the plugin's own clone.

## Running programs

- **No shell**. Every program runs with an argument list, never through a shell command line.
  The one exception is an npm-installed `claude.cmd` on Windows, which can only run through `cmd`, with fixed
  arguments.
- **Absolute paths**. `git`, `herdr`, `claude`, `dwebp`, a graphical editor command, the clipboard, trash and file
  manager tools, and on Windows every program, run from their absolute path, looked up in the absolute folders of
  `PATH` only. On Windows, a bare program name is first looked up in the working directory, and elsewhere an empty or
  `.` entry of `PATH` means that folder too: a repository could plant a `git` there. On Linux and macOS, the fallback
  terminal editor goes through the usual `PATH` lookup.
- **GitHub CLI**: the PRs view of source control runs `gh pr list`, `gh pr view`, `gh pr diff` and, on `C`,
  `gh pr checkout`, with the repository's git environment (its hooks and filters off for the git gh runs), no
  prompt and a 30 s limit. The checkout's fetch gets the settings of a fetch: refused when the repository sets
  network settings or its own upload-pack, and its ssh command, proxy, askpass and credential helpers replaced. Titles, descriptions and diffs are untrusted text, cleaned of control characters.
- **Editor commands**: the editor setting and `$EXPLORER_EDITOR`, `$VISUAL` or `$EDITOR` are split into words (quotes
  group them), never interpreted by a shell; the file path is the last argument.
- **Output limits**. Program output is read up to 64 MB.
- **The launcher** of the explorer and preview panes runs `scripts/fetch-bin` with the system's own shell and
  PowerShell, by absolute path (`/bin/sh`, and `powershell.exe` under `%SystemRoot%\System32\WindowsPowerShell`),
  never one found in a folder of `PATH`.
- **Messages between processes**: the launcher of the explorer and preview panes and the binary it restarts exchange
  their messages through a file in a new folder of their own (mode 0700 on Unix), created so that it fails if it
  already exists. Another user of a shared temporary folder cannot write the messages first and give the next process
  an environment.

## Git hardening

Every git call in a repository gets an environment built for that repository, passed as
`GIT_CONFIG_KEY_n`/`GIT_CONFIG_VALUE_n` (unlike `-c`, this does not split a driver name containing `=`). It turns
off what the repository's config could use to run commands. An older git than 2.31 ignores those variables: it is
not used at all, as if missing.

- **Hooks**: `core.hooksPath` points at a folder that never exists (`/dev/null`, or `.no-hooks` in the plugin folder
  on Windows, since `C:\dev\null` could be created by any user), and each `hook.<name>.command` the repository
  declares in its config (any `hook.<name>.*` key) gets `enabled=false`. No hook file runs, those of a global hooks folder included, so commits skip
  pre-commit, commit-msg and the others.
- **`core.fsmonitor`** is off.
- **Filters**: each filter driver the repository declares (in `.git/config`, `config.worktree` or an included file)
  gets empty `clean`, `smudge` and `process` commands and `required=false`. Filters from the global and system
  config, such as git-lfs, are kept.
- **Submodules** are ignored (`--ignore-submodules=all`) and never recursed into (`submodule.recurse`,
  `fetch.recurseSubmodules` and `push.recurseSubmodules` off), since their own config is out of reach.
- **Signatures**: `log.showSignature=false`, so the repository's `gpg.program` does not run on signed commits.
- **Lazy fetching**: `GIT_NO_LAZY_FETCH=1`, so a partial clone never fetches a missing object from its promisor
  remote, which would run its `uploadpack` or ssh command, even during `git status`.
- **Working tree**: a `core.worktree` set by the repository is ignored. The working tree stays the folder holding
  `.git`, so switching branch, pulling or discarding cannot write elsewhere, such as your home folder.
- **Locks**: `GIT_OPTIONAL_LOCKS=0`, so `git status` does not take `index.lock`, which would make a commit run at the
  same time fail.

### Commands that write (source control)

Every source control command that changes something (stage, commit, switch, stash, merge, rebase, cherry-pick,
reset, fetch, pull, push…) also replaces every program-running key the repository sets itself with your global
value, or git's default: `core.sshCommand`, `core.askPass`, `gpg.*.program`, `gpg.ssh.defaultKeyCommand`, credential
helpers and merge drivers. `remote.*.uploadpack` and `receivepack` are overridden on the command line, and
`core.gitProxy` is ignored (`GIT_PROXY_COMMAND` emptied). `protocol.ext.allow` and `protocol.file.allow` are set to
`never`.

### Network (fetch, pull, push)

- **Transports**: only `https`, `http`, `ssh` and `git` (`GIT_ALLOW_PROTOCOL`). No `ext::` command, no local remote
  (it would run its own hooks), no remote helper (`git-remote-<name>`).
- **Refused settings**: fetch, pull and push are refused when the repository's own config sets `http.*`,
  `url.*.insteadOf`, `url.*.pushInsteadOf` or `remote.*.proxy` (which could send your credentials elsewhere),
  `core.alternateRefsCommand` (a command git runs during a fetch) or `remote.*.vcs` (a helper program that can live
  in the repository). Run the command in a terminal after checking them.
- **Pull** is fast-forward only (`--ff-only --no-rebase`).
- **No prompt**: `GIT_TERMINAL_PROMPT=0` and a 120 s timeout. On Linux and macOS, network commands, commits and
  *Continue* run in a process group detached from the terminal, so ssh, gpg or a credential helper cannot take over
  the pane to ask for a password; the whole group is killed on timeout.

### Continue

Before `--continue`, source control refuses to go on when the rebase todo contains `exec` lines, or when the merge
strategy is not one of git's own (`ort`, `recursive`, `resolve`, `octopus`, `ours`, `subtree`), or when those cannot
be checked. It reads the strategy from `rebase-merge/strategy`, `sequencer/opts` and `pull.twohead`. `GIT_EDITOR=:`
keeps the prepared message, so no editor (and no `core.editor` the repository sets) opens. Merges, rebases and
cherry-picks started from the pane pass `--strategy=ort`, so `pull.twohead` cannot pick another strategy.

### Diffs

`git diff`, `show` and `stash show` run with `--no-color --no-ext-diff --no-textconv` and fixed `a/` `b/` prefixes:
external diff tools and textconv filters never run. A revision given to the diff pane that starts with `-` or
contains spaces or control characters is refused; the others pass after `--end-of-options`.

## Arguments and names

- **Literal paths**: paths are given to git as literal pathspecs (`GIT_LITERAL_PATHSPECS=1`), so a file named `*` or
  `app/[id]` never matches other files. Source control passes paths on standard input, NUL-separated
  (`--pathspec-from-file`), after `--`.
- **No option injection**: branch and remote names starting with `-` are refused.
- **Typed names**: a name typed for a new or renamed file that is empty, absolute, contains a control character or
  leaves its folder through `..` is refused. The check is on the name only: it does not follow a symbolic link that
  the name goes through.
- **Duplicate** copies symbolic links as links.
- **Saving** an edited file writes a new temporary file beside its target, created exclusively (never through a
  file or link already there) with the file's permissions and group, then renames it over the file. A file with hard
  links, owned by another user, or in a folder that cannot be written is written in place instead.
- **Discard**: untracked files discarded go to the trash, from the explorer's *Discard changes*, source control's
  `d` and `D` in the preview and the Changes pane alike; without a trash, they are deleted for good only after a
  second question. Source control touches them only when they resolve inside the repository.
- **Opening files**: a double-click opens with the system application only documents and media (images, PDF,
  Office, archives, audio, video); everything else goes to the preview or an editor, so it never starts an
  executable, a script or a shortcut. *Open in app* adds only `txt`, `md`, `csv`, `log`, `json`, `xml`, `html` and
  `htm`. The extension decides, not the execute bit (set on every file of FAT and NTFS drives). On macOS, a folder
  that is an application (`x.app`, or a link to one) opens in a Finder window rather than being launched.

## Terminal output

- **Control characters** (C0, DEL, C1) in file names, file contents, git output and diffs are never sent to the
  terminal: they are replaced or dropped, and tabs are expanded. The preview, the search and the diffs colour the
  text themselves, so a file cannot move the cursor, set the title or hide content with escape sequences.
- **Paste**: pasted text has its control characters removed, and a long paste is held until its end marker, so its
  text is never read as keys.
- **Insert into terminal** (also `ctrl+t` in the search) types the text with `herdr pane send-text`, never
  submitted. It refuses names containing control characters (a newline would run the command or submit
  the prompt), prefixes names starting with `-` with `./`, and puts the others in single quotes for the shell when
  they hold anything but plain characters (quoting valid in POSIX shells, in fish, where a backslash goes outside
  the quotes since fish reads `\'` as an escape even inside them, and in PowerShell, where typographic quotes are
  escaped too). For an agent (Claude Code), a path is prefixed with `@`, and one with spaces is quoted instead,
  so its end is clear and the rest cannot read as instructions.
- **Review notes** are typed on one line, without submitting it: control characters are removed from the note texts
  when they are saved and again when they are sent, and the line is refused if one remains. The text goes into the
  agent's prompt as you typed it, so a note asking for something is a request like any other you type there.
- **Kitty graphics**: the preview sends images as pixels it decoded itself, never a file path for the terminal to
  read; the terminal's answers to its queries are read apart from the keys. **Settings → Images** can keep them pixelated,
  drawn with characters only. The link under a pixelated image opens one fixed `https://` page of
  this documentation in the browser, never an address taken from a file; the plugin opens nothing in a browser that
  is not a plain `https://` address.
- **`herdr-explorer peek`** shows, in the preview, a regular file the caller names: anything the user can read, as
  the preview already does, never a folder or a device. An agent calling it changes what you see, not what is run. It writes the
  request file of the state folder only, whose path it reads from `state-dir` in the plugin's config folder.
- **Ctrl-clicked links**: herdr hands the plugin only `file://` hyperlinks. A link naming another host than this
  machine (or `localhost`) is refused, since on Windows it would reach a network share; the file then shows as with
  `peek`, never run.

## Commit messages from Claude

`✧` or `ctrl+g` sends the file list and the diff to the `claude` CLI, only when you ask:

- The prompt, the file list and the diff go to `claude -p` on its standard input. The diff is capped at 60,000
  characters (with nothing staged: the unstaged changes and up to 20 untracked files), and the prompt tells Claude
  to treat it as data, not instructions.
- Claude runs with no tools, no MCP servers, no saved session and user settings only, from the system temporary
  folder, so the repository's `CLAUDE.md`, settings and hooks are not loaded. `CLAUDECODE` is removed from its
  environment. It is stopped after 90 s, and runs detached from the terminal.
- Only the first non-empty line of the answer is kept, without escape sequences, control characters or backticks,
  cut to 200 characters. It only fills the message field: nothing is committed until you press `Enter`.

## Preview and search limits

Files are untrusted input:

- Only regular files are read: a named pipe or a device would block the pane.
- The preview reads at most 8 MB, highlights files up to 1 MB and lines up to 2,000 characters, and dumps the first
  64 KB of a binary file.
- Images are decoded by the plugin's own code (Rust, no library), with their size checked before anything is allocated (50 MB, 50 megapixels,
  256 MB of inflated PNG or TIFF data), and every loop bounded. WebP is decoded by `dwebp` (libwebp), run on the
  absolute path of the file, for 20 s at most; its output is checked to be a PNG and decoded like one.
- Images in rendered Markdown are only local files inside the project (or the Markdown file's folder when it is
  outside the project), read like the preview's images; a URL is never fetched. The path is checked once decoded, so
  that `%5C%5Chost%5Cshare` cannot reach a network share, which on Windows would send your credentials.
- The Markdown renderer limits nested inline spans to 16 levels and drops inline markup in paragraphs over 20,000
  characters.
- Diffs are cut at 200,000 rows, and show at most 200 untracked files.
- Search skips files that are not regular, binary files and files over 2 MB, stops at 5,000 matches and walks at most
  50,000 files; symbolic link loops are cut.

## Updates

- The check is a single HTTPS request to the GitHub releases API (3 s timeout), at most once a day, from the hooks
  only, for the repository of the clone's `origin` (the plugin's when it is not on GitHub), and only a version number made of digits and dots is kept: nothing it returns reaches a command. Turn it off
  with *Check for updates*.
- The plugin installs no update and never runs git to move its folder: the explorer only copies herdr's update
  command (`herdr plugin update gcaudan.explorer`) for you to run, and a linked clone is yours to pull.
- `scripts/fetch-bin.sh` and `scripts/fetch-bin.ps1`, run by herdr's build step and the launcher,
  follow the same rule: the asset of the manifest's version, from the clone's GitHub `origin` (else
  `gcaudan/herdr-explorer`), checked against `SHA256SUMS` before it replaces anything. A binary you built
  (`bin/.dev`) is never replaced. The release workflow builds
  the binaries on GitHub's runners and writes `SHA256SUMS` from them; the checksums come from the same release as
  the binary, so they guard against a corrupted or truncated download, not against a compromised release.
- `herdr-explorer setup`, run by you or from the dialog the explorer offers once, only appends the key bindings to
  herdr's `config.toml` and, when `~/.claude` exists, links `~/.claude/skills/herdr-preview` to the plugin folder;
  at each start, the explorer repoints that link when it points at another folder. A folder of that name that is
  not a link is left alone.

## Windows

- Programs are resolved with `.exe`, never from the working directory, and the hooks path is a folder inside the
  plugin rather than `/dev/null`.
- A symbolic link to a network share (`\\host\share`, or a `\\?\` or `\\.\` path), which a repository checked out with
  `core.symlinks=true` can hold, is never followed: opening it would send your credentials to that host. The links on
  the way are read without being opened; such an entry shows as a plain file in the tree, is left out of the search
  and of `peek`, and the preview refuses it, Markdown images included. The editors, *Open in app*, *Reveal* and
  *Duplicate* refuse it too; *Rename* acts on the link itself, and *Delete* never sends it to the trash (which
  would follow it) but asks to delete the link itself.
- The clipboard and recycle bin calls run PowerShell with fixed scripts; the path or text goes through an environment
  variable, never inside the script.
- VS Code is started by running `Code.exe` on its `cli.js`, as `code.cmd` does, without `cmd`. Other editor commands
  must be `.exe` files, since `.cmd` and `.bat` launchers cannot run without a shell.
- *Insert into terminal* quotes for PowerShell and POSIX shells, not for `cmd.exe`: there, a name containing `&`,
  `|`, `<`, `>`, `^` or `%` could still run a command once `Enter` is pressed.
