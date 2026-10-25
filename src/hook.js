// Startup restore, tab events and the toggle, search and source control actions: the binary's hook (shim.js).
'use strict';
require('./shim').run(['hook', ...process.argv.slice(2)]);
