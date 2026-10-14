// The first process of an explorer or preview pane stays as a small launcher of the real one, which runs as its child:
// restarting on new code (ui.restart), the child exits and the launcher starts the script again in the same terminal.
// A process cannot replace itself on Windows, and one that waited for the next kept its memory, one more at each
// update. Required before anything else, so that the launcher loads nothing more; it stays on the code it started on.
'use strict';
const RESTART = 75; // the exit code of a child that asks to be started again

// Returns true in the launcher, whose script must stop there; false in the child.
function launch() {
  if (process.send) {
    // The launcher gone (killed), the child goes too, instead of running on in a terminal that nobody shows.
    process.on('disconnect', () => process.exit(0));
    process.channel.unref();
    return false;
  }
  const { spawn } = require('child_process');
  // Raw mode from one child to the next, never read here: in line mode, a ctrl+c typed while the next one starts
  // would kill the pane on Unix. Each child restores at exit the mode it found.
  if (process.stdin.isTTY) { process.stdin.setRawMode(true); process.stdin.pause(); }
  let env = process.env, child;
  const start = () => {
    child = spawn(process.execPath, [...process.execArgv, ...process.argv.slice(1)], { stdio: ['inherit', 'inherit', 'inherit', 'ipc'], env });
    // What the next child must show, added to the environment.
    child.on('message', m => { if (m && m.restart) env = { ...env, ...m.restart }; });
    // Killed by a signal (no code): a failure, not a quit.
    child.on('close', code => (code === RESTART ? start() : process.exit(code ?? 1)));
    child.on('error', () => process.exit(1));
  };
  for (const sig of ['SIGTERM', 'SIGHUP', 'SIGINT']) process.on(sig, () => { try { child.kill(sig); } catch { child.kill(); } });
  start();
  return true;
}

module.exports = { launch, RESTART };
