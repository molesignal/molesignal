// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! CPU-heavy background work, such as encoding the segments a flush writes, on threads of its own
//! that run at a lower priority than request handling.
//!
//! Inside the node's CPU quota the scheduler then serves request threads first, and background
//! work takes the CPU they leave. Before this, a flush's encoding threads competed with requests
//! as equals, and while they ran the container hit its quota and requests stalled behind them.
//! Threads the work starts inherit its priority. Only Linux lowers it; elsewhere the threads run
//! at normal priority.

use kernel::{Error, Result};

/// Nice value of background threads; request threads run at 0. An unprivileged thread cannot raise
/// its priority again, which is why the work gets a thread of its own.
const NICE: i32 = 10;

/// Runs `work` on a new thread named `name` at background priority and waits for its result.
pub(crate) async fn run<T, F>(name: &str, work: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (sender, receiver) = tokio::sync::oneshot::channel();

    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            lower_priority();
            let _ = sender.send(work());
        })
        .map_err(|error| Error::internal(format!("start {name} thread: {error}")))?;

    receiver
        .await
        .map_err(|_| Error::internal(format!("{name} thread ended without a result")))
}

#[cfg(target_os = "linux")]
fn lower_priority() {
    // SAFETY: `gettid` and `setpriority` take no pointers; with this thread's id, `setpriority`
    // changes the nice value of this thread alone.
    let result = unsafe {
        let thread = libc::syscall(libc::SYS_gettid) as libc::id_t;
        libc::setpriority(libc::PRIO_PROCESS, thread, NICE)
    };

    if result != 0 {
        tracing::debug!(
            error = %std::io::Error::last_os_error(),
            "background thread keeps its priority"
        );
    }
}

#[cfg(not(target_os = "linux"))]
fn lower_priority() {
    let _ = NICE;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_work_runs_on_its_own_named_thread() {
        let name = run("encode-test", || {
            std::thread::current().name().map(str::to_owned)
        })
        .await
        .unwrap();

        assert_eq!(name.as_deref(), Some("encode-test"));
    }

    #[tokio::test]
    async fn work_that_panics_is_an_error() {
        let result: Result<()> = run("encode-panic", || panic!("boom")).await;

        assert!(result.is_err());
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn the_thread_runs_at_background_priority() {
        let nice = run("encode-nice", || unsafe {
            let thread = libc::syscall(libc::SYS_gettid) as libc::id_t;
            libc::getpriority(libc::PRIO_PROCESS, thread)
        })
        .await
        .unwrap();

        assert!(nice >= NICE, "{nice}");
    }
}
