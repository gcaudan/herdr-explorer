//! ReadDirectoryChangesW, as libuv watches a folder on Windows: the folder opened for its changes, read on a thread
//! with an overlapped read and a second event to stop it. The names come relative to the folder, with backslashes.

use super::Callback;
use std::io;
use std::os::raw::c_void;
use std::thread::JoinHandle;

type Handle = *mut c_void;
type Bool = i32;

const FILE_LIST_DIRECTORY: u32 = 0x1;
const FILE_SHARE_ALL: u32 = 0x7;
const OPEN_EXISTING: u32 = 3;
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OVERLAPPED: u32 = 0x4000_0000;
const INVALID_HANDLE_VALUE: isize = -1;
const INFINITE: u32 = 0xFFFF_FFFF;
const WAIT_OBJECT_0: u32 = 0;
/// What libuv asks for: names, attributes, size, times, creation, security.
const FILTER: u32 = 0x1 | 0x2 | 0x4 | 0x8 | 0x10 | 0x20 | 0x40 | 0x100;

#[repr(C)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    event: Handle,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        attrs: *mut c_void,
        how: u32,
        flags: u32,
        tpl: Handle,
    ) -> Handle;
    fn ReadDirectoryChangesW(
        h: Handle,
        buf: *mut c_void,
        len: u32,
        subtree: Bool,
        filter: u32,
        got: *mut u32,
        overlapped: *mut Overlapped,
        routine: *mut c_void,
    ) -> Bool;
    fn GetOverlappedResult(h: Handle, overlapped: *mut Overlapped, got: *mut u32, wait: Bool) -> Bool;
    fn CancelIoEx(h: Handle, overlapped: *mut Overlapped) -> Bool;
    fn CreateEventW(attrs: *mut c_void, manual: Bool, initial: Bool, name: *const u16) -> Handle;
    fn SetEvent(h: Handle) -> Bool;
    fn WaitForMultipleObjects(n: u32, handles: *const Handle, all: Bool, ms: u32) -> u32;
    fn CloseHandle(h: Handle) -> Bool;
}

pub struct Watcher {
    /// The stop event, as a usize: a HANDLE is a pointer, not Send.
    stop: usize,
    thread: Option<JoinHandle<()>>,
}

/// The folder opened, its read in progress: the buffer and the OVERLAPPED on the heap, where the system writes until
/// the read completes or is cancelled, whichever thread holds them.
struct Reader {
    h: Handle,
    io_event: Handle,
    recursive: bool,
    /// DWORD-aligned, as ReadDirectoryChangesW needs.
    buf: Vec<u32>,
    ov: Box<Overlapped>,
}

// SAFETY: the handles and the buffers move to the watcher's thread once, and are used by one thread at a time.
unsafe impl Send for Reader {}

impl Reader {
    /// The next read started.
    fn start(&mut self) -> io::Result<()> {
        *self.ov = Overlapped { internal: 0, internal_high: 0, offset: 0, offset_high: 0, event: self.io_event };
        // SAFETY: the buffer and the OVERLAPPED stay in place until the read completes or is cancelled and waited for.
        let started = unsafe {
            ReadDirectoryChangesW(
                self.h,
                self.buf.as_mut_ptr() as *mut c_void,
                (self.buf.len() * 4) as u32,
                self.recursive as Bool,
                FILTER,
                std::ptr::null_mut(),
                &mut *self.ov,
                std::ptr::null_mut(),
            )
        };
        if started == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        // SAFETY: a read still in progress is cancelled and waited for before its buffer goes; then the handles this
        // reader owns, closed once.
        unsafe {
            let mut got = 0u32;
            if CancelIoEx(self.h, &mut *self.ov) != 0 {
                GetOverlappedResult(self.h, &mut *self.ov, &mut got, 1);
            }
            CloseHandle(self.h);
            CloseHandle(self.io_event);
        }
    }
}

