//! Small helpers: locks, clocks, ids and a sync bridge to async.

use core::fmt::Write as _;
use core::future::Future;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::runtime::{Handle, Runtime, RuntimeFlavor};

/// Lock `m`, through a poisoning.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Lowercase hex of `b`.
pub fn hex(b: &[u8]) -> String {
    b.iter()
        .fold(String::with_capacity(2 * b.len()), |mut s, x| {
            let _ = write!(s, "{x:02x}");
            s
        })
}

fn since_epoch() -> core::time::Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}

/// Unix seconds.
pub fn now_s() -> i64 {
    i64::try_from(since_epoch().as_secs()).unwrap_or(i64::MAX)
}

/// Unix milliseconds.
pub fn now_ms() -> i64 {
    i64::try_from(since_epoch().as_millis()).unwrap_or(i64::MAX)
}

/// 32 random hex digits (a login's id).
pub fn new_id() -> String {
    let mut b = [0u8; 16];
    if getrandom::fill(&mut b).is_err() {
        // No OS randomness: still unique within the process.
        b[..8].copy_from_slice(&now_ms().to_le_bytes());
    }
    hex(&b)
}

fn fallback() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        #[allow(clippy::expect_used, reason = "no runtime, nothing to run on")]
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("grabcli-service-rt")
            .build()
            .expect("tokio runtime")
    })
}

/// Run `f` to its end from sync code: on the current multi-thread runtime
/// (`block_in_place`); inside a current-thread one, on a scoped thread
/// (that runtime can't be blocked in place); with none, on a small runtime
/// of this crate's.
pub fn block_on<F>(f: F) -> F::Output
where
    F: Future + Send,
    F::Output: Send,
{
    match Handle::try_current() {
        Ok(h) if h.runtime_flavor() == RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(|| h.block_on(f))
        }
        Ok(_) => std::thread::scope(|s| {
            s.spawn(|| fallback().block_on(f))
                .join()
                .unwrap_or_else(|p| std::panic::resume_unwind(p))
        }),
        Err(_) => fallback().block_on(f),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_hex_and_differ() {
        let (a, b) = (new_id(), new_id());
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
        assert_eq!(hex(&[0, 255]), "00ff");
    }
}
