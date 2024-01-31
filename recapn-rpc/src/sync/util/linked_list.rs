//! An intrusive double linked list of data.
//!
//! The data structure supports tracking pinned nodes. Most of the data
//! structure's APIs are `unsafe` as they require the caller to ensure the
//! specified node is actually contained by the list.
//! 
//! This is largely copied from tokio, as many of the types in this lib use similar mechanisms
//! for handling futures.

use core::cell::UnsafeCell;
use core::fmt;
use core::marker::{PhantomData, PhantomPinned};
use core::mem;
use core::ptr::NonNull;
use core::ptr::{addr_of, addr_of_mut};

/// An intrusive linked list.
///
/// Currently, the list is not emptied on drop. It is the caller's
/// responsibility to ensure the list is empty before dropping it.
pub(crate) struct LinkedList<L, T> {
    /// Linked list head
    head: Option<NonNull<T>>,

    /// Linked list tail
    tail: Option<NonNull<T>>,

    /// Node type marker.
    _marker: PhantomData<*const L>,
}

unsafe impl<L: Link> Send for LinkedList<L, L::Target> where L::Target: Send {}
unsafe impl<L: Link> Sync for LinkedList<L, L::Target> where L::Target: Sync {}

/// Defines how a type is tracked within a linked list.
///
/// In order to support storing a single type within multiple lists, accessing
/// the list pointers is decoupled from the entry type.
///
/// # Safety
///
/// Implementations must guarantee that `Target` types are pinned in memory. In
/// other words, when a node is inserted, the value will not be moved as long as
/// it is stored in the list.
pub(crate) unsafe trait Link {
    /// Handle to the list entry.
    ///
    /// This is usually a pointer-ish type.
    type Handle;

    /// Node type.
    type Target;

    /// Convert the handle to a raw pointer without consuming the handle.
    fn into_raw(handle: Self::Handle) -> NonNull<Self::Target>;

    /// Convert the raw pointer to a handle
    unsafe fn from_raw(ptr: NonNull<Self::Target>) -> Self::Handle;

    /// Return the pointers for a node
    ///
    /// # Safety
    ///
    /// The resulting pointer should have the same tag in the stacked-borrows
    /// stack as the argument. In particular, the method may not create an
    /// intermediate reference in the process of creating the resulting raw
    /// pointer.
    unsafe fn pointers(target: NonNull<Self::Target>) -> NonNull<Pointers<Self::Target>>;
}

/// Previous / next pointers.
pub(crate) struct Pointers<T> {
    inner: UnsafeCell<PointersInner<T>>,
}

/// We do not want the compiler to put the `noalias` attribute on mutable
/// references to this type, so the type has been made `!Unpin` with a
/// `PhantomPinned` field.
///
/// See this link for more information:
/// <https://github.com/rust-lang/rust/pull/82834>
struct PointersInner<T> {
    /// The previous node in the list. null if there is no previous node.
    prev: Option<NonNull<T>>,

    /// The next node in the list. null if there is no previous node.
    next: Option<NonNull<T>>,

    /// This type is !Unpin due to the heuristic from:
    /// <https://github.com/rust-lang/rust/pull/82834>
    _pin: PhantomPinned,
}

unsafe impl<T: Send> Send for Pointers<T> {}
unsafe impl<T: Sync> Sync for Pointers<T> {}

impl<T> Pointers<T> {
    /// Create a new set of empty pointers
    pub(crate) fn new() -> Pointers<T> {
        Pointers {
            inner: UnsafeCell::new(PointersInner {
                prev: None,
                next: None,
                _pin: PhantomPinned,
            }),
        }
    }

    pub(crate) fn get_prev(&self) -> Option<NonNull<T>> {
        unsafe { addr_of!((*self.inner.get()).prev).read() }
    }
    pub(crate) fn get_next(&self) -> Option<NonNull<T>> {
        unsafe { addr_of!((*self.inner.get()).next).read() }
    }

    fn set_prev(&mut self, value: Option<NonNull<T>>) {
        unsafe { addr_of_mut!((*self.inner.get()).prev).write(value) }
    }
    fn set_next(&mut self, value: Option<NonNull<T>>) {
        unsafe { addr_of_mut!((*self.inner.get()).next).write(value) }
    }
}

impl<T> fmt::Debug for Pointers<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prev = self.get_prev();
        let next = self.get_next();
        f.debug_struct("Pointers")
            .field("prev", &prev)
            .field("next", &next)
            .finish()
    }
}

