//! Windows: the console. A thread reads the console's input records (keys as VT sequences, resizes) and posts them,
//! with the console's control events and the wakes, to a queue the main thread waits on through an event object.

use super::{Event, Signal};
use std::io;
use std::os::raw::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

type Handle = *mut c_void;
type Bool = i32;

const STD_INPUT_HANDLE: u32 = -10i32 as u32;
const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
const ENABLE_WINDOW_INPUT: u32 = 0x8;
const ENABLE_VIRTUAL_TERMINAL_INPUT: u32 = 0x200;
const ENABLE_PROCESSED_OUTPUT: u32 = 0x1;
const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x4;
const KEY_EVENT: u16 = 0x1;
const WINDOW_BUFFER_SIZE_EVENT: u16 = 0x4;
const WAIT_OBJECT_0: u32 = 0;
const WAIT_FAILED: u32 = 0xFFFF_FFFF;
const INFINITE: u32 = 0xFFFF_FFFF;
const CTRL_C_EVENT: u32 = 0;
const CTRL_CLOSE_EVENT: u32 = 2;

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Coord {
    x: i16,
    y: i16,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct SmallRect {
    left: i16,
    top: i16,
    right: i16,
    bottom: i16,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct ScreenBufferInfo {
    size: Coord,
    cursor: Coord,
    attributes: u16,
    window: SmallRect,
    max_window: Coord,
}

/// INPUT_RECORD: the event's kind, then a 16-byte union read field by field.
#[repr(C)]
#[derive(Clone, Copy)]
struct InputRecord {
    kind: u16,
    _pad: u16,
    data: [u16; 8],
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetStdHandle(which: u32) -> Handle;
    fn GetConsoleMode(h: Handle, mode: *mut u32) -> Bool;
    fn SetConsoleMode(h: Handle, mode: u32) -> Bool;
    fn GetConsoleScreenBufferInfo(h: Handle, info: *mut ScreenBufferInfo) -> Bool;
    fn ReadConsoleInputW(h: Handle, records: *mut InputRecord, len: u32, read: *mut u32) -> Bool;
    fn ReadFile(h: Handle, buf: *mut c_void, len: u32, read: *mut u32, overlapped: *mut c_void) -> Bool;
    fn CreateEventW(attrs: *mut c_void, manual: Bool, initial: Bool, name: *const u16) -> Handle;
    fn SetEvent(h: Handle) -> Bool;
    fn WaitForSingleObject(h: Handle, ms: u32) -> u32;
    fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32) -> Bool>, add: Bool) -> Bool;
    fn GetComputerNameExW(kind: u32, buf: *mut u16, len: *mut u32) -> Bool;
    fn GetFileInformationByHandle(h: Handle, info: *mut FileInfo) -> Bool;
    fn OpenProcess(access: u32, inherit: Bool, pid: u32) -> Handle;
    fn CloseHandle(h: Handle) -> Bool;
}

/// Waits for the process `pid` to end: false at once when it cannot be opened (gone already, or not this user's).
pub fn wait_process(pid: u32) -> bool {
    const SYNCHRONIZE: u32 = 0x0010_0000;
    // SAFETY: a handle opened here, waited on, then closed; null when the process cannot be opened.
    unsafe {
        let h = OpenProcess(SYNCHRONIZE, 0, pid);
        if h.is_null() {
            return false;
        }
        WaitForSingleObject(h, INFINITE);
        CloseHandle(h);
    }
    true
}

/// BY_HANDLE_FILE_INFORMATION: attributes, three times, the volume, the size, the link count, the file's index.
#[repr(C)]
#[derive(Default)]
struct FileInfo {
    attributes: u32,
    times: [u32; 6],
    volume: u32,
    size: [u32; 2],
    links: u32,
    index: [u32; 2],
}

/// A file's number of names (hard links), as fs.statSync's nlink: 1 when it cannot be read.
pub fn links(path: &str) -> u64 {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    // No access asked, every sharing allowed, a folder too (FILE_FLAG_BACKUP_SEMANTICS): as libuv's stat opens it.
    let opened = std::fs::OpenOptions::new().access_mode(0).share_mode(7).custom_flags(0x0200_0000).open(path);
    let Ok(f) = opened else { return 1 };
    let mut info = FileInfo::default();
    // SAFETY: an open handle, and a struct of the size the call writes.
    if unsafe { GetFileInformationByHandle(f.as_raw_handle() as Handle, &mut info) } == 0 {
        return 1;
    }
    info.links as u64
}

/// A file's volume serial number and index, the link itself when it is one (FILE_FLAG_OPEN_REPARSE_POINT), or None.
pub fn file_id(path: &str) -> Option<(u64, u64)> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    let f = std::fs::OpenOptions::new().access_mode(0).share_mode(7).custom_flags(0x0220_0000).open(path).ok()?;
    let mut info = FileInfo::default();
    // SAFETY: an open handle, and a struct of the size the call writes.
    if unsafe { GetFileInformationByHandle(f.as_raw_handle() as Handle, &mut info) } == 0 {
        return None;
    }
    Some((info.volume as u64, (info.index[0] as u64) << 32 | info.index[1] as u64))
}

