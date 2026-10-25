//! A pane's worker threads: the JS ran its herdr, git and clipboard calls asynchronously; a pane here runs them on a
//! thread, a job at a time, and each job's result comes back to the pane's loop (`Then`), which owns all the state.

use crate::sys;
use std::panic::AssertUnwindSafe;
use std::sync::mpsc::{Sender, channel};

/// The name of the worker threads, whose panics are caught (an action failed) and not printed over the pane.
const NAME: &str = "pane-worker";

/// What the loop runs once a job is done: the result applied to the pane P.
pub type Then<P> = Box<dyn FnOnce(&mut P) + Send>;
/// A job of a worker.
pub type Job<P> = Box<dyn FnOnce() -> Then<P> + Send>;

/// A thread running jobs one at a time, their results sent to the loop.
pub struct Worker<P>(Sender<Job<P>>);

impl<P: 'static> Worker<P> {
    pub fn new(tx: Sender<Then<P>>, waker: sys::Waker) -> Worker<P> {
        let (jobs, rx) = channel::<Job<P>>();
        let _ = std::thread::Builder::new().name(NAME.into()).spawn(move || {
            for job in rx {
                // A job that panicked gives nothing, as a promise whose rejection is ignored.
                if let Ok(then) = std::panic::catch_unwind(AssertUnwindSafe(job)) {
                    if tx.send(then).is_err() {
                        break;
                    }
                    waker.wake();
                }
            }
        });
        Worker(jobs)
    }

    pub fn send(&self, job: Job<P>) {
        let _ = self.0.send(job);
    }
}

/// A job that panicked is an action that failed: not printed over the pane.
pub fn quiet_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().name() != Some(NAME) {
            default(info)
        }
    }));
}
