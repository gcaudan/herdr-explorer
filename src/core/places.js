// The places of peek.js's argument: file[:places], places separated by commas, each a line (12), lines (12-14), or
// columns of a line (12:5-18, counted in characters from 1); 12:5 is line 12, as compilers and grep write it.
'use strict';
const fs = require('fs');
const path = require('path');

const PLACE = /^(\d+)(?:-(\d+))?(?::(\d+)(?:-(\d+))?)?$/;

// A name that exists as given is taken whole, so that a colon in it is not read as places, and so is one whose tail
// does not read as places. Returns [file, marks]; throws on a place that reads as one but cannot be (12-3, 0, 12:9-5).
function target(arg, cwd) {
  const whole = path.resolve(cwd, arg);
  const at = arg.indexOf(':', /^[a-z]:[\\/]/i.test(arg) ? 2 : 0); // past a Windows drive
  if (fs.existsSync(whole) || at < 1) return [whole, []];
  const marks = [];
  for (const place of arg.slice(at + 1).split(',')) {
    const m = PLACE.exec(place);
    if (!m) return [whole, []];
    const from = +m[1], to = m[2] ? +m[2] : from;
    // Columns only on one line, a range of them only (a single column is a position, not a part).
    const cols = !m[2] && m[4];
    if (!from || to < from || (cols && (!+m[3] || +m[4] < +m[3]))) throw new Error(`wrong place: ${place}`);
    marks.push({ from, to, a: cols ? +m[3] : 0, b: cols ? +m[4] : 0 });
  }
  return [path.resolve(cwd, arg.slice(0, at)), marks];
}

module.exports = { target };
