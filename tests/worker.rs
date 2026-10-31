//! core::worker: a job that panics, in a binary of its own, as a process has one `Events`.

use herdr_explorer::core::worker::{Then, Worker};
use herdr_explorer::sys;
use std::sync::mpsc::channel;

/// A job that panics gives nothing, unless sent with a fallback; the worker goes on with the next jobs.
#[test]
fn a_panicking_job_gives_its_fallback() {
    let events = sys::Events::new().unwrap();
    let (tx, rx) = channel::<Then<Vec<u8>>>();
    let worker = Worker::new(tx, events.waker());
    worker.send(Box::new(|| panic!("lost")));
    worker.send_or(Box::new(|| panic!("failed")), Box::new(|v: &mut Vec<u8>| v.push(1)));
    worker.send_or(Box::new(|| Box::new(|v: &mut Vec<u8>| v.push(2))), Box::new(|v: &mut Vec<u8>| v.push(3)));
    let mut got = Vec::new();
    for _ in 0..2 {
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap()(&mut got);
    }
    assert_eq!(got, [1, 2]);
    assert!(rx.recv_timeout(std::time::Duration::from_millis(100)).is_err());
}
