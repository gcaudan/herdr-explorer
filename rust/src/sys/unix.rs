//! Linux and macOS: termios, ioctl, poll, a self-pipe for signals and wakes. The structs and constants differ between
//! the two (termios: `tcflag_t` 32 bits and `NCCS` 32 on Linux, 64 bits and 20 on macOS), declared per target.

use super::{Event, Signal};
use std::io;
use std::os::raw::{c_int, c_ulong, c_void};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
mod consts {
    pub type Tcflag = u32;
    pub type Speed = u32;
    pub const NCCS: usize = 32;
    pub const BRKINT: Tcflag = 0o2;
    pub const ICRNL: Tcflag = 0o400;
    pub const INPCK: Tcflag = 0o20;
    pub const ISTRIP: Tcflag = 0o40;
    pub const IXON: Tcflag = 0o2000;
    pub const ONLCR: Tcflag = 0o4;
    pub const CS8: Tcflag = 0o60;
    pub const ECHO: Tcflag = 0o10;
    pub const ICANON: Tcflag = 0o2;
    pub const IEXTEN: Tcflag = 0o100000;
    pub const ISIG: Tcflag = 0o1;
    pub const VMIN: usize = 6;
    pub const VTIME: usize = 5;
    pub const TIOCGWINSZ: std::os::raw::c_ulong = 0x5413;
    pub const TIOCSWINSZ: std::os::raw::c_ulong = 0x5414;
    pub const O_NONBLOCK: std::os::raw::c_int = 0o4000;
    pub type Nfds = std::os::raw::c_ulong;

    /// Linux's termios: c_line between the flags and c_cc.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Termios {
        pub c_iflag: Tcflag,
        pub c_oflag: Tcflag,
        pub c_cflag: Tcflag,
        pub c_lflag: Tcflag,
        pub c_line: u8,
        pub c_cc: [u8; NCCS],
        pub c_ispeed: Speed,
        pub c_ospeed: Speed,
    }
}

#[cfg(target_os = "macos")]
mod consts {
    pub type Tcflag = u64;
    pub type Speed = u64;
    pub const NCCS: usize = 20;
    pub const BRKINT: Tcflag = 0x2;
    pub const ICRNL: Tcflag = 0x100;
    pub const INPCK: Tcflag = 0x10;
    pub const ISTRIP: Tcflag = 0x20;
    pub const IXON: Tcflag = 0x200;
    pub const ONLCR: Tcflag = 0x2;
    pub const CS8: Tcflag = 0x300;
    pub const ECHO: Tcflag = 0x8;
    pub const ICANON: Tcflag = 0x100;
    pub const IEXTEN: Tcflag = 0x400;
    pub const ISIG: Tcflag = 0x80;
    pub const VMIN: usize = 16;
    pub const VTIME: usize = 17;
    pub const TIOCGWINSZ: std::os::raw::c_ulong = 0x40087468;
    pub const O_NONBLOCK: std::os::raw::c_int = 0x4;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Termios {
        pub c_iflag: Tcflag,
        pub c_oflag: Tcflag,
        pub c_cflag: Tcflag,
        pub c_lflag: Tcflag,
        pub c_cc: [u8; NCCS],
        pub c_ispeed: Speed,
        pub c_ospeed: Speed,
    }
}

use consts::*;

pub const SIGHUP: c_int = 1;
pub const SIGINT: c_int = 2;
pub const SIGTERM: c_int = 15;
const SIGWINCH: c_int = 28; // the same on Linux and macOS
const TCSADRAIN: c_int = 1;
const F_GETFD: c_int = 1;
const F_SETFD: c_int = 2;
const F_GETFL: c_int = 3;
const F_SETFL: c_int = 4;
const FD_CLOEXEC: c_int = 1;
#[cfg(target_os = "linux")]
const POLLIN: i16 = 0x1;
#[cfg(target_os = "linux")]
const POLLERR: i16 = 0x8;
#[cfg(target_os = "linux")]
const POLLHUP: i16 = 0x10;
#[cfg(target_os = "linux")]
const POLLNVAL: i16 = 0x20;
const EINTR: i32 = 4;
const EAGAIN_LINUX: i32 = 11;
const EAGAIN_MACOS: i32 = 35;

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Winsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}

