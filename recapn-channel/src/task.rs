//! Tasks with some associated data.
//! 
//! This module contains types for "data tasks", or futures with some data associated with them.
//! The main use case for data tasks is as part of a "data task set", which allows you to remove
//! futures from the set after they're added.
//! 
//! This is extremely similar to FuturesUnordered, but the integration of task and task set allows
//! for operations like task removal.
//! 
//! When you create a data task, you can choose to get a separate data ref which allows you to
//! access the data associated with the task separately. When you put the data task into a task set
//! you can later remove that task from the set using one of the previously created data refs.
//! Joining a task out of the task set returns the output of the task along with the task's data
//! ref. This way, you can store associated data that will be needed when the task finishes.
//! 
//! This is used by recapn-rpc for connection tasks. It's optimized for event oriented futures such
//! as requests finishing. Futures which perform work are not ideal for this task system.

use std::borrow::Borrow;
use std::cell::UnsafeCell;
use std::fmt::{self, Debug};
use std::future::{Future, poll_fn};
use std::marker::PhantomPinned;
use std::mem::ManuallyDrop;
use std::ops::Deref;
use std::pin::Pin;
use std::ptr::{NonNull, addr_of_mut};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Wake, Waker};

use parking_lot::Mutex;
use scopeguard::ScopeGuard;

use crate::util::atomic_option_arc::AtomicOptionArc;
use crate::util::linked_list::{Link, LinkedList, Pointers};

struct TaskInner<T, F> {
    /// The parent task set that owns this task.
    parent: AtomicOptionArc<SharedTaskSet<T, F>>,

    /// The number of refs that can access the data associated with this task. When this count
    /// reaches 0, the data is released.
    /// 
    /// This is so we can control when the data itself is released and allows us to make the task
    /// itself Send + Sync for wakers.
    ref_count: AtomicUsize,

    task_set_pointers: Pointers<TaskInner<T, F>>,

    /// A bool indicating whether the task is ready in the parent task set.
    ready: UnsafeCell<bool>,

    /// The data associated with the task
    data: UnsafeCell<ManuallyDrop<T>>,

    future: UnsafeCell<ManuallyDrop<F>>,
}

fn task_vtable<T, F>() -> &'static RawWakerVTable {
    unsafe fn clone<T, F>(ptr: *const ()) -> RawWaker {
        Arc::increment_strong_count(ptr.cast::<TaskInner<T, F>>());
        RawWaker::new(ptr, task_vtable::<T, F>())
    }

    unsafe fn wake<T, F>(ptr: *const ()) {
        wake_by_ref::<T, F>(ptr);
        drop::<T, F>(ptr);
    }

    unsafe fn wake_by_ref<T, F>(ptr: *const ()) {
        let ptr = ptr.cast::<TaskInner<T, F>>();
        (*ptr).wake();
    }

    unsafe fn drop<T, F>(ptr: *const ()) {
        Arc::decrement_strong_count(ptr.cast::<TaskInner<T, F>>());
    }

    &RawWakerVTable::new(clone::<T, F>, wake::<T, F>, wake_by_ref::<T, F>, drop::<T, F>)
}

impl<T, F> TaskInner<T, F> {
    /// Drop the future associated with this task
    /// 
    /// # Safety
    /// 
    /// This must be done by the DataTask that owns this task on drop or by the owning DataTaskSet
    /// when the future is joined and completed. It cannot be done by the SharedTask on drop since
    /// we've already asserted that F doesn't need to be Send or Sync so that the TaskInner can be
    /// used as a Waker.
    unsafe fn drop_future(&self) {
        let fut = &mut *self.future.get();
        ManuallyDrop::drop(fut);
    }