// ===== impl LinkedList =====

impl<L, T> LinkedList<L, T> {
    /// Creates an empty linked list.
    pub(crate) const fn new() -> LinkedList<L, T> {
        LinkedList {
            head: None,
            tail: None,
            _marker: PhantomData,
        }
    }
}

impl<L: Link> LinkedList<L, L::Target> {
    /// Adds an element first in the list.
    pub(crate) fn push_front(&mut self, val: L::Handle) {
        let ptr = L::into_raw(val);
        assert_ne!(self.head, Some(ptr));
        unsafe {
            let ptrs = L::pointers(ptr).as_mut();
            ptrs.set_next(self.head);
            ptrs.set_prev(None);

            let ptr = Some(ptr);

            match self.head {
                None => self.tail = ptr,
                Some(head) => L::pointers(head).as_mut().set_prev(ptr),
            }

            self.head = ptr;
        }
    }

    /// Adds an element to the back of the list.
    pub(crate) fn push_back(&mut self, val: L::Handle) {
        let ptr = L::into_raw(val);
        assert_ne!(self.tail, Some(ptr));
        unsafe {
            let ptrs = L::pointers(ptr).as_mut();
            ptrs.set_next(None);
            ptrs.set_prev(self.tail);

            let ptr = Some(ptr);

            match self.tail {
                None => self.head = ptr,
                Some(tail) => L::pointers(tail).as_mut().set_next(ptr),
            }

            self.tail = ptr;
        }
    }

    /// Moves all elements from the other list to this list.
    pub(crate) fn append_back(&mut self, other: &mut Self) {
        match self.tail {
            None => mem::swap(self, other),
            Some(tail) => {
                if let Some(other_head) = other.head.take() {
                    unsafe {
                        L::pointers(tail).as_mut().set_next(Some(other_head));
                        L::pointers(other_head).as_mut().set_prev(Some(tail));
                    }

                    self.tail = other.tail.take();
                }
            }
        }
    }

    /// Removes the last element from a list and returns it, or None if it is
    /// empty.
    pub fn pop_back(&mut self) -> Option<L::Handle> {
        unsafe {
            let last = self.tail?;
            self.tail = L::pointers(last).as_ref().get_prev();

            if let Some(prev) = L::pointers(last).as_ref().get_prev() {
                L::pointers(prev).as_mut().set_next(None);
            } else {
                self.head = None
            }

            L::pointers(last).as_mut().set_prev(None);
            L::pointers(last).as_mut().set_next(None);

            Some(L::from_raw(last))
        }
    }

    /// Removes the first element from a list and returns it, or None if it is
    /// empty.
    pub(crate) fn pop_front(&mut self) -> Option<L::Handle> {
        unsafe {
            let first = self.head?;
            self.head = L::pointers(first).as_ref().get_next();

            if let Some(next) = L::pointers(first).as_ref().get_next() {
                L::pointers(next).as_mut().set_prev(None);
            } else {
                self.tail = None
            }

            L::pointers(first).as_mut().set_prev(None);
            L::pointers(first).as_mut().set_next(None);

            Some(L::from_raw(first))
        }
    }

    /// Returns whether the linked list does not contain any node
    pub(crate) fn is_empty(&self) -> bool {
        if self.head.is_some() {
            return false;
        }

        assert!(self.tail.is_none());
        true
    }

    /// Removes the specified node from the list
    ///
    /// # Safety
    ///
    /// The caller **must** ensure that exactly one of the following is true:
    /// - `node` is currently contained by `self`,
    /// - `node` is not contained by any list,
    /// - `node` is currently contained by some other `GuardedLinkedList` **and**
    ///   the caller has an exclusive access to that list. This condition is
    ///   used by the linked list in `sync::Notify`.
    pub(crate) unsafe fn remove(&mut self, node: NonNull<L::Target>) -> Option<L::Handle> {
        if let Some(prev) = L::pointers(node).as_ref().get_prev() {
            debug_assert_eq!(L::pointers(prev).as_ref().get_next(), Some(node));
            L::pointers(prev)
                .as_mut()
                .set_next(L::pointers(node).as_ref().get_next());
        } else {
            if self.head != Some(node) {
                return None;
            }

            self.head = L::pointers(node).as_ref().get_next();
        }

        if let Some(next) = L::pointers(node).as_ref().get_next() {
            debug_assert_eq!(L::pointers(next).as_ref().get_prev(), Some(node));
            L::pointers(next)
                .as_mut()
                .set_prev(L::pointers(node).as_ref().get_prev());
        } else {
            // This might be the last item in the list
            if self.tail != Some(node) {
                return None;
            }

            self.tail = L::pointers(node).as_ref().get_prev();
        }

        L::pointers(node).as_mut().set_next(None);
        L::pointers(node).as_mut().set_prev(None);

        Some(L::from_raw(node))
    }
}

