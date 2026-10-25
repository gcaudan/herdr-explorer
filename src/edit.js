// The Editor pane: the binary's (shim.js). Its folder is the file's without a root.
'use strict';
const path = require('path');
const file = process.env.EXPLORER_FILE;
require('./shim').pane('edit', { dir: process.env.EXPLORER_ROOT || (file && path.dirname(file)) });