    fn add_ref(&self) {
        self.ref_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn drop_ref(&self) {
        let old = self.ref_count.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        if old - 1 == 0 {
            // All references to data have been dropped, we can now drop `data`.
            unsafe {
                let slot = &mut *self.data.get();
                ManuallyDrop::drop(slot)
            }
        }
    }

    fn wake(&self) {
        loop {
            let Some(parent) = self.parent.add_ref() else {
                // No parent
                return
            };

            let mut locked = parent.guarded.lock();
            if !self.parent.same_as(Some(&parent)) {
                continue
            }

            let is_notified = unsafe { std::mem::replace(&mut *self.ready.get(), true) };
            if !is_notified {
                let ptr = NonNull::from_ref(self);
                unsafe {
                    locked.idle.remove(ptr);
                }
                locked.ready.push_back(TaskInSet { ptr });
            }

            let waker = locked.waker.take();
            drop(locked);
            if let Some(waker) = waker {
                waker.wake();
            }

            return
        }
    }
}

impl<T, F> Wake for TaskInner<T, F> {
    fn wake_by_ref(self: &Arc<Self>) {
        TaskInner::wake(self);
    }
    fn wake(self: Arc<Self>) {
        self.wake_by_ref()
    }
}

unsafe impl<T, F> Send for TaskInner<T, F> {}
unsafe impl<T, F> Sync for TaskInner<T, F> {}

type SharedTask<T, F> = Arc<TaskInner<T, F>>;

/// A future paired with some data. This data can be referenced independently through a DataRef.
pub struct DataTask<T, F> {
    inner: ManuallyDrop<SharedTask<T, F>>,
    _pinned: PhantomPinned,
}

impl<T, F> DataTask<T, F> {
    pub fn new(data: T, future: F) -> Self {
        let task = TaskInner {
            parent: AtomicOptionArc::none(),
            ref_count: AtomicUsize::new(1),
            data: UnsafeCell::new(ManuallyDrop::new(data)),
            task_set_pointers: Pointers::new(),
            ready: UnsafeCell::new(false),
            future: UnsafeCell::new(ManuallyDrop::new(future)),
        };

        Self::from_inner(Arc::new(task))
    }
    fn from_inner(task: SharedTask<T, F>) -> Self {
        Self { inner: ManuallyDrop::new(task), _pinned: PhantomPinned }
    }
    fn take_inner(mut self) -> SharedTask<T, F> {
        let inner = unsafe { ManuallyDrop::take(&mut self.inner) };
        std::mem::forget(self);
        inner
    }

    #[inline]
    pub fn data(&self) -> &T {
        unsafe { &**self.inner.data.get() }
    }

    /// Get a separate DataRef for the data associated with this task.
    pub fn data_ref(&self) -> DataRef<T, F> {
        DataRef::new(&self.inner)
    }
}

impl<T, F> Drop for DataTask<T, F> {
    fn drop(&mut self) {
        unsafe {
            self.inner.drop_ref();
            self.inner.drop_future();
            ManuallyDrop::drop(&mut self.inner)
        }
    }
}

impl<T, F: Future> Future for DataTask<T, F> {
    type Output = F::Output;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        let fut = unsafe { Pin::new_unchecked(&mut **self.inner.future.get()) };
        fut.poll(cx)
    }
}

unsafe impl<T: Send, F: Send> Send for DataTask<T, F> {}
// SAFETY:
// The future is exclusively accessed, so we don't need a Sync requirement on F
// but T can be accessed by other DataRef values, so it needs to be Sync.
unsafe impl<T: Sync, F> Sync for DataTask<T, F> {}

/// The data associated with a DataTask. This allows you to access the task's data separately from
/// the task itself.
pub struct DataRef<T, F> {
    inner: SharedTask<T, F>,
}

// SAFETY: F cannot be accessed from a DataRef, so it doesn't need Send or Sync constraints
unsafe impl<T: Send, F> Send for DataRef<T, F> {}
unsafe impl<T: Sync, F> Sync for DataRef<T, F> {}