impl<L: Link> fmt::Debug for LinkedList<L, L::Target> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LinkedList")
            .field("head", &self.head)
            .field("tail", &self.tail)
            .finish()
    }
}

impl<L: Link> Default for LinkedList<L, L::Target> {
    fn default() -> Self {
        Self::new()
    }
}

/// An intrusive linked list, but instead of keeping pointers to the head
/// and tail nodes, it uses a special guard node linked with those nodes.
/// It means that the list is circular and every pointer of a node from
/// the list is not `None`, including pointers from the guard node.
///
/// If a list is empty, then both pointers of the guard node are pointing
/// at the guard node itself.
pub(crate) struct GuardedLinkedList<L, T> {
    /// Pointer to the guard node.
    guard: NonNull<T>,

    /// Node type marker.
    _marker: PhantomData<*const L>,
}

impl<U, L: Link<Handle = NonNull<U>>> LinkedList<L, L::Target> {
    /// Turns a linked list into the guarded version by linking the guard node
    /// with the head and tail nodes. Like with other nodes, you should guarantee
    /// that the guard node is pinned in memory.
    pub(crate) fn into_guarded(self, guard_handle: L::Handle) -> GuardedLinkedList<L, L::Target> {
        // `guard_handle` is a NonNull pointer, we don't have to care about dropping it.
        let guard = L::into_raw(guard_handle);

        unsafe {
            if let Some(head) = self.head {
                debug_assert!(L::pointers(head).as_ref().get_prev().is_none());
                L::pointers(head).as_mut().set_prev(Some(guard));
                L::pointers(guard).as_mut().set_next(Some(head));

                // The list is not empty, so the tail cannot be `None`.
                let tail = self.tail.unwrap();
                debug_assert!(L::pointers(tail).as_ref().get_next().is_none());
                L::pointers(tail).as_mut().set_next(Some(guard));
                L::pointers(guard).as_mut().set_prev(Some(tail));
            } else {
                // The list is empty.
                L::pointers(guard).as_mut().set_prev(Some(guard));
                L::pointers(guard).as_mut().set_next(Some(guard));
            }
        }

        GuardedLinkedList { guard, _marker: PhantomData }
    }
}

impl<L: Link> GuardedLinkedList<L, L::Target> {
    fn tail(&self) -> Option<NonNull<L::Target>> {
        let tail_ptr = unsafe {
            L::pointers(self.guard).as_ref().get_prev().unwrap()
        };

        // Compare the tail pointer with the address of the guard node itself.
        // If the guard points at itself, then there are no other nodes and
        // the list is considered empty.
        if tail_ptr != self.guard {
            Some(tail_ptr)
        } else {
            None
        }
    }

    /// Removes the last element from a list and returns it, or None if it is
    /// empty.
    pub(crate) fn pop_back(&mut self) -> Option<L::Handle> {
        unsafe {
            let last = self.tail()?;
            let before_last = L::pointers(last).as_ref().get_prev().unwrap();

            L::pointers(self.guard).as_mut().set_prev(Some(before_last));
            L::pointers(before_last).as_mut().set_next(Some(self.guard));

            L::pointers(last).as_mut().set_prev(None);
            L::pointers(last).as_mut().set_next(None);

            Some(L::from_raw(last))
        }
    }
}

#[cfg(any(test, fuzzing))]
#[cfg(not(loom))]
pub(crate) mod tests {
    use super::*;

    use std::pin::Pin;

    #[derive(Debug)]
    #[repr(C)]
    struct Entry {
        pointers: Pointers<Entry>,
        val: i32,
    }

    unsafe impl<'a> Link for &'a Entry {
        type Handle = Pin<&'a Entry>;
        type Target = Entry;

        fn into_raw(handle: Pin<&'_ Entry>) -> NonNull<Entry> {
            NonNull::from(handle.get_ref())
        }