/// The machine's name (libuv's GetHostNameW gives the DNS host name).
pub fn hostname() -> String {
    const COMPUTER_NAME_DNS_HOSTNAME: u32 = 1;
    let mut buf = [0u16; 256];
    let mut len = buf.len() as u32;
    // SAFETY: writes at most len units, and len back.
    if unsafe { GetComputerNameExW(COMPUTER_NAME_DNS_HOSTNAME, buf.as_mut_ptr(), &mut len) } == 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..len as usize])
}

fn console_mode(h: Handle) -> Option<u32> {
    let mut mode = 0;
    // SAFETY: GetConsoleMode writes one u32; it fails on a handle that is not a console.
    (unsafe { GetConsoleMode(h, &mut mode) } != 0).then_some(mode)
}

fn stdin() -> Handle {
    // SAFETY: GetStdHandle has no precondition.
    unsafe { GetStdHandle(STD_INPUT_HANDLE) }
}

fn stdout() -> Handle {
    // SAFETY: as above.
    unsafe { GetStdHandle(STD_OUTPUT_HANDLE) }
}

pub fn stdin_is_tty() -> bool {
    console_mode(stdin()).is_some()
}

pub fn stdout_is_tty() -> bool {
    console_mode(stdout()).is_some()
}

/// The window's size, as libuv reads it: the visible rectangle of the screen buffer.
pub fn size() -> Option<(u16, u16)> {
    let mut info = ScreenBufferInfo::default();
    // SAFETY: writes one struct.
    if unsafe { GetConsoleScreenBufferInfo(stdout(), &mut info) } == 0 {
        return None;
    }
    let w = info.window.right - info.window.left + 1;
    let h = info.window.bottom - info.window.top + 1;
    Some((w.max(0) as u16, h.max(0) as u16))
}

pub struct Saved {
    input: u32,
    output: Option<u32>,
}