impl<T, F> DataRef<T, F> {
    fn new(inner: &SharedTask<T, F>) -> Self {
        inner.add_ref();
        Self { inner: Arc::clone(inner) }
    }

    /// Get the data associated with this `DataRef`.
    #[inline]
    pub fn get(this: &Self) -> &T {
        unsafe { &**this.inner.data.get() }
    }

    /// Compare two `DataRef` instances to see if they point at the same task.
    #[inline]
    pub fn ptr_eq(this: &Self, other: &Self) -> bool {
        Arc::ptr_eq(&this.inner, &other.inner)
    }
}

impl<T, F> Deref for DataRef<T, F> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        Self::get(self)
    }
}

impl<T, F> AsRef<T> for DataRef<T, F> {
    #[inline]
    fn as_ref(&self) -> &T {
        Self::get(self)
    }
}

impl<T, F> Borrow<T> for DataRef<T, F> {
    #[inline]
    fn borrow(&self) -> &T {
        Self::get(self)
    }
}

impl<T, F> Clone for DataRef<T, F> {
    fn clone(&self) -> Self {
        Self::new(&self.inner)
    }
}

impl<T, F> Drop for DataRef<T, F> {
    fn drop(&mut self) {
        self.inner.drop_ref();
    }
}

impl<T: Debug, F> Debug for DataRef<T, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DataRef")
            .field("inner", &**self)
            .finish_non_exhaustive()
    }
}

struct TaskInSet<T, F> {
    ptr: NonNull<TaskInner<T, F>>,
}

unsafe impl<T, F> Link for TaskInSet<T, F> {
    type Handle = Self;
    type Target = TaskInner<T, F>;

    fn as_raw(handle: &Self::Handle) -> NonNull<Self::Target> {
        handle.ptr
    }
    unsafe fn from_raw(ptr: NonNull<Self::Target>) -> Self::Handle {
        Self { ptr }
    }
    unsafe fn pointers(target: NonNull<Self::Target>) -> NonNull<Pointers<Self::Target>> {
        let me = target.as_ptr();
        let field = addr_of_mut!((*me).task_set_pointers);
        NonNull::new_unchecked(field)
    }
}

struct GuardedTaskSet<T, F> {
    idle: LinkedList<TaskInSet<T, F>, TaskInner<T, F>>,
    ready: LinkedList<TaskInSet<T, F>, TaskInner<T, F>>,
    waker: Option<Waker>,
}

struct SharedTaskSet<T, F> {
    guarded: Mutex<GuardedTaskSet<T, F>>,
}

impl<T, F> SharedTaskSet<T, F> {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            guarded: Mutex::new(GuardedTaskSet {
                idle: LinkedList::new(),
                ready: LinkedList::new(),
                waker: None,
            })
        })
    }
    /// Insert a task into the shared task set. Inserted tasks are automatically put into the ready
    /// state, whether or not they're actually ready. This lets us prepare for the task to be made
    /// ready on another poll of join_next. It's fine to poll a task for readiness even if it's not
    /// actually ready.
    fn insert(self: &Arc<Self>, task: DataTask<T, F>) {
        let mut lock = self.guarded.lock();
        let task = task.take_inner();
        task.parent.replace(Some(Arc::clone(self)));
        unsafe {
            *task.ready.get() = true;
        }
        let ptr = Arc::into_raw(task);
        lock.ready.push_back(TaskInSet { ptr: NonNull::new(ptr.cast_mut()).unwrap() });
        if let Some(waker) = lock.waker.take() {
            waker.wake();
        }
    }
    fn remove(self: &Arc<Self>, task: &DataRef<T, F>) -> Option<DataTask<T, F>> {
        let mut lock = self.guarded.lock();
        if !task.inner.parent.same_as(Some(self)) {
            return None
        }

        task.inner.parent.replace(None);

        let is_ready = unsafe { *task.inner.ready.get() };
        let list = if is_ready {
            &mut lock.ready
        } else {
            &mut lock.idle
        };

        let task = unsafe {
            let task = list.remove(NonNull::from_ref(&task.inner)).unwrap();
            Arc::from_raw(task.ptr.as_ptr().cast_const())
        };

        Some(DataTask::from_inner(task))
    }
    fn remove_all(self: &Arc<Self>, len: usize) -> Vec<DataTask<T, F>> {
        let mut tasks = Vec::with_capacity(len);

        'outer: loop {
            let mut guarded = self.guarded.lock();
            // Only pull 10 items from the set at a time to give an opportunity for other operations
            // to occur on the set.
            for _ in 0..10 {
                let next = guarded.ready.pop_front().or_else(|| guarded.idle.pop_front());
                let Some(task) = next else {
                    break 'outer;
                };
                let task = unsafe { Arc::from_raw(task.ptr.as_ptr().cast_const()) };
                task.parent.clear();
                tasks.push(DataTask::from_inner(task));
            }
        }

        tasks
    }
}

