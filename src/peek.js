// A file in the tab's preview, from an agent's pane, a script or the peek action: the binary's peek (shim.js).
'use strict';
require('./shim').run(['peek', ...process.argv.slice(2)]);