impl Watcher {
    pub fn new(dir: &str, recursive: bool, on: Callback, lost: super::Lost) -> io::Result<Watcher> {
        if !std::fs::metadata(dir)?.is_dir() {
            return Err(io::Error::from_raw_os_error(267)); // ERROR_DIRECTORY
        }
        let wide: Vec<u16> = dir.encode_utf16().chain([0]).collect();
        // SAFETY: a wide C string that outlives the call; the handles are closed by the reader, or below.
        let mut reader = unsafe {
            let flags = FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED;
            let h = CreateFileW(
                wide.as_ptr(),
                FILE_LIST_DIRECTORY,
                FILE_SHARE_ALL,
                std::ptr::null_mut(),
                OPEN_EXISTING,
                flags,
                std::ptr::null_mut(),
            );
            if h as isize == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let io_event = CreateEventW(std::ptr::null_mut(), 1, 0, std::ptr::null());
            if io_event.is_null() {
                let e = io::Error::last_os_error();
                CloseHandle(h);
                return Err(e);
            }
            let ov = Box::new(Overlapped { internal: 0, internal_high: 0, offset: 0, offset_high: 0, event: io_event });
            Reader { h, io_event, recursive, buf: vec![0u32; 16384], ov }
        };
        // The first read before returning, as libuv: the changes made from now on are seen, and a folder that cannot
        // be watched so (a network share) fails here, for the caller's fallback.
        reader.start()?;
        // SAFETY: a manual-reset event, unnamed.
        let stop = unsafe { CreateEventW(std::ptr::null_mut(), 1, 0, std::ptr::null()) };
        if stop.is_null() {
            return Err(io::Error::last_os_error());
        }
        let stop = stop as usize;
        let thread = std::thread::spawn(move || run(reader, stop, on, lost));
        Ok(Watcher { stop, thread: Some(thread) })
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // SAFETY: the stop event made in new, set, then closed once the thread is done.
        unsafe { SetEvent(self.stop as Handle) };
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        // SAFETY: as above.
        unsafe { CloseHandle(self.stop as Handle) };
    }
}

/// The watcher's thread: the changes read until the stop event is set, or the folder is gone. The reader, dropped at
/// the end, cancels the read in progress. A read that fails (the folder removed) ends the watch: lost.
fn run(mut reader: Reader, stop: usize, mut on: Callback, lost: super::Lost) {
    let gone = |on: &mut Callback| {
        lost.store(true, std::sync::atomic::Ordering::SeqCst);
        on(None);
    };
    loop {
        let handles = [reader.io_event, stop as Handle];
        // SAFETY: two valid events.
        if unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) } != WAIT_OBJECT_0 {
            break;
        }
        let mut got = 0u32;
        // SAFETY: the read completed.
        if unsafe { GetOverlappedResult(reader.h, &mut *reader.ov, &mut got, 0) } == 0 {
            gone(&mut on);
            break;
        }
        // FILE_NOTIFY_INFORMATION: the offset of the next, the action, the name's length in bytes, the name. Read
        // before the next read reuses the buffer; the changes meanwhile wait in the system's own.
        let mut names = Vec::new();
        if got == 0 {
            // The buffer overflowed: the changes are lost.
            names.push(None);
        } else {
            // SAFETY: the got bytes the read wrote, inside buf.
            let bytes = unsafe { std::slice::from_raw_parts(reader.buf.as_ptr() as *const u8, got as usize) };
            let mut at = 0usize;
            while at + 12 <= bytes.len() {
                let word = |i: usize| u32::from_ne_bytes(bytes[at + i..at + i + 4].try_into().unwrap()) as usize;
                let (next, len) = (word(0), word(8));
                let units: Vec<u16> = bytes[at + 12..(at + 12 + len).min(bytes.len())]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_ne_bytes(*c))
                    .collect();
                names.push(Some(String::from_utf16_lossy(&units)));
                if next == 0 {
                    break;
                }
                at += next;
            }
        }
        let again = reader.start();
        for name in names {
            on(name);
        }
        if again.is_err() {
            gone(&mut on);
            break;
        }
    }
}
