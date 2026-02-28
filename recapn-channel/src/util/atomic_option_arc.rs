use std::ptr::null;
use std::ptr::null_mut;
use std::sync::Arc;
use std::sync::atomic::AtomicPtr;
use std::sync::atomic::Ordering;

/// An atomic Option<Arc<T>> pointer.
pub struct AtomicOptionArc<T> {
    ptr: AtomicPtr<T>,
}

impl<T> AtomicOptionArc<T> {
    const SENTINEL: *const T = std::ptr::without_provenance(usize::MAX);

    fn value_to_ptr(value: Option<Arc<T>>) -> *const T {
        match value {
            Some(value) => Arc::into_raw(value),
            None => null(),
        }
    }

    fn ptr_to_value(ptr: *const T) -> Option<Arc<T>> {
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { Arc::from_raw(ptr) })
        }
    }

    fn replace_ptr(&self, new: *const T) -> *const T {
        loop {
            let result = self.ptr.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |old| {
                if old.cast_const() == Self::SENTINEL {
                    return None
                }

                Some(new.cast_mut())
            });
            match result {
                Ok(value) => break value,
                Err(_) => std::hint::spin_loop(),
            }
        }
    }

    fn replace_ptr_mut(&mut self, new: *const T) -> *const T {
        std::mem::replace(self.ptr.get_mut(), new.cast_mut()).cast_const()
    }

    /// Place the sentinel value in the atom unless it's null.
    fn place_sentinel(&self) -> *const T {
        loop {
            let result = self.ptr.fetch_update(Ordering::Acquire, Ordering::Relaxed, |old| {
                if old.cast_const() == Self::SENTINEL {
                    return None
                }

                if old == null_mut() {
                    return None
                }

                Some(Self::SENTINEL.cast_mut())
            });
            match result {
                Ok(value) => break value,
                Err(ptr) if ptr.is_null() => break ptr,
                Err(_) => std::hint::spin_loop(),
            }
        }
    }

    /// Place a new value in the atom assuming the existing value is the sentinel.
    fn replace_sentinel(&self, new: *const T) -> bool {
        self.ptr.compare_exchange(Self::SENTINEL.cast_mut(), new.cast_mut(), Ordering::Release, Ordering::Relaxed).is_ok()
    }

    pub const fn none() -> Self {
        Self { ptr: AtomicPtr::new(null_mut()) }
    }

    pub fn clear(&self) {
        drop(self.take());
    }

    pub fn replace(&self, new: Option<Arc<T>>) -> Option<Arc<T>> {
        let new = Self::value_to_ptr(new);
        let old = self.replace_ptr(new);
        Self::ptr_to_value(old)
    }

    pub fn replace_mut(&mut self, new: Option<Arc<T>>) -> Option<Arc<T>> {
        let new = Self::value_to_ptr(new);
        let old = self.replace_ptr_mut(new);
        if old == Self::SENTINEL {
            None
        } else {
            Self::ptr_to_value(old)
        }
    }

    /// Remove the value, returning the `Arc<T>` if it exists.
    pub fn take(&self) -> Option<Arc<T>> {
        self.replace(None)
    }

    pub fn add_ref(&self) -> Option<Arc<T>> {
        let old = self.place_sentinel();
        let current = Self::ptr_to_value(old)?;
        let replacement = Arc::into_raw(Arc::clone(&current));
        assert!(self.replace_sentinel(replacement));
        Some(current)
    }

    pub fn same_as(&self, other: Option<&Arc<T>>) -> bool {
        let other = match other {
            None => null(),
            Some(arc) => Arc::as_ptr(arc),
        };

        self.ptr_eq(other.cast())
    }

    /// Compare the value in the atom with the given unit pointer.
    pub fn ptr_eq(&self, other: *const ()) -> bool {
        let ptr = loop {
            let ptr = self.ptr.load(Ordering::Relaxed).cast_const();
            if ptr == Self::SENTINEL {
                std::hint::spin_loop();
                continue
            }

            break ptr;
        };
        ptr.cast::<()>() == other
    }
}

impl<T> Drop for AtomicOptionArc<T> {
    fn drop(&mut self) {
        self.replace_mut(None);
    }
}