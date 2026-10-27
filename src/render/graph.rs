//! Branch graph of the history: one column of lanes on the left of each commit, as git log --graph or VS Code's
//! Git Graph draw it, but on the commit's own row (no rows of connectors in between). Commits come children first
//! (git log --date-order or --topo-order).

/// A commit's row: cells two columns per lane (a character and its lane colour, None for a blank), the commit as ●,
/// colours as lane numbers (the caller picks the colours: a lane keeps its colour from its first commit to its last);
/// above: its lane comes from a commit above (a child), below: it goes on to a parent.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub cells: Vec<(char, Option<usize>)>,
    pub color: usize,
    pub above: bool,
    pub below: bool,
}

// Makes room for lane i (with holes).
fn put<T: Clone>(lanes: &mut Vec<Option<T>>, i: usize) {
    if lanes.len() <= i {
        lanes.resize(i + 1, None);
    }
}

#[derive(Clone)]
struct Lane<'a> {
    // The hash expected next in this lane.
    hash: &'a str,
    color: usize,
}

/// The rows of commits, given as (hash, parents). base: the commit where the current branch leaves the default
/// branch, from which its lane takes another colour, so that the branch's own commits stand apart even when it is a
/// straight line ahead of the default branch: the colour of a lane joining there (the default branch's, when it went
/// on), else a new one.
pub fn layout<S: AsRef<str>>(commits: &[(S, Vec<S>)], base: Option<&str>) -> Vec<Row> {
    let mut lanes: Vec<Option<Lane>> = Vec::new();
    let mut colors = 0;
    let mut rows = Vec::new();
    for (hash, parents) in commits {
        let hash = hash.as_ref();
        let found = lanes.iter().position(|l| l.as_ref().is_some_and(|l| l.hash == hash));
        let above = found.is_some();
        // A branch tip, nothing above it: the first free lane.
        let col = found.unwrap_or_else(|| {
            let col = lanes.iter().position(Option::is_none).unwrap_or(lanes.len());
            put(&mut lanes, col);
            lanes[col] = Some(Lane { hash, color: colors });
            colors += 1;
            col
        });
        let before = lanes.clone();
        // Other lanes waiting for this commit end here: the branches that started from it.
        let mut closing = Vec::new();
        for (i, l) in before.iter().enumerate() {
            if l.as_ref().is_some_and(|l| i != col && l.hash == hash) {
                closing.push(i);
                lanes[i] = None;
            }
        }
        if above && Some(hash) == base {
            let color = match closing.first() {
                Some(&k) => before[k].as_ref().map_or(0, |l| l.color),
                None => {
                    colors += 1;
                    colors - 1
                }
            };
            if let Some(l) = lanes[col].as_mut() {
                l.color = color;
            }
        }
        let color = lanes[col].as_ref().map_or(0, |l| l.color);
        let mut rest = parents.iter().map(AsRef::as_ref);
        lanes[col] = rest.next().map(|first| Lane { hash: first, color });
        // The other parents of a merge: the lane already waiting for one, or a new lane on the right.
        let (mut joining, mut opening) = (Vec::new(), Vec::new());
        for p in rest {
            if let Some(i) = lanes.iter().position(|l| l.as_ref().is_some_and(|l| l.hash == p)) {
                joining.push(i);
                continue;
            }
            let i =
                lanes.iter().enumerate().position(|(j, l)| l.is_none() && j > col).unwrap_or(lanes.len().max(col + 1));
            put(&mut lanes, i);
            lanes[i] = Some(Lane { hash: p, color: colors });
            colors += 1;
            opening.push(i);
        }
        let n = before.len().max(lanes.len());
        let mut cells: Vec<(char, Option<usize>)> = vec![(' ', None); n * 2];
        let lane = |v: &Vec<Option<Lane>>, i: usize| v.get(i).and_then(|l| l.as_ref().map(|l| l.color));
        let lane_color = |i: usize| lane(&lanes, i).or_else(|| lane(&before, i));
        // Horizontal links from the commit to each lane it closes, joins or opens, the farthest first so that the
        // nearer ones keep their own colour.
        let mut links: Vec<usize> = closing.iter().chain(&joining).chain(&opening).copied().collect();
        links.sort_by_key(|&i| std::cmp::Reverse(i.abs_diff(col)));
        for &i in &links {
            let k = if closing.contains(&i) { lane(&before, i) } else { lane_color(i) };
            // A lane passing under the link keeps its │, drawn below.
            // Empty when i is the commit's own lane (a parent named twice).
            for cell in cells.iter_mut().take(i.max(col) * 2).skip(i.min(col) * 2 + 1) {
                *cell = ('─', k);
            }
        }
        for i in 0..n {
            let right = i > col;
            if i == col {
                cells[i * 2] = ('●', Some(color));
            } else if closing.contains(&i) {
                cells[i * 2] = (if right { '╯' } else { '╰' }, lane(&before, i));
            } else if opening.contains(&i) {
                cells[i * 2] = (if right { '╮' } else { '╭' }, lane(&lanes, i));
            } else if joining.contains(&i) {
                cells[i * 2] = (if right { '┤' } else { '├' }, lane(&lanes, i));
            } else if lane(&before, i).is_some() && lane(&lanes, i).is_some() {
                cells[i * 2] = ('│', lane(&lanes, i));
            }
        }
        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }
        // Trailing blank cells dropped: the caller pads every row to the same width.
        while cells.last().is_some_and(|c| c.0 == ' ') {
            cells.pop();
        }
        rows.push(Row { cells, color, above, below: !parents.is_empty() });
    }
    rows
}
