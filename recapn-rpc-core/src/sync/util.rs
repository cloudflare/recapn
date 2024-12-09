pub mod array_vec;
pub mod linked_list;
use alloc::sync::Arc;

// use std::sync::Arc;

/// A **M**ovable **Arc**.
///
/// A utility for a shared value wrapped in an Arc that can be moved away unsafely,
/// for example, for dropping or transfering to another type state.
pub struct Marc<T> {
    shared: Option<Arc<T>>,
}

impl<T> Marc<T> {
    pub const fn new(value: Arc<T>) -> Self {
        Self {
            shared: Some(value),
        }
    }

    pub fn get(&self) -> &T {
        debug_assert!(self.shared.is_some());

        unsafe { self.shared.as_ref().unwrap_unchecked() }
    }

    pub fn inner(&self) -> &Arc<T> {
        debug_assert!(self.shared.is_some());

        unsafe { self.shared.as_ref().unwrap_unchecked() }
    }

    pub unsafe fn take(&mut self) -> Arc<T> {
        debug_assert!(self.shared.is_some());

        self.shared.take().unwrap_unchecked()
    }

    pub fn try_take(&mut self) -> Option<Arc<T>> {
        self.shared.take()
    }
}
