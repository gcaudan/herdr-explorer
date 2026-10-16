# Security

The explorer opens by itself on any folder, including repositories an agent has just cloned and nobody has reviewed.
It runs git, reads files and writes to your terminal in those folders. This document lists what it does so that a
hostile repository cannot run code, leak credentials or take over your terminal through it.

## Running programs

- **No shell**. Every program runs with an argument list (`execFile`, `spawn`), never through a shell command line.
  The one exception is an npm-installed `claude.cmd` on Windows, which can only run through `cmd`, with fixed
  arguments.
- **Absolute paths**. `git`, `herdr`, `claude`, `dwebp`, a graphical editor command, and on Windows every program, run from
  their absolute path, looked up in the absolute folders of `PATH` only. On Windows, a bare program name is first
  looked up in the working directory, and elsewhere an empty or `.` entry of `PATH` means that folder too: a
  repository could plant a `git` there. On Linux and macOS, the clipboard, trash and file manager tools and the
  fallback terminal editor go through the usual `PATH` lookup.
- **Output limits**. Program output is read up to 64 MB.

## Git hardening

Every git call in a repository gets an environment built for that repository, passed as
`GIT_CONFIG_KEY_n`/`GIT_CONFIG_VALUE_n` (unlike `-c`, this does not split a driver name containing `=`). It turns
off what the repository's config could use to run commands:

- **Hooks**: `core.hooksPath` points at a folder that never exists (`/dev/null`, or `.no-hooks` in the plugin folder
  on Windows, since `C:\dev\null` could be created by any user), and each `hook.<name>.command` the repository
  declares in its config is disabled. No hook file runs, those of a global hooks folder included, so commits skip
  pre-commit, commit-msg and the others.
- **`core.fsmonitor`** is off.
- **Filters**: each filter driver the repository declares (in `.git/config`, `config.worktree` or an included file)
  gets empty `clean`, `smudge` and `process` commands and `required=false`. Filters from the global and system
  config, such as git-lfs, are kept.
- **Submodules** are ignored (`--ignore-submodules=all`) and never recursed into (switch, restore, fetch, pull,
  push), since their own config is out of reach.
- **Signatures**: `log.showSignature=false`, so the repository's `gpg.program` does not run on signed commits.
- **Lazy fetching**: `GIT_NO_LAZY_FETCH=1`, so a partial clone never fetches a missing object from its promisor
  remote, which would run its `uploadpack` or ssh command, even during `git status`.
- **Working tree**: a `core.worktree` set by the repository is ignored. The working tree stays the folder holding
  `.git`, so switching branch, pulling or discarding cannot write elsewhere, such as your home folder.
- **Locks**: status runs with `GIT_OPTIONAL_LOCKS=0`.

### Commands that write (source control)

Stage, commit, switch, stash, fetch, pull and push also replace every program-running key the repository sets
itself with your global value, or git's default: `core.sshCommand`, `core.askPass`, `gpg.*.program`,
`gpg.ssh.defaultKeyCommand`, credential helpers and merge drivers. `remote.*.uploadpack` and `receivepack` are
overridden on the command line, and `core.gitProxy` is ignored (`GIT_PROXY_COMMAND` emptied).

### Network (fetch, pull, push)

- **Transports**: only `https`, `http`, `ssh` and `git` (`GIT_ALLOW_PROTOCOL`). No `ext::` command, no local remote
  (it would run its own hooks), no remote helper (`git-remote-<name>`).
- **Refused settings**: fetch, pull and push are refused when the repository's own config sets `http.*`,
  `url.*.insteadOf`, `url.*.pushInsteadOf` or `remote.*.proxy` (which could send your credentials elsewhere),
  `core.alternateRefsCommand` (a command git runs during a fetch) or `remote.*.vcs` (a helper program that can live
  in the repository). Run the command in a terminal after checking them.
- **No prompt**: `GIT_TERMINAL_PROMPT=0` and a 120 s timeout. On Linux and macOS, network commands, commits and
  *Continue* run in a process group detached from the terminal, so ssh, gpg or a credential helper cannot take over
  the pane to ask for a password; the whole group is killed on timeout.

### Continue

Before `--continue`, source control refuses to go on when the rebase todo contains `exec` lines, or when the merge
strategy is not one of git's own (`ort`, `recursive`, `resolve`, `octopus`, `ours`, `subtree`), or when those cannot
be checked. It reads the strategy from `rebase-merge/strategy`, `sequencer/opts` and `pull.twohead`. `GIT_EDITOR=:`
keeps the prepared message, so no editor (and no `core.editor` the repository sets) opens.

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
- **Discard**: source control deletes untracked files only when they resolve inside the repository; the explorer's
  *Discard changes* sends them to the trash instead.
