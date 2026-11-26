use std::sync::{Mutex, MutexGuard, PoisonError};

/// Keeps access to the same protected value after a holder panics.
#[derive(Debug, Default)]
pub(crate) struct Lock<T>(Mutex<T>);

impl<T> Lock<T> {
    pub(crate) const fn new(value: T) -> Self {
        Self(Mutex::new(value))
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, T> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