impl<T, F: Future> SharedTaskSet<T, F> {
    fn poll_join<'a>(self: &'a Arc<Self>, cx: &mut Context<'_>) -> Poll<Option<(DataRef<T, F>, F::Output)>> {
        let waker = cx.waker();
        let mut guarded = self.guarded.lock();

        macro_rules! update_waker {
            () => {
                match guarded.waker.as_mut() {
                    Some(slot) => slot.clone_from(waker),
                    None => guarded.waker = Some(waker.clone()),
                }
            };
        }

        let Some(task) = guarded.ready.pop_front() else {
            return if guarded.idle.is_empty() {
                Poll::Ready(None)
            } else {
                update_waker!();
                Poll::Pending
            }
        };

        // If there's already more stuff ready, wake the new waker now so that we don't
        // have to wake it later.
        if !guarded.ready.is_empty() {
            waker.wake_by_ref();
            guarded.waker = None;
        } else {
            update_waker!();
        }

        let task_ptr = task.ptr;
        let task_ref = unsafe { task_ptr.as_ref() };

        unsafe {
            *task_ref.ready.get() = false;
        }
        // Push the task back into the idle list immediately. If it polls ready we'll remove it.
        // Note: it could be moved back into the ready list when we call the future's poll since
        // it might immediately wake the waker.
        guarded.idle.push_back(task);

        // Drop the guard at this point so that when we call the future we don't deadlock if it
        // calls back into us to wake.
        drop(guarded);

        let task_waker = {
            let ptr = task_ptr.as_ptr().cast_const();
            unsafe {
                Arc::increment_strong_count(ptr);
                Waker::from_raw(RawWaker::new(ptr.cast::<()>(), task_vtable::<T, F>()))
            }
        };

        let mut new_context = Context::from_waker(&task_waker);

        let future = unsafe {
            // terrible no good very bad
            Pin::new_unchecked(&mut **(*addr_of_mut!((*task_ptr.as_ptr()).future)).get())
        };

        let poll_guard = scopeguard::guard((), |()| {
            // If poll panics, we need to remove the task from the set and drop it.
            let mut guarded = self.guarded.lock();
            let is_ready = unsafe { *task_ref.ready.get() };
            let list = if is_ready {
                &mut guarded.ready
            } else {
                &mut guarded.idle
            };
            unsafe {
                list.remove(NonNull::from_ref(task_ref)).unwrap();
            }
            task_ref.parent.clear();

            let task = unsafe { Arc::from_raw(task_ref) };
            drop(DataTask::from_inner(task));
        });

        let poll = future.poll(&mut new_context);
        ScopeGuard::into_inner(poll_guard);

        let Poll::Ready(output) = poll else {
            return Poll::Pending
        };

        // We're ready! We can now drop the future and return a data ref
        let mut guarded = self.guarded.lock();