- **Opening files**: a double-click opens with the system application only documents and media (images, PDF,
  Office, archives, audio, video); everything else goes to the preview or an editor, so it never starts an
  executable, a script or a shortcut. *Open in app* adds only `txt`, `md`, `csv`, `log`, `json`, `xml`, `html` and
  `htm`. The extension decides, not the execute bit (set on every file of FAT and NTFS drives).

## Terminal output

- **Control characters** (C0, DEL, C1) in file names, file contents, git output and diffs are never sent to the
  terminal: they are replaced or dropped, and tabs are expanded. The preview, the search and the diffs colour the
  text themselves, so a file cannot move the cursor, set the title or hide content with escape sequences.
- **Paste**: pasted text has its control characters removed, and a long paste is held until its end marker, so its
  text is never read as keys.
- **Insert into terminal** refuses names containing control characters (a newline would run the command or submit
  the prompt), prefixes names starting with `-` with `./`, and puts the others in single quotes for the shell when
  they hold anything but plain characters (quoting valid in POSIX shells and in PowerShell, where typographic quotes
  are escaped too). For an agent, a path with spaces is quoted, so its end is clear and the rest cannot read as
  instructions.
- **Review notes** are typed on one line, without submitting it: control characters are removed from the note texts
  when they are saved and again when they are sent, and the line is refused if one remains. The text goes into the
  agent's prompt as you typed it, so a note asking for something is a request like any other you type there.
- **Kitty graphics**: the preview sends images as pixels it decoded itself, never a file path for the terminal to
  read; the terminal's answers to its queries are read apart from the keys. **Settings → Images** can keep them pixelated,
  drawn with characters only. The link under a pixelated image opens one fixed `https://` page of
  this documentation in the browser, never an address taken from a file.
- **`peek.js`** shows, in the preview, a regular file the caller names: anything the user can read, as the preview
  already does, never a folder or a device. An agent calling it changes what you see, not what is run. It writes the
  request file of the state folder only, whose path it reads from `state-dir` in the plugin's config folder.

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
- Images are decoded in JavaScript, with their size checked before anything is allocated (50 MB, 50 megapixels,
  256 MB of inflated PNG or TIFF data), and every loop bounded. WebP is decoded by `dwebp` (libwebp), run on the
  absolute path of the file, for 20 s at most; its output is checked to be a PNG and decoded like one.
- Images in rendered Markdown are only local files, read like the preview's images; a URL is never fetched.
- The Markdown renderer limits nested inline spans to 16 levels and drops inline markup in paragraphs over 20,000
  characters.
- Search skips files that are not regular, binary files and files over 2 MB, stops at 5,000 matches and walks at most
  50,000 files; symbolic link loops are cut.
- Lookup tables have no prototype, so a file named `constructor` is just a name.

## Updates

- The check is a single HTTPS request to the GitHub releases API (3 s timeout), at most once a day, from the hooks
  only, and only a version number made of digits and dots is kept: nothing it returns reaches a command. Turn it off
  with *Check for updates*.
- The update only runs `git pull --ff-only --no-rebase` in the plugin's own clone (its git top level must be the
  plugin folder, not a dotfiles repository holding a copy), on `main`, with no operation in progress and no local
  changes, under the same no-prompt rules (`GIT_TERMINAL_PROMPT=0`, `GCM_INTERACTIVE=never`, detached, ssh in
  `BatchMode` unless you set your own ssh command).
- A checkout installed by `herdr plugin install` is never pulled: the plugin copies herdr's update command instead.
- The installer is never run by the update. When an update changes the manifest or the install scripts, the plugin
  only copies the command for you to run.

## Windows

- Programs are resolved with `.exe`, never from the working directory, and the hooks path is a folder inside the
  plugin rather than `/dev/null`.
- The clipboard and recycle bin calls run PowerShell with fixed scripts; the path or text goes through an environment
  variable, never inside the script.
- VS Code is started by running `Code.exe` on its `cli.js`, as `code.cmd` does, without `cmd`. Other editor commands
  must be `.exe` files, since `.cmd` and `.bat` launchers cannot run without a shell.
- *Insert into terminal* quotes for PowerShell and POSIX shells, not for `cmd.exe`: there, a name containing `&`,
  `|`, `<`, `>`, `^` or `%` could still run a command once `Enter` is pressed.