        unsafe fn from_raw(ptr: NonNull<Entry>) -> Pin<&'a Entry> {
            Pin::new_unchecked(&*ptr.as_ptr())
        }

        unsafe fn pointers(target: NonNull<Entry>) -> NonNull<Pointers<Entry>> {
            target.cast()
        }
    }

    fn entry(val: i32) -> Pin<Box<Entry>> {
        Box::pin(Entry {
            pointers: Pointers::new(),
            val,
        })
    }

    fn ptr(r: &Pin<Box<Entry>>) -> NonNull<Entry> {
        r.as_ref().get_ref().into()
    }

    fn collect_list(list: &mut LinkedList<&'_ Entry, <&'_ Entry as Link>::Target>) -> Vec<i32> {
        let mut ret = vec![];

        while let Some(entry) = list.pop_back() {
            ret.push(entry.val);
        }

        ret
    }

    fn push_all<'a>(
        list: &mut LinkedList<&'a Entry, <&'_ Entry as Link>::Target>,
        entries: &[Pin<&'a Entry>],
    ) {
        for entry in entries.iter() {
            list.push_front(*entry);
        }
    }

    #[cfg(test)]
    macro_rules! assert_clean {
        ($e:ident) => {{
            assert!($e.pointers.get_next().is_none());
            assert!($e.pointers.get_prev().is_none());
        }};
    }

    #[cfg(test)]
    macro_rules! assert_ptr_eq {
        ($a:expr, $b:expr) => {{
            // Deal with mapping a Pin<&mut T> -> Option<NonNull<T>>
            assert_eq!(Some($a.as_ref().get_ref().into()), $b)
        }};
    }

    #[test]
    fn const_new() {
        const _: LinkedList<&Entry, <&Entry as Link>::Target> = LinkedList::new();
    }

    #[test]
    fn push_and_drain() {
        let a = entry(5);
        let b = entry(7);
        let c = entry(31);

        let mut list = LinkedList::new();
        assert!(list.is_empty());

        list.push_front(a.as_ref());
        assert!(!list.is_empty());
        list.push_front(b.as_ref());
        list.push_front(c.as_ref());

        let items: Vec<i32> = collect_list(&mut list);
        assert_eq!([5, 7, 31].to_vec(), items);

        assert!(list.is_empty());
    }

    #[test]
    fn push_pop_push_pop() {
        let a = entry(5);
        let b = entry(7);

        let mut list = LinkedList::<&Entry, <&Entry as Link>::Target>::new();

        list.push_front(a.as_ref());

        let entry = list.pop_back().unwrap();
        assert_eq!(5, entry.val);
        assert!(list.is_empty());

        list.push_front(b.as_ref());

        let entry = list.pop_back().unwrap();
        assert_eq!(7, entry.val);

        assert!(list.is_empty());
        assert!(list.pop_back().is_none());
    }

    #[test]
    fn remove_by_address() {
        let a = entry(5);
        let b = entry(7);
        let c = entry(31);

        unsafe {
            // Remove first
            let mut list = LinkedList::new();

            push_all(&mut list, &[c.as_ref(), b.as_ref(), a.as_ref()]);
            assert!(list.remove(ptr(&a)).is_some());
            assert_clean!(a);
            // `a` should be no longer there and can't be removed twice
            assert!(list.remove(ptr(&a)).is_none());
            assert!(!list.is_empty());

            assert!(list.remove(ptr(&b)).is_some());
            assert_clean!(b);
            // `b` should be no longer there and can't be removed twice
            assert!(list.remove(ptr(&b)).is_none());
            assert!(!list.is_empty());

            assert!(list.remove(ptr(&c)).is_some());
            assert_clean!(c);
            // `b` should be no longer there and can't be removed twice
            assert!(list.remove(ptr(&c)).is_none());
            assert!(list.is_empty());
        }

        unsafe {
            // Remove middle
            let mut list = LinkedList::new();

            push_all(&mut list, &[c.as_ref(), b.as_ref(), a.as_ref()]);

            assert!(list.remove(ptr(&a)).is_some());
            assert_clean!(a);

            assert_ptr_eq!(b, list.head);
            assert_ptr_eq!(c, b.pointers.get_next());
            assert_ptr_eq!(b, c.pointers.get_prev());

            let items = collect_list(&mut list);
            assert_eq!([31, 7].to_vec(), items);
        }

        unsafe {
            // Remove middle
            let mut list = LinkedList::new();

            push_all(&mut list, &[c.as_ref(), b.as_ref(), a.as_ref()]);

            assert!(list.remove(ptr(&b)).is_some());
            assert_clean!(b);

            assert_ptr_eq!(c, a.pointers.get_next());
            assert_ptr_eq!(a, c.pointers.get_prev());

            let items = collect_list(&mut list);
            assert_eq!([31, 5].to_vec(), items);
        }

        unsafe {
            // Remove last
            // Remove middle
            let mut list = LinkedList::new();

            push_all(&mut list, &[c.as_ref(), b.as_ref(), a.as_ref()]);

            assert!(list.remove(ptr(&c)).is_some());
            assert_clean!(c);

            assert!(b.pointers.get_next().is_none());
            assert_ptr_eq!(b, list.tail);

            let items = collect_list(&mut list);
            assert_eq!([7, 5].to_vec(), items);
        }

        unsafe {
            // Remove first of two
            let mut list = LinkedList::new();

            push_all(&mut list, &[b.as_ref(), a.as_ref()]);

            assert!(list.remove(ptr(&a)).is_some());

            assert_clean!(a);

            // a should be no longer there and can't be removed twice
            assert!(list.remove(ptr(&a)).is_none());

            assert_ptr_eq!(b, list.head);
            assert_ptr_eq!(b, list.tail);

            assert!(b.pointers.get_next().is_none());
            assert!(b.pointers.get_prev().is_none());

            let items = collect_list(&mut list);
            assert_eq!([7].to_vec(), items);
        }

        unsafe {
            // Remove last of two
            let mut list = LinkedList::new();

            push_all(&mut list, &[b.as_ref(), a.as_ref()]);

            assert!(list.remove(ptr(&b)).is_some());

            assert_clean!(b);

            assert_ptr_eq!(a, list.head);
            assert_ptr_eq!(a, list.tail);

            assert!(a.pointers.get_next().is_none());
            assert!(a.pointers.get_prev().is_none());

            let items = collect_list(&mut list);
            assert_eq!([5].to_vec(), items);
        }

        unsafe {
            // Remove last item
            let mut list = LinkedList::new();

            push_all(&mut list, &[a.as_ref()]);

            assert!(list.remove(ptr(&a)).is_some());
            assert_clean!(a);

            assert!(list.head.is_none());
            assert!(list.tail.is_none());
            let items = collect_list(&mut list);
            assert!(items.is_empty());
        }

        unsafe {
            // Remove missing
            let mut list = LinkedList::<&Entry, <&Entry as Link>::Target>::new();

            list.push_front(b.as_ref());
            list.push_front(a.as_ref());

            assert!(list.remove(ptr(&c)).is_none());
        }
    }

    /// This is a fuzz test. You run it by entering `cargo fuzz run fuzz_linked_list` in CLI in `/tokio/` module.
    #[cfg(fuzzing)]
    pub fn fuzz_linked_list(ops: &[u8]) {
        enum Op {
            Push,
            Pop,
            Remove(usize),
        }
        use std::collections::VecDeque;

        let ops = ops
            .iter()
            .map(|i| match i % 3u8 {
                0 => Op::Push,
                1 => Op::Pop,
                2 => Op::Remove((i / 3u8) as usize),
                _ => unreachable!(),
            })
            .collect::<Vec<_>>();

        let mut ll = LinkedList::<&Entry, <&Entry as Link>::Target>::new();
        let mut reference = VecDeque::new();

        let entries: Vec<_> = (0..ops.len()).map(|i| entry(i as i32)).collect();

        for (i, op) in ops.iter().enumerate() {
            match op {
                Op::Push => {
                    reference.push_front(i as i32);
                    assert_eq!(entries[i].val, i as i32);

                    ll.push_front(entries[i].as_ref());
                }
                Op::Pop => {
                    if reference.is_empty() {
                        assert!(ll.is_empty());
                        continue;
                    }

                    let v = reference.pop_back();
                    assert_eq!(v, ll.pop_back().map(|v| v.val));
                }
                Op::Remove(n) => {
                    if reference.is_empty() {
                        assert!(ll.is_empty());
                        continue;
                    }

                    let idx = n % reference.len();
                    let expect = reference.remove(idx).unwrap();

                    unsafe {
                        let entry = ll.remove(ptr(&entries[expect as usize])).unwrap();
                        assert_eq!(expect, entry.val);
                    }
                }
            }
        }
    }
}
