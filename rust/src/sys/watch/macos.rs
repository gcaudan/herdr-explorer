//! FSEvents, as libuv watches a folder on macOS: a stream of the folder's tree, file by file, on a dispatch queue of
//! its own. The paths come real (/private/var for /var): taken relative to the folder's real path. A watch that is
//! not recursive keeps the folder's own entries only, as libuv does.

use super::Callback;
use std::ffi::{CStr, CString, c_char};
use std::io;
use std::os::raw::c_void;
use std::sync::Mutex;

type CFRef = *const c_void;

const UTF8: u32 = 0x0800_0100;
const SINCE_NOW: u64 = u64::MAX;
const NO_DEFER: u32 = 0x2;
const FILE_EVENTS: u32 = 0x10;
const MUST_SCAN_SUBDIRS: u32 = 0x1;
/// libuv's latency, in seconds.
const LATENCY: f64 = 0.05;

#[repr(C)]
struct Context {
    version: isize,
    info: *mut c_void,
    retain: Option<extern "C" fn(*const c_void) -> *const c_void>,
    release: Option<extern "C" fn(*const c_void)>,
    describe: Option<extern "C" fn(*const c_void) -> CFRef>,
}

type StreamCallback = extern "C" fn(*mut c_void, *mut c_void, usize, *mut c_void, *const u32, *const u64);

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFTypeArrayCallBacks: c_void;
    fn CFStringCreateWithCString(alloc: CFRef, s: *const c_char, encoding: u32) -> CFRef;
    fn CFArrayCreate(alloc: CFRef, values: *const CFRef, n: isize, callbacks: *const c_void) -> CFRef;
    fn CFRelease(r: CFRef);
}

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn FSEventStreamCreate(
        alloc: CFRef,
        callback: StreamCallback,
        context: *const Context,
        paths: CFRef,
        since: u64,
        latency: f64,
        flags: u32,
    ) -> *mut c_void;
    fn FSEventStreamSetDispatchQueue(stream: *mut c_void, queue: *mut c_void);
    fn FSEventStreamStart(stream: *mut c_void) -> u8;
    fn FSEventStreamStop(stream: *mut c_void);
    fn FSEventStreamInvalidate(stream: *mut c_void);
    fn FSEventStreamRelease(stream: *mut c_void);
}

unsafe extern "C" {
    fn dispatch_queue_create(label: *const c_char, attr: *const c_void) -> *mut c_void;
    fn dispatch_release(object: *mut c_void);
    fn dispatch_sync_f(queue: *mut c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
}

/// Nothing: run on the queue, it waits for a callback in progress to end.
extern "C" fn drained(_: *mut c_void) {}

/// What the stream's callback reads: the folder's real path, whether recursive, the callback.
struct Info {
    real: String,
    recursive: bool,
    on: Mutex<Callback>,
    /// The callback panicked: no more calls, as the watcher's thread ends with it on the other systems.
    dead: std::sync::atomic::AtomicBool,
}

extern "C" fn release(info: *const c_void) {
    // SAFETY: the Box made in Watcher::new, released once by the stream.
    drop(unsafe { Box::from_raw(info as *mut Info) });
}

extern "C" fn changed(
    _: *mut c_void,
    info: *mut c_void,
    n: usize,
    paths: *mut c_void,
    flags: *const u32,
    _: *const u64,
) {
    // SAFETY: the stream gives the Info it was made with, n C strings (no CF types asked) and n flags.
    let (info, paths, flags) = unsafe {
        (
            &*(info as *const Info),
            std::slice::from_raw_parts(paths as *const *const c_char, n),
            std::slice::from_raw_parts(flags, n),
        )
    };
    use std::sync::atomic::Ordering;
    if info.dead.load(Ordering::Relaxed) {
        return;
    }
    // A panic must not unwind through FSEvents' frames, which would abort the process.
    let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| call(info, paths, flags)));
    if done.is_err() {
        info.dead.store(true, Ordering::Relaxed);
    }
}

fn call(info: &Info, paths: &[*const c_char], flags: &[u32]) {
    let mut on = info.on.lock().unwrap_or_else(|p| p.into_inner());
    for (&p, &flag) in paths.iter().zip(flags) {
        if flag & MUST_SCAN_SUBDIRS != 0 {
            on(None);
            continue;
        }
        // SAFETY: a C string of the stream's, valid during the call.
        let path = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        let rel = if path == info.real {
            info.real.rsplit('/').next().unwrap_or("").to_string()
        } else {
            let base = if info.real == "/" { String::from("/") } else { format!("{}/", info.real) };
            let Some(rel) = path.strip_prefix(&base) else { continue };
            if !info.recursive && rel.contains('/') {
                continue;
            }
            rel.to_string()
        };
        on(Some(rel));
    }
}

pub struct Watcher {
    stream: *mut c_void,
    queue: *mut c_void,
}

// SAFETY: the stream and the queue are only stopped and released, from the thread that drops the Watcher.
unsafe impl Send for Watcher {}

impl Watcher {
    pub fn new(dir: &str, recursive: bool, on: Callback) -> io::Result<Watcher> {
        let real = std::fs::canonicalize(dir)?;
        if !real.is_dir() {
            return Err(io::Error::from_raw_os_error(20)); // ENOTDIR
        }
        let real = real.to_string_lossy().into_owned();
        let path = CString::new(real.clone()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        let info = Box::into_raw(Box::new(Info { real, recursive, on: Mutex::new(on), dead: Default::default() }));
        let context =
            Context { version: 0, info: info as *mut c_void, retain: None, release: Some(release), describe: None };
        // SAFETY: CoreFoundation and FSEvents calls on values made here; the array and the string are released once
        // the stream holds them, the Info by the stream (release) once it is released.
        unsafe {
            let s = CFStringCreateWithCString(std::ptr::null(), path.as_ptr(), UTF8);
            let values = [s];
            let paths = CFArrayCreate(std::ptr::null(), values.as_ptr(), 1, &kCFTypeArrayCallBacks);
            CFRelease(s);
            let stream = FSEventStreamCreate(
                std::ptr::null(),
                changed,
                &context,
                paths,
                SINCE_NOW,
                LATENCY,
                NO_DEFER | FILE_EVENTS,
            );
            CFRelease(paths);
            if stream.is_null() {
                drop(Box::from_raw(info));
                return Err(io::Error::other("FSEventStreamCreate failed"));
            }
            let queue = dispatch_queue_create(c"herdr-explorer.watch".as_ptr(), std::ptr::null());
            FSEventStreamSetDispatchQueue(stream, queue);
            if FSEventStreamStart(stream) == 0 {
                FSEventStreamInvalidate(stream);
                FSEventStreamRelease(stream);
                dispatch_release(queue);
                return Err(io::Error::other("FSEventStreamStart failed"));
            }
            Ok(Watcher { stream, queue })
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // SAFETY: the stream and queue made in new, stopped and released once; the queue drained in between, so that
        // no callback still runs when the stream releases its Info.
        unsafe {
            FSEventStreamStop(self.stream);
            FSEventStreamInvalidate(self.stream);
            dispatch_sync_f(self.queue, std::ptr::null_mut(), drained);
            FSEventStreamRelease(self.stream);
            dispatch_release(self.queue);
        }
    }
}
