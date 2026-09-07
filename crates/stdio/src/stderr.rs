//! Full-content stderr capture. Reads are nonblocking so a descendant holding
//! the pipe cannot prevent provider shutdown or rebuild from finishing.

use crate::routing::SessionId;
use std::io::{self, Read};
use std::process::ChildStderr;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const CHUNK_BYTES: usize = 4 * 1024;
const IDLE_WAIT: Duration = Duration::from_millis(100);
const CLOSE_DRAIN: Duration = Duration::from_millis(100);

pub(crate) struct StderrReader {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl StderrReader {
    pub(crate) fn join(mut self) -> std::thread::Result<()> {
        self.stop.store(true, Ordering::Release);
        let thread = self.thread.take().expect("stderr reader is joined once");
        thread.thread().unpark();
        thread.join()
    }
}

impl Drop for StderrReader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
    }
}

pub(crate) fn spawn(stderr: ChildStderr, session: SessionId) -> io::Result<StderrReader> {
    set_nonblocking(&stderr)?;
    let stop = Arc::new(AtomicBool::new(false));
    let reader_stop = Arc::clone(&stop);
    let thread = std::thread::Builder::new()
        .name("rootle-provider-stderr".into())
        .spawn(move || drain(stderr, session, reader_stop))?;
    Ok(StderrReader {
        stop,
        thread: Some(thread),
    })
}

#[cfg(unix)]
fn set_nonblocking(stderr: &ChildStderr) -> io::Result<()> {
    let flags = rustix::fs::fcntl_getfl(stderr)?;
    rustix::fs::fcntl_setfl(stderr, flags | rustix::fs::OFlags::NONBLOCK)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_nonblocking(_: &ChildStderr) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "provider stderr capture requires Unix; use WSL on Windows",
    ))
}

fn drain(mut stderr: ChildStderr, session: SessionId, stop: Arc<AtomicBool>) {
    let mut chunk = [0u8; CHUNK_BYTES];
    let mut closing = None;
    loop {
        if stop.load(Ordering::Acquire) {
            let deadline = *closing.get_or_insert_with(|| Instant::now() + CLOSE_DRAIN);
            if Instant::now() >= deadline {
                break;
            }
        }
        let read = match stderr.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if closing.is_some() {
                    break;
                }
                std::thread::park_timeout(IDLE_WAIT);
                continue;
            }
            Err(_) => {
                if rootle_trace::capture_content() {
                    rootle_trace::mark_incomplete("provider stderr could not be read");
                }
                break;
            }
        };
        // Continue draining after recording fails or a later session has a
        // different privacy policy. Core pins this lazy payload to its session.
        if !rootle_trace::capture_content() {
            continue;
        }
        rootle_trace::record_with(rootle_trace::EventKind::ProviderStderr, || {
            serde_json::json!({
                "provider":"stdio", "session":session.value(), "bytes":read,
                "utf8_valid":std::str::from_utf8(&chunk[..read]).is_ok(),
                "text":rootle_trace::capture_content().then(||String::from_utf8_lossy(&chunk[..read])),
            })
        });
    }
}