        // Remove the task from the set.
        let is_ready = unsafe { *task_ref.ready.get() };
        let list = if is_ready {
            &mut guarded.ready
        } else {
            &mut guarded.idle
        };
        unsafe {
            list.remove(NonNull::from_ref(task_ref)).unwrap();
        }
        task_ref.parent.clear();

        drop(guarded);

        let task = unsafe { Arc::from_raw(task_ref) };
        // We purposefully create a DataRef directly to reuse the ref count from the
        // DataTask. DataTask inherently has 1 ref to the data and new will add a ref
        // on top of that, but we want to reuse the 1 ref.
        let data_ref = DataRef { inner: task };
        unsafe {
            task_ref.drop_future();
        }

        Poll::Ready(Some((data_ref, output)))
    }
}

/// An unordered future set where every task has some associated data.
pub struct DataTaskSet<T, F> {
    shared: Arc<SharedTaskSet<T, F>>,
    len: usize,
}

impl<T, F> DataTaskSet<T, F> {
    /// Create a new task set.
    #[inline]
    pub fn new() -> Self {
        Self {
            shared: SharedTaskSet::new(),
            len: 0,
        }
    }

    /// The number of tasks in the set.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns whether the task set is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Inserts a new data task into the set. This consumes the task and does not give
    /// a `DataRef` for the consumed task.
    pub fn insert(&mut self, task: DataTask<T, F>) {
        self.len += 1;
        self.shared.insert(task);
    }

    /// Remove a task from the set, if it's in the set. If the task is not in the set this
    /// returns None.
    pub fn remove(&mut self, data: &DataRef<T, F>) -> Option<DataTask<T, F>> {
        let task = self.shared.remove(data);
        if task.is_some() {
            self.len -= 1;
        }
        task
    }

    /// Remove all tasks from the set and store them in a `Vec`.
    pub fn remove_all(&mut self) -> Vec<DataTask<T, F>> {
        self.shared.remove_all(std::mem::replace(&mut self.len, 0))
    }
}

impl<T, F: Future> DataTaskSet<T, F> {
    /// Return a future awaiting the completion of the next task in the set.
    /// 
    /// # Cancel safety
    /// 
    /// This function is cancel safe. If the returned future is dropped it is guaranteed that
    /// no futures will be lost.
    pub async fn join_next(&mut self) -> Option<(DataRef<T, F>, F::Output)> {
        poll_fn(|cx| self.poll_next(cx)).await
    }

    /// Poll the task set. If a future returns Ready, this returns a `DataRef` for the task along
    /// with the future's output.
    pub fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<Option<(DataRef<T, F>, F::Output)>> {
        let poll = self.shared.poll_join(cx);
        if matches!(poll, Poll::Ready(Some(_))) {
            self.len -= 1;
        }
        poll
    }
}

impl<T, F> Drop for DataTaskSet<T, F> {
    fn drop(&mut self) {
        drop(self.remove_all());
    }
}

// SAFETY: This uses the same requirements as DataTask since it's used the same way as DataTask.
unsafe impl<T: Send, F: Send> Send for DataTaskSet<T, F> {}
unsafe impl<T: Sync, F> Sync for DataTaskSet<T, F> {}

