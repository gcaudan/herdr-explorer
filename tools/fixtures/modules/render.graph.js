'use strict';
// The lanes of the history graph on histories of branches, merges, octopus merges and lanes freed then reused.
const h = s => s.split(' ').map(c => { const [hash, ps = ''] = c.split(':'); return { hash, parents: ps ? ps.split(',') : [] }; });
const HISTORIES = [
  '', 'a', 'b:a a', 'f2:f1 m2:m1 f1:m1 m1:m0 m0', 'M:a,b b:a a', 'b:a c:a a',
  'M:a,b,c c:a b:a a', 'x:a y:b z:c a:r b:r c:r r', 'M:a,b N:c,d d:c b:a c:r a:r r',
  'm3:m2,f2 f2:f1 m2:m1 f1:m1 m1:m0 m0', 'a:b,c c:d b:d d:e,f f e', 'p:q,r,s,t t:u s:u r:u q:u u',
  'k1:k0 j1:j0 k0 j0', 'a:z b:z c:a,b z', 'top:mid mid:low,side side:low low', 'm:a,a a', 'b:a m:a,a,b a',
];
const cases = [];
for (const s of HISTORIES) {
  const commits = s ? h(s) : [];
  cases.push({ fn: 'layout', args: [commits] });
  for (const c of commits.slice(0, 4)) cases.push({ fn: 'layout', args: [commits, c.hash] });
}
module.exports = { module: 'render/graph', cases };
