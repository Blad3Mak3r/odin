//! Locking that survives a panic elsewhere.
//!
//! `Mutex::lock` fails forever once a thread panicked while holding the
//! guard. Every `Mutex` in Odin guards a registry, cache or connection that
//! stays usable after such a panic, and an `expect` on the poisoned lock
//! would turn one failed request into every later request failing too.

use std::sync::{Mutex, MutexGuard, PoisonError};

pub trait LockExt<T> {
    /// Locks the mutex, taking the guard back even if a previous holder
    /// panicked.
    fn lock_or_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> LockExt<T> for Mutex<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn a_lock_poisoned_by_a_panic_can_still_be_taken() {
        let value = Arc::new(Mutex::new(1));
        let poisoner = value.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.lock().unwrap();
            panic!("holder panicked");
        })
        .join();
        assert!(value.lock().is_err());

        *value.lock_or_recover() += 1;

        assert_eq!(*value.lock_or_recover(), 2);
    }
}