#[cfg(test)]
mod test {
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};
    use std::{cell::Cell, pin::pin};
    use std::future::Future;
    use tokio_test::{assert_pending, assert_ready};
    use tokio_test::task::spawn;

    use crate::task::{DataTask, DataTaskSet, DataRef};

    #[test]
    fn task_data() {
        //! Test to make sure the data in a DataTask lasts as long as
        //! the DataTask instance + all DataRef instances

        struct NoneOnDrop<'a>(&'a Cell<Option<i32>>);
        impl Drop for NoneOnDrop<'_> {
            fn drop(&mut self) {
                self.0.set(None);
            }
        }
        let data = Cell::new(Some(1));
        let data_task = DataTask::new(NoneOnDrop(&data), async {});
        let data_ref = data_task.data_ref();
        let data_ref2 = DataRef::clone(&data_ref);

        data_task.data().0.set(Some(2));
        assert_eq!(data_task.data().0.get(), Some(2));
        assert_eq!(data_task.data().0.get(), data_ref.0.get());

        drop(data_task);
        assert_eq!(data_ref.0.get(), Some(2));

        drop(data_ref);
        assert_eq!(data.get(), Some(2));

        drop(data_ref2);
        assert_eq!(data.get(), None);
    }

    #[test]
    fn future_data() {
        //! Test to make sure the future is dropped when the DataTask is dropped, not when all
        //! DataRefs are dropped

        struct TrueOnDrop<'a>(&'a Cell<bool>);
        impl Drop for TrueOnDrop<'_> {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }
        let dropped = Cell::new(false);
        let true_on_drop = TrueOnDrop(&dropped);
        let data_task = DataTask::new((), async move { true_on_drop });
        let data_ref = data_task.data_ref();

        assert_eq!(dropped.get(), false);
        drop(data_task);

        assert!(dropped.get());

        drop(data_ref);
        assert_eq!(dropped.get(), true);
    }

    #[test]
    fn poll_task() {
        let value = 23;
        let task = DataTask::new((), async move { value });
        let pin_task = pin!(task);
        let poll = pin_task.poll(&mut Context::from_waker(Waker::noop()));
        assert!(matches!(poll, Poll::Ready(23)));
    }

    #[test]
    fn task_set() {
        let mut tasks = DataTaskSet::<(), Pin<Box<dyn Future<Output = ()>>>>::new();

        // With no tasks in the set, calling join_next immediately returns None.
        {
            let mut join_next = spawn(tasks.join_next());
            let result = assert_ready!(join_next.poll());
            assert!(matches!(result, None));
        }

        // Insert a ready task in the set and join it
        {
            let ready_task = DataTask::new((), Box::pin(async {}) as _);
            let task_ref = ready_task.data_ref();
            tasks.insert(ready_task);

            let mut join_next = spawn(tasks.join_next());
            let (joined_ref, ()) = assert_ready!(join_next.poll()).unwrap();

            assert!(DataRef::ptr_eq(&task_ref, &joined_ref));
        }

        assert!(tasks.is_empty());

        // Insert two tasks in the set and join them.
        {
            let (send1, shot1) = tokio::sync::oneshot::channel::<()>();
            let (send2, shot2) = tokio::sync::oneshot::channel::<()>();

            let task1 = DataTask::new((), Box::pin(async move { let _ = shot1.await; }) as _);
            let ref1 = task1.data_ref();

            let task2 = DataTask::new((), Box::pin(async move { let _ = shot2.await; }) as _);
            let ref2 = task2.data_ref();

            // Both tasks are inserted as ready, so we'll have the first join task wake up twice to
            // properly poll them.
            tasks.insert(task1);
            tasks.insert(task2);

            {
                let mut join_next = spawn(tasks.join_next());
                assert_pending!(join_next.poll());

                // The task immediately wakes up since it has another ready task
                assert!(join_next.is_woken());

                // But the ready task actually is just another pending one.
                assert_pending!(join_next.poll());
                assert!(!join_next.is_woken());

                send1.send(()).unwrap();

                assert!(join_next.is_woken());
                let (joined_ref, ()) = assert_ready!(join_next.poll()).unwrap();
                assert!(DataRef::ptr_eq(&ref1, &joined_ref));
            }

            {
                let mut join_next = spawn(tasks.join_next());

                assert_pending!(join_next.poll());
                assert!(!join_next.is_woken());

                send2.send(()).unwrap();

                assert!(join_next.is_woken());

                let (joined_ref, ()) = assert_ready!(join_next.poll()).unwrap();
                assert!(DataRef::ptr_eq(&ref2, &joined_ref));
            }
        }
    }
}