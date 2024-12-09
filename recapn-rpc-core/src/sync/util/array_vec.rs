use core::mem::MaybeUninit;
use core::ptr;

pub(crate) struct ArrayVec<T, const N: usize> {
    inner: [MaybeUninit<T>; N],
    curr: usize,
}

impl<T, const N: usize> ArrayVec<T, N> {
    pub const fn new() -> Self {
        Self {
            inner: unsafe {
                // safety: Create an uninitialized array of `MaybeUninit`. The
                // `assume_init` is safe because the type we are claiming to
                // have initialized here is a bunch of `MaybeUninit`s, which do
                // not require initialization.
                MaybeUninit::uninit().assume_init()
            },
            curr: 0,
        }
    }

    #[inline]
    pub const fn can_push(&self) -> bool {
        self.curr < N
    }

    #[inline]
    pub fn push(&mut self, val: T) {
        debug_assert!(self.can_push());

        self.inner[self.curr] = MaybeUninit::new(val);
        self.curr += 1;
    }

    #[inline]
    pub fn for_each<F: FnMut(T)>(&mut self, mut func: F) {
        assert!(self.curr <= N);
        while self.curr > 0 {
            self.curr -= 1;
            (func)(unsafe { self.inner[self.curr].assume_init_read() });
        }
    }
}

impl<T, const N: usize> Drop for ArrayVec<T, N> {
    fn drop(&mut self) {
        let slice = ptr::slice_from_raw_parts_mut(self.inner.as_mut_ptr().cast::<T>(), self.curr);
        unsafe { ptr::drop_in_place(slice) };
    }
}
