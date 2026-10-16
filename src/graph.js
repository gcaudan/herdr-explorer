// Branch graph of the history: one column of lanes on the left of each commit, as git log --graph or VS Code's
// Git Graph draw it, but on the commit's own row (no rows of connectors in between). Commits come children first
// (git log --date-order or --topo-order).
'use strict';

// Lanes of each commit: { cells: [[char, color]], color, above, below }, cells two columns per lane, the commit
// as ●, colors as lane numbers (the caller picks the colours: a lane keeps its colour from its first commit to its
// last); above: its lane comes from a commit above (a child), below: it goes on to a parent. commits: [{ hash,
// parents: [hash] }].
function layout(commits) {
  const lanes = []; // { hash expected next in this lane, color }
  let colors = 0;
  return commits.map(c => {
    let col = lanes.findIndex(l => l && l.hash === c.hash);
    const above = col >= 0;
    // A branch tip, nothing above it: the first free lane.
    if (col < 0) {
      col = lanes.findIndex(l => !l);
      if (col < 0) col = lanes.length;
      lanes[col] = { hash: c.hash, color: colors++ };
    }
    const before = lanes.slice(), color = lanes[col].color;
    // Other lanes waiting for this commit end here: the branches that started from it.
    const closing = [];
    before.forEach((l, i) => { if (l && i !== col && l.hash === c.hash) { closing.push(i); lanes[i] = null; } });
    const [first, ...rest] = c.parents;
    lanes[col] = first ? { hash: first, color } : null;
    // The other parents of a merge: the lane already waiting for one, or a new lane on the right.
    const joining = [], opening = [];
    for (const p of rest) {
      let i = lanes.findIndex(l => l && l.hash === p);
      if (i >= 0) { joining.push(i); continue; }
      i = lanes.findIndex((l, j) => !l && j > col);
      if (i < 0) i = Math.max(lanes.length, col + 1);
      lanes[i] = { hash: p, color: colors++ };
      opening.push(i);
    }
    const n = Math.max(before.length, lanes.length);
    const cells = Array.from({ length: n * 2 }, () => [' ', null]);
    const laneColor = i => (lanes[i] || before[i]).color;
    // Horizontal links from the commit to each lane it closes, joins or opens, the farthest first so that the
    // nearer ones keep their own colour.
    const links = [...closing, ...joining, ...opening].sort((a, b) => Math.abs(b - col) - Math.abs(a - col));
    for (const i of links) {
      const k = closing.includes(i) ? before[i].color : laneColor(i);
      // A lane passing under the link keeps its │, drawn below.
      for (let x = Math.min(i, col) * 2 + 1; x < Math.max(i, col) * 2; x++) cells[x] = ['─', k];
    }
    for (let i = 0; i < n; i++) {
      const right = i > col;
      if (i === col) cells[i * 2] = ['●', color];
      else if (closing.includes(i)) cells[i * 2] = [right ? '╯' : '╰', before[i].color];
      else if (opening.includes(i)) cells[i * 2] = [right ? '╮' : '╭', lanes[i].color];
      else if (joining.includes(i)) cells[i * 2] = [right ? '┤' : '├', lanes[i].color];
      else if (before[i] && lanes[i]) cells[i * 2] = ['│', lanes[i].color];
    }
    while (lanes.length && !lanes[lanes.length - 1]) lanes.pop();
    // Trailing blank cells dropped: the caller pads every row to the same width.
    while (cells.length && cells[cells.length - 1][0] === ' ') cells.pop();
    return { cells, color, above, below: c.parents.length > 0 };
  });
}

module.exports = { layout };