unsafe extern "C" {
    fn isatty(fd: c_int) -> c_int;
    fn tcgetattr(fd: c_int, t: *mut Termios) -> c_int;
    fn tcsetattr(fd: c_int, action: c_int, t: *const Termios) -> c_int;
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    fn pipe(fds: *mut c_int) -> c_int;
    fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
    fn read(fd: c_int, buf: *mut c_void, n: usize) -> isize;
    fn write(fd: c_int, buf: *const c_void, n: usize) -> isize;
    #[cfg(target_os = "linux")]
    fn poll(fds: *mut PollFd, n: Nfds, timeout: c_int) -> c_int;
    fn signal(sig: c_int, handler: usize) -> usize;
}

fn errno() -> i32 {
    io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

pub fn stdin_is_tty() -> bool {
    // SAFETY: isatty only reads the descriptor's state.
    unsafe { isatty(0) == 1 }
}

pub fn stdout_is_tty() -> bool {
    // SAFETY: as above.
    unsafe { isatty(1) == 1 }
}

/// The size of the terminal on fd: columns and rows.
pub fn size_of(fd: c_int) -> Option<(u16, u16)> {
    let mut ws = Winsize::default();
    // SAFETY: TIOCGWINSZ writes one Winsize.
    let ok = unsafe { ioctl(fd, TIOCGWINSZ, &mut ws as *mut Winsize) } == 0;
    ok.then_some((ws.ws_col, ws.ws_row))
}

pub fn size() -> Option<(u16, u16)> {
    size_of(1)
}

pub struct Saved {
    fd: c_int,
    termios: Termios,
}

/// libuv's raw mode (uv_tty_set_mode, UV_TTY_MODE_RAW): no break, CR or parity handling, no flow control, no echo,
/// canonical mode or signals, 8 bits, NL still written as CR NL, reads of at least one byte.
pub fn raw_mode_fd(fd: c_int) -> io::Result<Saved> {
    // SAFETY: tcgetattr fills the struct, declared for this target.
    let mut t: Termios = unsafe { std::mem::zeroed() };
    if unsafe { tcgetattr(fd, &mut t) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let saved = Saved { fd, termios: t };
    t.c_iflag &= !(BRKINT | ICRNL | INPCK | ISTRIP | IXON);
    t.c_oflag |= ONLCR;
    t.c_cflag |= CS8;
    t.c_lflag &= !(ECHO | ICANON | IEXTEN | ISIG);
    t.c_cc[VMIN] = 1;
    t.c_cc[VTIME] = 0;
    // SAFETY: as above.
    if unsafe { tcsetattr(fd, TCSADRAIN, &t) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(saved)
}

pub fn raw_mode() -> io::Result<Saved> {
    raw_mode_fd(0)
}

pub fn restore(saved: &Saved) {
    // SAFETY: the struct came from tcgetattr on the same descriptor.
    unsafe { tcsetattr(saved.fd, TCSADRAIN, &saved.termios) };
}

#[cfg(test)]
/// The raw-mode flags of fd's terminal, for the tests: (input, output, control, local, VMIN, VTIME).
pub fn mode_of(fd: c_int) -> Option<(u64, u64, u64, u64, u8, u8)> {
    // SAFETY: as in raw_mode_fd.
    let mut t: Termios = unsafe { std::mem::zeroed() };
    (unsafe { tcgetattr(fd, &mut t) } == 0).then(|| {
        let raw_in = (t.c_iflag & (BRKINT | ICRNL | INPCK | ISTRIP | IXON)) as u64;
        let raw_l = (t.c_lflag & (ECHO | ICANON | IEXTEN | ISIG)) as u64;
        (raw_in, (t.c_oflag & ONLCR) as u64, (t.c_cflag & CS8) as u64, raw_l, t.c_cc[VMIN], t.c_cc[VTIME])
    })
}

// --- events

/// The self-pipe: signal handlers and wakers write one byte, the signal's number or 0 for a wake.
static PIPE: OnceLock<(c_int, c_int)> = OnceLock::new();
static PIPE_WRITE: AtomicI32 = AtomicI32::new(-1);

extern "C" fn on_signal(sig: c_int) {
    let fd = PIPE_WRITE.load(Ordering::Relaxed);
    if fd >= 0 {
        let b = sig as u8;
        // SAFETY: write is async-signal-safe; a full pipe drops the byte, the reader already has some to read.
        unsafe { write(fd, &b as *const u8 as *const c_void, 1) };
    }
}

fn self_pipe() -> io::Result<(c_int, c_int)> {
    if let Some(p) = PIPE.get() {
        return Ok(*p);
    }
    let mut fds = [0 as c_int; 2];
    // SAFETY: pipe writes two descriptors.
    if unsafe { pipe(fds.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    for fd in fds {
        // SAFETY: plain flag changes on descriptors this process owns.
        unsafe {
            fcntl(fd, F_SETFD, FD_CLOEXEC);
            let flags = fcntl(fd, F_GETFL);
            fcntl(fd, F_SETFL, flags | O_NONBLOCK);
        }
    }
    let p = *PIPE.get_or_init(|| (fds[0], fds[1]));
    PIPE_WRITE.store(p.1, Ordering::Relaxed);
    for sig in [SIGWINCH, SIGTERM, SIGHUP, SIGINT] {
        // SAFETY: the handler only writes to the pipe.
        unsafe { signal(sig, on_signal as *const () as usize) };
    }
    Ok(p)
}

#[derive(Clone)]
pub struct Waker(c_int);

impl Waker {
    pub fn wake(&self) {
        let b = 0u8;
        // SAFETY: a one-byte write to the self-pipe.
        unsafe { write(self.0, &b as *const u8 as *const c_void, 1) };
    }
}

/// Waits up to ms (forever: -1) for the pipe or the input (-1: none) to be readable; Err holds errno. Linux: poll.
#[cfg(target_os = "linux")]
fn ready(pipe: c_int, input: c_int, ms: c_int) -> Result<(bool, bool), i32> {
    let mut fds = [PollFd { fd: pipe, events: POLLIN, revents: 0 }, PollFd { fd: input, events: POLLIN, revents: 0 }];
    // SAFETY: poll reads and writes the two structs.
    if unsafe { poll(fds.as_mut_ptr(), 2, ms) } < 0 {
        return Err(errno());
    }
    // A hung up, failed or invalid input reads as its end.
    Ok((fds[0].revents & POLLIN != 0, fds[1].revents & (POLLIN | POLLHUP | POLLERR | POLLNVAL) != 0))
}

/// macOS: select, as macOS's poll does not support every terminal device (libuv and tmux avoid it there too).
#[cfg(target_os = "macos")]
fn ready(pipe: c_int, input: c_int, ms: c_int) -> Result<(bool, bool), i32> {
    let mut set = FdSet { bits: [0; 32] };
    let add = |set: &mut FdSet, fd: c_int| set.bits[fd as usize / 32] |= 1 << (fd % 32);
    let has = |set: &FdSet, fd: c_int| fd >= 0 && set.bits[fd as usize / 32] & (1 << (fd % 32)) != 0;
    if pipe >= 1024 || input >= 1024 {
        return Err(24); // EMFILE: beyond FD_SETSIZE
    }
    add(&mut set, pipe);
    if input >= 0 {
        add(&mut set, input);
    }
    let mut tv = Timeval { tv_sec: (ms / 1000) as i64, tv_usec: (ms % 1000) * 1000 };
    let tvp = if ms < 0 { std::ptr::null_mut() } else { &mut tv as *mut Timeval };
    // SAFETY: select reads and writes the set and the timeout.
    let n = unsafe { select(pipe.max(input) + 1, &mut set, std::ptr::null_mut(), std::ptr::null_mut(), tvp) };
    if n < 0 {
        return Err(errno());
    }
    Ok((has(&set, pipe), has(&set, input)))
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct FdSet {
    bits: [i32; 32],
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct Timeval {
    tv_sec: i64,
    tv_usec: i32,
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn select(n: c_int, r: *mut FdSet, w: *mut FdSet, e: *mut FdSet, timeout: *mut Timeval) -> c_int;
}

pub struct Events {
    input: c_int,
    pipe: c_int,
    wake: c_int,
    eof: bool,
}

impl Events {
    pub fn new() -> io::Result<Events> {
        Events::on(0)
    }

    /// Events with input read from fd (stdin, or a pty in the tests). A closed fd is an input already ended,
    /// checked before the self-pipe is made, which would take its number.
    pub fn on(input: c_int) -> io::Result<Events> {
        // SAFETY: F_GETFD only reads the descriptor's flags.
        let closed = unsafe { fcntl(input, F_GETFD) } < 0;
        let (r, w) = self_pipe()?;
        Ok(Events { input, pipe: r, wake: w, eof: closed })
    }

    pub fn waker(&self) -> Waker {
        Waker(self.wake)
    }

    pub fn wait(&mut self, timeout: Option<Duration>) -> Vec<Event> {
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            let ms = match deadline {
                None => -1,
                Some(d) => d.saturating_duration_since(Instant::now()).as_millis().min(i32::MAX as u128) as c_int,
            };
            let input = if self.eof { -1 } else { self.input };
            let (pipe_ready, input_ready) = match ready(self.pipe, input, ms) {
                Ok(r) => r,
                Err(e) if e == EINTR => continue,
                Err(e) => panic!("waiting for the terminal: {}", io::Error::from_raw_os_error(e)),
            };
            let mut events = Vec::new();
            if pipe_ready {
                self.drain_pipe(&mut events);
            }
            if input_ready {
                let mut buf = vec![0u8; 65536];
                // SAFETY: read writes at most buf.len() bytes.
                let got = unsafe { read(self.input, buf.as_mut_ptr() as *mut c_void, buf.len()) };
                if got > 0 {
                    buf.truncate(got as usize);
                    events.push(Event::Input(buf));
                } else if got == 0 || ![EINTR, EAGAIN_LINUX, EAGAIN_MACOS].contains(&errno()) {
                    self.eof = true;
                    events.push(Event::Eof);
                }
            }
            let elapsed = deadline.is_some_and(|d| Instant::now() >= d);
            if !events.is_empty() || elapsed {
                return events;
            }
        }
    }

    fn drain_pipe(&self, events: &mut Vec<Event>) {
        let mut buf = [0u8; 256];
        loop {
            // SAFETY: as in wait.
            let got = unsafe { read(self.pipe, buf.as_mut_ptr() as *mut c_void, buf.len()) };
            if got <= 0 {
                return;
            }
            for &b in &buf[..got as usize] {
                let e = match b as c_int {
                    0 => Event::Wake,
                    SIGWINCH => Event::Resize,
                    SIGTERM => Event::Signal(Signal::Term),
                    SIGHUP => Event::Signal(Signal::Hup),
                    SIGINT => Event::Signal(Signal::Int),
                    _ => continue,
                };
                if !events.contains(&e) {
                    events.push(e);
                }
            }
        }
    }
}

/// Pseudo-terminals, for the tests' screen captures (Linux only).
#[cfg(target_os = "linux")]
pub mod pty {
    use std::io;
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::os::raw::{c_char, c_int};

    const O_RDWR: c_int = 2;
    const O_NOCTTY: c_int = 0o400;
    const O_CLOEXEC: c_int = 0o2000000;
    pub const TIOCSCTTY: std::os::raw::c_ulong = 0x540E;

    unsafe extern "C" {
        fn posix_openpt(flags: c_int) -> c_int;
        fn grantpt(fd: c_int) -> c_int;
        fn unlockpt(fd: c_int) -> c_int;
        fn ptsname_r(fd: c_int, buf: *mut c_char, len: usize) -> c_int;
        fn open(path: *const c_char, flags: c_int, ...) -> c_int;
        pub fn setsid() -> c_int;
        pub fn ioctl(fd: c_int, request: std::os::raw::c_ulong, ...) -> c_int;
    }

    /// Sets the size of the terminal on fd, which sends SIGWINCH to its foreground process group.
    pub fn set_size(fd: c_int, cols: u16, rows: u16) -> io::Result<()> {
        let ws = super::Winsize { ws_row: rows, ws_col: cols, ..Default::default() };
        // SAFETY: TIOCSWINSZ reads one Winsize.
        if unsafe { ioctl(fd, super::TIOCSWINSZ, &ws as *const super::Winsize) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// A new pseudo-terminal: its master side and its slave side, both closed on exec.
    pub fn open_pair() -> io::Result<(OwnedFd, OwnedFd)> {
        // SAFETY: plain libc calls; each descriptor is owned once it is valid.
        unsafe {
            let master = posix_openpt(O_RDWR | O_NOCTTY | O_CLOEXEC);
            if master < 0 {
                return Err(io::Error::last_os_error());
            }
            let master = OwnedFd::from_raw_fd(master);
            use std::os::fd::AsRawFd;
            let m = master.as_raw_fd();
            let mut name = [0 as c_char; 128];
            if grantpt(m) != 0 || unlockpt(m) != 0 {
                return Err(io::Error::last_os_error());
            }
            // ptsname_r returns its error rather than setting errno.
            let err = ptsname_r(m, name.as_mut_ptr(), name.len());
            if err != 0 {
                return Err(io::Error::from_raw_os_error(err));
            }
            let slave = open(name.as_ptr(), O_RDWR | O_NOCTTY | O_CLOEXEC);
            if slave < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok((master, OwnedFd::from_raw_fd(slave)))
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;
    use std::sync::Mutex;

    unsafe extern "C" {
        fn raise(sig: c_int) -> c_int;
    }

    /// The self-pipe is the process's: the tests that read it take turns.
    static TURN: Mutex<()> = Mutex::new(());

    #[test]
    fn raw_mode_as_libuv_sets_it_and_back() {
        let (_m, s) = pty::open_pair().unwrap();
        let fd = s.as_raw_fd();
        let before = mode_of(fd).unwrap();
        let saved = raw_mode_fd(fd).unwrap();
        assert_eq!(mode_of(fd).unwrap(), (0, ONLCR as u64, CS8 as u64, 0, 1, 0));
        restore(&saved);
        assert_eq!(mode_of(fd).unwrap(), before);
    }

    #[test]
    fn size_of_a_terminal() {
        let (m, s) = pty::open_pair().unwrap();
        pty::set_size(m.as_raw_fd(), 100, 30).unwrap();
        assert_eq!(size_of(s.as_raw_fd()), Some((100, 30)));
        let mut fds = [0; 2];
        // SAFETY: test pipe.
        unsafe { pipe(fds.as_mut_ptr()) };
        assert_eq!(size_of(fds[0]), None);
    }

    #[test]
    fn input_wakes_and_signals() {
        let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
        let (m, s) = pty::open_pair().unwrap();
        raw_mode_fd(s.as_raw_fd()).unwrap();
        let mut ev = Events::on(s.as_raw_fd()).unwrap();
        // Nothing: the timeout elapses.
        assert_eq!(ev.wait(Some(Duration::from_millis(20))), vec![]);
        // SAFETY: test write to the master side.
        unsafe { write(m.as_raw_fd(), b"\x1b[Aq".as_ptr() as *const c_void, 4) };
        assert_eq!(ev.wait(Some(Duration::from_secs(2))), vec![Event::Input(b"\x1b[Aq".to_vec())]);
        let w = ev.waker();
        std::thread::spawn(move || w.wake()).join().unwrap();
        assert_eq!(ev.wait(Some(Duration::from_secs(2))), vec![Event::Wake]);
        // SAFETY: the handlers are set (Events::on): the signals reach the pipe, they do not end the test.
        unsafe {
            raise(SIGWINCH);
            raise(SIGWINCH);
        }
        assert_eq!(ev.wait(Some(Duration::from_secs(2))), vec![Event::Resize]);
        // SAFETY: as above.
        unsafe { raise(SIGHUP) };
        assert_eq!(ev.wait(Some(Duration::from_secs(2))), vec![Event::Signal(Signal::Hup)]);
        // The terminal gone: the end of the input, once.
        drop(m);
        assert_eq!(ev.wait(Some(Duration::from_secs(2))), vec![Event::Eof]);
        assert_eq!(ev.wait(Some(Duration::from_millis(20))), vec![]);
    }
}
