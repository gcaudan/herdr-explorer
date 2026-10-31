//! A minimal terminal emulator for the screen captures: what a pane wrote, played on a grid of cells (text and SGR
//! attributes), then written out as text. Recorded and new captures go through the same emulator, so it only needs to
//! be deterministic and to keep what a user sees: cursor moves, erases, scrolling, alternate screen, colours. Queries,
//! OSC and DCS are skipped; the Kitty graphics commands (APC) are listed, without their data, which depends on the
//! compressor.

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Color {
    #[default]
    Default,
    Idx(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Style {
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
    pub strike: bool,
}

impl Style {
    /// The style written in a capture: `fg=… bg=… b d i u r s`, empty for the default.
    fn text(&self) -> String {
        let color = |c: Color| match c {
            Color::Default => String::new(),
            Color::Idx(n) => n.to_string(),
            Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        };
        let mut parts = Vec::new();
        if self.fg != Color::Default {
            parts.push(format!("fg={}", color(self.fg)));
        }
        if self.bg != Color::Default {
            parts.push(format!("bg={}", color(self.bg)));
        }
        for (on, s) in [
            (self.bold, "b"),
            (self.dim, "d"),
            (self.italic, "i"),
            (self.underline, "u"),
            (self.reverse, "r"),
            (self.strike, "s"),
        ] {
            if on {
                parts.push(s.into());
            }
        }
        parts.join(" ")
    }
}

#[derive(Clone, PartialEq, Debug)]
struct Cell {
    text: String, // empty: the second column of a wide character
    style: Style,
}

impl Cell {
    fn blank(style: Style) -> Cell {
        Cell { text: " ".into(), style: Style { bg: style.bg, ..Style::default() } }
    }
}

#[derive(Default)]
enum State {
    #[default]
    Ground,
    Esc,
    Csi(String),
    /// ESC ( ) * + # %: the next character designates a charset or a line size, never shown.
    Designate,
    /// OSC, DCS, APC, PM, SOS: skipped up to BEL (OSC) or ESC \; an APC's control part (up to ;) kept.
    Str {
        esc: bool,
        apc: Option<String>,
    },
}

type Grid = Vec<Vec<Cell>>;

pub struct Screen {
    w: usize,
    h: usize,
    grid: Vec<Vec<Cell>>,
    /// The main screen and its cursor, while the alternate one shows.
    main: Option<(Grid, (usize, usize))>,
    x: usize,
    y: usize,
    saved: (usize, usize),
    style: Style,
    wrap: bool,
    pending_wrap: bool,
    top: usize,
    bottom: usize,
    state: State,
    utf8: Vec<u8>,
    /// The Kitty graphics commands, where the cursor was: a=q queries and the data's later chunks left out.
    graphics: Vec<String>,
}

/// Columns of a character: 0 for combining marks and joiners, 2 for wide ones (East Asian wide, emoji), else 1. A
/// rough table: both captures use it.
fn width(c: char) -> usize {
    let u = c as u32;
    match u {
        0x0300..=0x036F | 0x200B..=0x200F | 0xFE00..=0xFE0F | 0x20D0..=0x20FF => 0,
        0x1100..=0x115F
        | 0x2E80..=0x303E
        | 0x3041..=0x33FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1F64F
        | 0x1F900..=0x1F9FF
        | 0x20000..=0x3FFFD => 2,
        _ => 1,
    }
}

impl Screen {
    pub fn new(w: usize, h: usize) -> Screen {
        Screen {
            w,
            h,
            grid: vec![vec![Cell::blank(Style::default()); w]; h],
            main: None,
            x: 0,
            y: 0,
            saved: (0, 0),
            style: Style::default(),
            wrap: true,
            pending_wrap: false,
            top: 0,
            bottom: h - 1,
            state: State::Ground,
            utf8: Vec::new(),
            graphics: Vec::new(),
        }
    }

    /// The terminal resized: the grid cut or padded, as most terminals do on the alternate screen.
    pub fn resize(&mut self, w: usize, h: usize) {
        for row in &mut self.grid {
            row.resize(w, Cell::blank(Style::default()));
        }
        self.grid.resize(h, vec![Cell::blank(Style::default()); w]);
        self.w = w;
        self.h = h;
        self.top = 0;
        self.bottom = h - 1;
        self.x = self.x.min(w - 1);
        self.y = self.y.min(h - 1);
        self.saved = (self.saved.0.min(w - 1), self.saved.1.min(h - 1));
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.utf8.push(b);
            match std::str::from_utf8(&self.utf8) {
                Ok(s) => {
                    let c = s.chars().next().unwrap();
                    self.utf8.clear();
                    self.char(c);
                }
                Err(e) if e.error_len().is_some() => {
                    self.utf8.clear();
                    self.char('\u{FFFD}');
                }
                Err(_) => {}
            }
        }
    }

    fn char(&mut self, c: char) {
        match std::mem::take(&mut self.state) {
            State::Ground => self.ground(c),
            State::Esc => self.esc(c),
            State::Designate => {}
            State::Csi(mut params) => {
                if ('\x40'..='\x7e').contains(&c) {
                    self.csi(&params, c);
                } else {
                    params.push(c);
                    self.state = State::Csi(params);
                }
            }
            State::Str { esc, mut apc } => {
                if c == '\x07' || (esc && c == '\\') {
                    return self.apc(apc);
                }
                if let Some(a) = apc.as_mut().filter(|a| !a.ends_with(';') && c != '\x1b') {
                    a.push(c);
                }
                self.state = State::Str { esc: c == '\x1b', apc };
            }
        }
    }

    /// A Kitty graphics command listed: its keys but m (whether more data follows), and the cursor's row and column.
    fn apc(&mut self, apc: Option<String>) {
        let Some(control) = apc.as_deref().and_then(|a| a.strip_prefix('G')) else { return };
        let keys: Vec<&str> = control.trim_end_matches(';').split(',').collect();
        if keys.iter().all(|k| !k.starts_with("a=")) || keys.contains(&"a=q") {
            return;
        }
        let kept: Vec<&str> = keys.into_iter().filter(|k| !k.starts_with("m=")).collect();
        self.graphics.push(format!("{} @{};{}", kept.join(","), self.y + 1, self.x + 1));
    }

    fn ground(&mut self, c: char) {
        match c {
            '\x1b' => self.state = State::Esc,
            '\r' => {
                self.x = 0;
                self.pending_wrap = false;
            }
            '\n' | '\x0b' | '\x0c' => {
                self.pending_wrap = false;
                self.line_feed();
            }
            '\x08' => {
                self.pending_wrap = false;
                self.x = self.x.saturating_sub(1);
            }
            '\t' => self.x = ((self.x / 8 + 1) * 8).min(self.w - 1),
            c if (c as u32) < 0x20 || c == '\x7f' => {}
            c => self.print(c),
        }
    }

    fn print(&mut self, c: char) {
        let cw = width(c);
        if cw == 0 {
            // Joined to the character before.
            let (x, y) = (if self.pending_wrap { self.x } else { self.x.saturating_sub(1) }, self.y);
            let x = if self.grid[y][x].text.is_empty() && x > 0 { x - 1 } else { x };
            self.grid[y][x].text.push(c);
            return;
        }
        if self.pending_wrap {
            self.pending_wrap = false;
            if self.wrap {
                self.x = 0;
                self.line_feed();
            }
        }
        if cw == 2 && self.x + 1 >= self.w {
            if !self.wrap {
                return;
            }
            self.x = 0;
            self.line_feed();
        }
        let (x, y) = (self.x, self.y);
        self.clear_wide(x, y);
        self.grid[y][x] = Cell { text: c.to_string(), style: self.style };
        if cw == 2 {
            self.clear_wide(x + 1, y);
            self.grid[y][x + 1] = Cell { text: String::new(), style: self.style };
        }
        if x + cw >= self.w {
            self.x = self.w - 1;
            self.pending_wrap = true;
        } else {
            self.x = x + cw;
        }
    }

    /// Before a cell is written: a wide character it was part of loses its other half.
    fn clear_wide(&mut self, x: usize, y: usize) {
        if self.grid[y][x].text.is_empty() && x > 0 {
            self.grid[y][x - 1] = Cell::blank(Style::default());
        }
        if x + 1 < self.w && self.grid[y][x + 1].text.is_empty() {
            self.grid[y][x + 1] = Cell::blank(Style::default());
        }
    }

    fn line_feed(&mut self) {
        if self.y == self.bottom {
            self.scroll_up(1);
        } else if self.y + 1 < self.h {
            self.y += 1;
        }
    }

    fn scroll_up(&mut self, n: usize) {
        for _ in 0..n {
            self.grid.remove(self.top);
            self.grid.insert(self.bottom, vec![Cell::blank(self.style); self.w]);
        }
    }

    fn scroll_down(&mut self, n: usize) {
        for _ in 0..n {
            self.grid.remove(self.bottom);
            self.grid.insert(self.top, vec![Cell::blank(self.style); self.w]);
        }
    }

    fn esc(&mut self, c: char) {
        match c {
            '[' => self.state = State::Csi(String::new()),
            '_' => self.state = State::Str { esc: false, apc: Some(String::new()) },
            ']' | 'P' | '^' | 'X' => self.state = State::Str { esc: false, apc: None },
            '(' | ')' | '*' | '+' | '#' | '%' => self.state = State::Designate,
            '7' => self.saved = (self.x, self.y),
            '8' => (self.x, self.y) = self.saved,
            'M' => {
                if self.y == self.top {
                    self.scroll_down(1);
                } else {
                    self.y = self.y.saturating_sub(1);
                }
            }
            'D' => self.line_feed(),
            'E' => {
                self.x = 0;
                self.line_feed();
            }
            'c' => *self = Screen::new(self.w, self.h),
            _ => {} // charset designations and the like
        }
    }

    fn erase(&mut self, y: usize, from: usize, to: usize) {
        for x in from..to.min(self.w) {
            self.grid[y][x] = Cell::blank(self.style);
        }
    }

    fn csi(&mut self, params: &str, f: char) {
        let private = params.starts_with(['?', '>', '<', '=']);
        let body = params.trim_start_matches(['?', '>', '<', '=']);
        // Intermediate bytes (space, $, " …) make another command: skipped.
        if body.chars().any(|c| (' '..='/').contains(&c)) {
            return;
        }
        let nums: Vec<usize> = body.split([';', ':']).map(|n| n.parse().unwrap_or(0)).collect();
        let n = |i: usize, d: usize| nums.get(i).copied().filter(|v| *v != 0).unwrap_or(d);
        if f != 'm' {
            self.pending_wrap = false;
        }
        match (private, f) {
            (false, 'H' | 'f') => {
                self.y = (n(0, 1) - 1).min(self.h - 1);
                self.x = (n(1, 1) - 1).min(self.w - 1);
            }
            (false, 'A') => self.y = self.y.saturating_sub(n(0, 1)),
            (false, 'B' | 'e') => self.y = (self.y + n(0, 1)).min(self.h - 1),
            (false, 'C' | 'a') => self.x = (self.x + n(0, 1)).min(self.w - 1),
            (false, 'D') => self.x = self.x.saturating_sub(n(0, 1)),
            (false, 'E') => {
                self.y = (self.y + n(0, 1)).min(self.h - 1);
                self.x = 0;
            }
            (false, 'F') => {
                self.y = self.y.saturating_sub(n(0, 1));
                self.x = 0;
            }
            (false, 'G' | '`') => self.x = (n(0, 1) - 1).min(self.w - 1),
            (false, 'd') => self.y = (n(0, 1) - 1).min(self.h - 1),
            (false, 'J') => match nums.first().copied().unwrap_or(0) {
                0 => {
                    let (x, y) = (self.x, self.y);
                    self.erase(y, x, self.w);
                    for r in y + 1..self.h {
                        self.erase(r, 0, self.w);
                    }
                }
                1 => {
                    let (x, y) = (self.x, self.y);
                    for r in 0..y {
                        self.erase(r, 0, self.w);
                    }
                    self.erase(y, 0, x + 1);
                }
                _ => {
                    for r in 0..self.h {
                        self.erase(r, 0, self.w);
                    }
                }
            },
            (false, 'K') => {
                let (x, y) = (self.x, self.y);
                match nums.first().copied().unwrap_or(0) {
                    0 => self.erase(y, x, self.w),
                    1 => self.erase(y, 0, x + 1),
                    _ => self.erase(y, 0, self.w),
                }
            }
            (false, 'X') => {
                let (x, y) = (self.x, self.y);
                self.erase(y, x, x + n(0, 1));
            }
            (false, '@') => {
                let (x, y) = (self.x, self.y);
                for _ in 0..n(0, 1).min(self.w - x) {
                    self.grid[y].pop();
                    self.grid[y].insert(x, Cell::blank(self.style));
                }
            }
            (false, 'P') => {
                let (x, y) = (self.x, self.y);
                for _ in 0..n(0, 1).min(self.w - x) {
                    self.grid[y].remove(x);
                    self.grid[y].push(Cell::blank(self.style));
                }
            }
            (false, 'L') if (self.top..=self.bottom).contains(&self.y) => {
                let top = self.top;
                self.top = self.y;
                self.scroll_down(n(0, 1));
                self.top = top;
            }
            (false, 'M') if (self.top..=self.bottom).contains(&self.y) => {
                let top = self.top;
                self.top = self.y;
                self.scroll_up(n(0, 1));
                self.top = top;
            }
            (false, 'S') => self.scroll_up(n(0, 1)),
            (false, 'T') => self.scroll_down(n(0, 1)),
            (false, 'r') => {
                self.top = (n(0, 1) - 1).min(self.h - 1);
                self.bottom = (n(1, self.h) - 1).min(self.h - 1).max(self.top);
                self.x = 0;
                self.y = 0;
            }
            (false, 's') => self.saved = (self.x, self.y),
            (false, 'u') => (self.x, self.y) = self.saved,
            (false, 'm') => self.sgr(body),
            (true, 'h' | 'l') if params.starts_with('?') => {
                let on = f == 'h';
                for &mode in &nums {
                    match mode {
                        7 => self.wrap = on,
                        47 | 1047 | 1049 => self.alternate(on, mode == 1049),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn alternate(&mut self, on: bool, cursor: bool) {
        let blank = vec![vec![Cell::blank(Style::default()); self.w]; self.h];
        if on && self.main.is_none() {
            let main = std::mem::replace(&mut self.grid, blank);
            self.main = Some((main, (self.x, self.y)));
        } else if !on && let Some((main, at)) = self.main.take() {
            self.grid = main;
            for row in &mut self.grid {
                row.resize(self.w, Cell::blank(Style::default()));
            }
            self.grid.resize(self.h, vec![Cell::blank(Style::default()); self.w]);
            if cursor {
                (self.x, self.y) = (at.0.min(self.w - 1), at.1.min(self.h - 1));
            }
        }
    }

    fn sgr(&mut self, body: &str) {
        // A colon form (38:2::r:g:b, 38:5:n) is one group; the semicolon form spreads the colour over the next ones.
        let groups: Vec<Vec<usize>> =
            body.split(';').map(|g| g.split(':').map(|n| n.parse().unwrap_or(0)).collect()).collect();
        let colon = |g: &[usize]| match g.get(1) {
            Some(5) => Color::Idx(g.get(2).copied().unwrap_or(0) as u8),
            Some(2) => {
                let rgb = if g.len() >= 6 { &g[g.len() - 3..] } else { &g[2.min(g.len())..] };
                let c = |k: usize| rgb.get(k).copied().unwrap_or(0) as u8;
                Color::Rgb(c(0), c(1), c(2))
            }
            _ => Color::Default,
        };
        let mut nums = Vec::new();
        for g in &groups {
            if g.len() > 1 && matches!(g[0], 38 | 48 | 58) {
                // In order with the rest: what came before first.
                self.sgr_flat(&std::mem::take(&mut nums));
                let c = colon(g);
                match g[0] {
                    38 => self.style.fg = c,
                    48 => self.style.bg = c,
                    _ => {}
                }
                continue;
            }
            nums.push(g[0]);
        }
        self.sgr_flat(&nums);
    }

    fn sgr_flat(&mut self, nums: &[usize]) {
        if nums.is_empty() {
            return;
        }
        let mut i = 0;
        while i < nums.len() {
            let s = &mut self.style;
            match nums[i] {
                0 => *s = Style::default(),
                1 => s.bold = true,
                2 => s.dim = true,
                3 => s.italic = true,
                4 => s.underline = true,
                7 => s.reverse = true,
                9 => s.strike = true,
                21 | 22 => {
                    s.bold = false;
                    s.dim = false;
                }
                23 => s.italic = false,
                24 => s.underline = false,
                27 => s.reverse = false,
                29 => s.strike = false,
                n @ 30..=37 => s.fg = Color::Idx((n - 30) as u8),
                n @ 90..=97 => s.fg = Color::Idx((n - 90 + 8) as u8),
                n @ 40..=47 => s.bg = Color::Idx((n - 40) as u8),
                n @ 100..=107 => s.bg = Color::Idx((n - 100 + 8) as u8),
                39 => s.fg = Color::Default,
                49 => s.bg = Color::Default,
                n @ (38 | 48 | 58) => {
                    let color = match nums.get(i + 1) {
                        Some(5) => {
                            i += 2;
                            Color::Idx(nums.get(i).copied().unwrap_or(0) as u8)
                        }
                        Some(2) => {
                            let c = |k| nums.get(i + k).copied().unwrap_or(0) as u8;
                            let rgb = Color::Rgb(c(2), c(3), c(4));
                            i += 4;
                            rgb
                        }
                        _ => Color::Default,
                    };
                    match n {
                        38 => s.fg = color,
                        48 => s.bg = color,
                        _ => {} // underline colour: not kept
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    /// The screen as text: one line per row, a style change written ⟪style⟫ where it happens (⟪⟫: back to the
    /// default), the default-styled blanks at the end of a row left out.
    pub fn text(&self) -> String {
        let mut out = format!("size {}x{}\n", self.w, self.h);
        for row in &self.grid {
            let end = row.iter().rposition(|c| c.text != " " || c.style != Style::default()).map_or(0, |i| i + 1);
            let mut style = Style::default();
            let mut line = String::new();
            for cell in &row[..end] {
                if cell.style != style {
                    style = cell.style;
                    line.push_str(&format!("⟪{}⟫", style.text()));
                }
                line.push_str(&cell.text);
            }
            if style != Style::default() {
                line.push_str("⟪⟫");
            }
            out.push_str(&line);
            out.push('\n');
        }
        if !self.graphics.is_empty() {
            out.push_str("graphics\n");
            for g in &self.graphics {
                out.push_str(g);
                out.push('\n');
            }
        }
        out
    }
}