/// Raw mode as libuv's UV_TTY_MODE_RAW_VT, exactly its flags: window events and keys read as VT sequences (mouse and
/// bracketed paste need them); no line editing, echo, ctrl+c processing or console mouse records. The output gets VT processing, as libuv turns it on.
pub fn raw_mode() -> io::Result<Saved> {
    let input = console_mode(stdin()).ok_or_else(io::Error::last_os_error)?;
    let raw = ENABLE_WINDOW_INPUT | ENABLE_VIRTUAL_TERMINAL_INPUT;
    // SAFETY: plain mode changes on the process's console.
    if unsafe { SetConsoleMode(stdin(), raw) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let output = console_mode(stdout());
    if let Some(out) = output {
        // SAFETY: as above.
        unsafe { SetConsoleMode(stdout(), out | ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING) };
    }
    Ok(Saved { input, output })
}

pub fn restore(saved: &Saved) {
    // SAFETY: the modes read before.
    unsafe {
        SetConsoleMode(stdin(), saved.input);
        if let Some(out) = saved.output {
            SetConsoleMode(stdout(), out);
        }
    }
}

// --- events

struct Shared {
    queue: Mutex<Vec<Event>>,
    /// The event object, as a usize: a HANDLE is a pointer, not Send.
    event: AtomicUsize,
}

impl Shared {
    fn post(&self, e: Event) {
        let mut q = self.queue.lock().unwrap_or_else(|p| p.into_inner());
        if let (Event::Input(more), Some(Event::Input(bytes))) = (&e, q.last_mut()) {
            bytes.extend_from_slice(more);
        } else if matches!(e, Event::Input(_)) || !q.contains(&e) {
            q.push(e);
        }
        drop(q);
        // SAFETY: the handle stays valid for the process's life.
        unsafe { SetEvent(self.event.load(Ordering::Relaxed) as Handle) };
    }
}

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();

unsafe extern "system" fn on_ctrl(kind: u32) -> Bool {
    // As libuv: ctrl+c is SIGINT, the console's close SIGHUP; ctrl+break (SIGBREAK, not listened to),
    // logoff and shutdown are left to the default handler, which ends the process.
    let sig = match kind {
        CTRL_C_EVENT => Signal::Int,
        CTRL_CLOSE_EVENT => Signal::Hup,
        _ => return 0,
    };
    if let Some(s) = SHARED.get() {
        s.post(Event::Signal(sig));
    }
    // The console ends the process once the handler returns from a close: as libuv, it never returns, and the main
    // thread restores the terminal and exits first.
    if sig == Signal::Hup {
        loop {
            std::thread::sleep(Duration::from_secs(60));
        }
    }
    1
}

/// The input thread: console records, or the bytes of a pipe when stdin is not a console.
fn read_input(shared: Arc<Shared>) {
    let h = stdin();
    if console_mode(h).is_none() {
        let mut buf = vec![0u8; 65536];
        loop {
            let mut got = 0u32;
            // SAFETY: ReadFile writes at most buf.len() bytes.
            let ok = unsafe {
                ReadFile(h, buf.as_mut_ptr() as *mut c_void, buf.len() as u32, &mut got, std::ptr::null_mut())
            };
            if ok == 0 || got == 0 {
                shared.post(Event::Eof);
                return;
            }
            shared.post(Event::Input(buf[..got as usize].to_vec()));
        }
    }
    let mut records = vec![InputRecord { kind: 0, _pad: 0, data: [0; 8] }; 512];
    let mut pending: Option<u16> = None; // a high surrogate waiting for its low half
    loop {
        let mut got = 0u32;
        // SAFETY: writes at most records.len() records.
        if unsafe { ReadConsoleInputW(h, records.as_mut_ptr(), records.len() as u32, &mut got) } == 0 {
            shared.post(Event::Eof);
            return;
        }
        let mut text = String::new();
        let mut resized = false;
        for r in &records[..got as usize] {
            match r.kind {
                // KEY_EVENT_RECORD: bKeyDown (i32), repeat count, virtual key, scan code, the UTF-16 unit, control keys.
                KEY_EVENT => {
                    let down = r.data[0] != 0 || r.data[1] != 0;
                    let repeat = r.data[2].max(1);
                    let unit = r.data[5];
                    if !down || unit == 0 {
                        continue;
                    }
                    for _ in 0..repeat {
                        match (pending.take(), unit) {
                            (_, 0xD800..=0xDBFF) => pending = Some(unit),
                            (Some(hi), 0xDC00..=0xDFFF) => {
                                text.extend(char::decode_utf16([hi, unit]).map(|c| c.unwrap_or('\u{FFFD}')))
                            }
                            (_, u) => text.push(char::from_u32(u as u32).unwrap_or('\u{FFFD}')),
                        }
                    }
                }
                WINDOW_BUFFER_SIZE_EVENT => resized = true,
                _ => {}
            }
        }
        if !text.is_empty() {
            shared.post(Event::Input(text.into_bytes()));
        }
        if resized {
            shared.post(Event::Resize);
        }
    }
}

#[derive(Clone)]
pub struct Waker(Arc<Shared>);

impl Waker {
    pub fn wake(&self) {
        self.0.post(Event::Wake);
    }
}

pub struct Events {
    shared: Arc<Shared>,
}

impl Events {
    pub fn new() -> io::Result<Events> {
        if let Some(s) = SHARED.get() {
            return Ok(Events { shared: s.clone() });
        }
        // SAFETY: an auto-reset event, unnamed.
        let event = unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, std::ptr::null()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        let shared = SHARED
            .get_or_init(|| Arc::new(Shared { queue: Mutex::new(Vec::new()), event: AtomicUsize::new(event as usize) }))
            .clone();
        // SAFETY: the handler only posts to the queue.
        unsafe { SetConsoleCtrlHandler(Some(on_ctrl), 1) };
        let reader = shared.clone();
        std::thread::spawn(move || read_input(reader));
        // A window resized without its screen buffer (conhost with scrollback) sends no record: its size is
        // watched instead, as libuv does with a console layout hook. Not in a herdr pane: its pseudo console sends a
        // record for each resize, and the watch would wake the pane 4 times a second for nothing.
        let watcher = shared.clone();
        let pseudo_console = std::env::var_os("HERDR_PANE_ID").is_some();
        if !pseudo_console {
            std::thread::spawn(move || {
                let mut last = size();
                loop {
                    std::thread::sleep(Duration::from_millis(250));
                    let now = size();
                    if now != last {
                        last = now;
                        watcher.post(Event::Resize);
                    }
                }
            });
        }
        Ok(Events { shared })
    }

    pub fn waker(&self) -> Waker {
        Waker(self.shared.clone())
    }

    pub fn wait(&mut self, timeout: Option<Duration>) -> Vec<Event> {
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            let events = std::mem::take(&mut *self.shared.queue.lock().unwrap_or_else(|p| p.into_inner()));
            if !events.is_empty() {
                return events;
            }
            let ms = match deadline {
                None => INFINITE,
                Some(d) => d.saturating_duration_since(Instant::now()).as_millis().min(INFINITE as u128 - 1) as u32,
            };
            // SAFETY: the handle stays valid for the process's life.
            let r = unsafe { WaitForSingleObject(self.shared.event.load(Ordering::Relaxed) as Handle, ms) };
            if r == WAIT_FAILED {
                panic!("waiting for the console: {}", io::Error::last_os_error());
            }
            if r != WAIT_OBJECT_0 && deadline.is_some_and(|d| Instant::now() >= d) {
                return Vec::new();
            }
        }
    }
}
