//! Locking that survives a panic elsewhere.
//!
//! `Mutex::lock` fails forever once a thread panicked while holding the
//! guard. Every `Mutex` in Odin guards a registry, cache or connection that
//! stays usable after such a panic, and an `expect` on the poisoned lock
//! would turn one failed request into every later request failing too.

use std::sync::{Mutex, MutexGuard};

pub trait LockExt<T> {
    /// Locks the mutex, taking the guard back even if a previous holder
    /// panicked. The recovery is logged once per panic, so the original
    /// failure does not pass unnoticed.
    fn lock_or_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> LockExt<T> for Mutex<T> {
    fn lock_or_recover(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| {
            tracing::error!(
                "a thread panicked while holding a lock; continuing with the data it left"
            );
            // Without this every later call would log again.
            self.clear_poison();
            poisoned.into_inner()
        })
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

    #[test]
    fn recovering_clears_the_poison_so_it_is_reported_once() {
        let value = Arc::new(Mutex::new(1));
        let poisoner = value.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.lock().unwrap();
            panic!("holder panicked");
        })
        .join();

        drop(value.lock_or_recover());

        assert!(value.lock().is_ok());
    }
}
