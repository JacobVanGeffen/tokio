use crate::runtime::task::RawTask;

use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

cfg_rt! {
    /// An owned permission to join on a task (await its termination).
    ///
    /// This can be thought of as the equivalent of [`std::thread::JoinHandle`] for
    /// a task rather than a thread.
    ///
    /// A `JoinHandle` *detaches* the associated task when it is dropped, which
    /// means that there is no longer any handle to the task, and no way to `join`
    /// on it.
    ///
    /// This `struct` is created by the [`task::spawn`] and [`task::spawn_blocking`]
    /// functions.
    ///
    /// # Examples
    ///
    /// Creation from [`task::spawn`]:
    ///
    /// ```
    /// use tokio::task;
    ///
    /// # async fn doc() {
    /// let join_handle: task::JoinHandle<_> = task::spawn(async {
    ///     // some work here
    /// });
    /// # }
    /// ```
    ///
    /// Creation from [`task::spawn_blocking`]:
    ///
    /// ```
    /// use tokio::task;
    ///
    /// # async fn doc() {
    /// let join_handle: task::JoinHandle<_> = task::spawn_blocking(|| {
    ///     // some blocking work here
    /// });
    /// # }
    /// ```
    ///
    /// The generic parameter `T` in `JoinHandle<T>` is the return type of the spawned task.
    /// If the return value is an i32, the join handle has type `JoinHandle<i32>`:
    ///
    /// ```
    /// use tokio::task;
    ///
    /// # async fn doc() {
    /// let join_handle: task::JoinHandle<i32> = task::spawn(async {
    ///     5 + 3
    /// });
    /// # }
    ///
    /// ```
    ///
    /// If the task does not have a return value, the join handle has type `JoinHandle<()>`:
    ///
    /// ```
    /// use tokio::task;
    ///
    /// # async fn doc() {
    /// let join_handle: task::JoinHandle<()> = task::spawn(async {
    ///     println!("I return nothing.");
    /// });
    /// # }
    /// ```
    ///
    /// Note that `handle.await` doesn't give you the return type directly. It is wrapped in a
    /// `Result` because panics in the spawned task are caught by Tokio. The `?` operator has
    /// to be double chained to extract the returned value:
    ///
    /// ```
    /// use tokio::task;
    /// use std::io;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     let join_handle: task::JoinHandle<Result<i32, io::Error>> = tokio::spawn(async {
    ///         Ok(5 + 3)
    ///     });
    ///
    ///     let result = join_handle.await??;
    ///     assert_eq!(result, 8);
    ///     Ok(())
    /// }
    /// ```
    ///
    /// If the task panics, the error is a [`JoinError`] that contains the panic:
    ///
    /// ```
    /// use tokio::task;
    /// use std::io;
    /// use std::panic;
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     let join_handle: task::JoinHandle<Result<i32, io::Error>> = tokio::spawn(async {
    ///         panic!("boom");
    ///     });
    ///
    ///     let err = join_handle.await.unwrap_err();
    ///     assert!(err.is_panic());
    ///     Ok(())
    /// }
    ///
    /// ```
    /// Child being detached and outliving its parent:
    ///
    /// ```no_run
    /// use tokio::task;
    /// use tokio::time;
    /// use std::time::Duration;
    ///
    /// # #[tokio::main] async fn main() {
    /// let original_task = task::spawn(async {
    ///     let _detached_task = task::spawn(async {
    ///         // Here we sleep to make sure that the first task returns before.
    ///         time::sleep(Duration::from_millis(10)).await;
    ///         // This will be called, even though the JoinHandle is dropped.
    ///         println!("♫ Still alive ♫");
    ///     });
    /// });
    ///
    /// original_task.await.expect("The task being joined has panicked");
    /// println!("Original task is joined.");
    ///
    /// // We make sure that the new task has time to run, before the main
    /// // task returns.
    ///
    /// time::sleep(Duration::from_millis(1000)).await;
    /// # }
    /// ```
    ///
    /// [`task::spawn`]: crate::task::spawn()
    /// [`task::spawn_blocking`]: crate::task::spawn_blocking
    /// [`std::thread::JoinHandle`]: std::thread::JoinHandle
    /// [`JoinError`]: crate::task::JoinError
    pub struct JoinHandle<T> {
        // TODO Make this one Enum instead
        async_handle: Option<shuttle::asynch::JoinHandle<T>>,
        thread_handle: Option<shuttle::thread::JoinHandle<T>>,
        _p: PhantomData<T>,
    }
}

unsafe impl<T: Send> Send for JoinHandle<T> {}
unsafe impl<T: Send> Sync for JoinHandle<T> {}

// NOTE: This is basically the same as runtime::blocking::NoopSchedule
struct NoopSchedule;
impl crate::runtime::task::Schedule for NoopSchedule {
    fn bind(_task: crate::runtime::task::Task<Self>) -> Self {
        NoopSchedule
    }

    fn release(&self, _task: &crate::runtime::task::Task<Self>) -> Option<crate::runtime::task::Task<Self>> {
        None
    }

    fn schedule(&self, _task: crate::runtime::task::Notified<Self>) {
        unreachable!();
    }
}

impl<T> JoinHandle<T> {
    pub(super) fn new(raw: Option<RawTask>) -> JoinHandle<T> {
        JoinHandle {
            async_handle: None,
            thread_handle: None,
            _p: PhantomData,
        }
    }

    pub(crate) fn new_async_handle(shuttle_handle: shuttle::asynch::JoinHandle<T>) -> JoinHandle<T> {
        /*
        struct ShuttleFuture<S> { inner: shuttle::asynch::JoinHandle<S> }
        impl<S> Future for ShuttleFuture<S> {
            type Output = super::Result<S>;

            fn poll(self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
                if let Some(result) = self.inner.result() {
                    Poll::Ready(
                        match result {
                            Ok(res) => Ok(res),
                            Err(shuttle::asynch::JoinError::Cancelled) => Err(super::JoinError::cancelled()),
                            // TODO other error cases when implemented
                        }
                    )
                } else {
                    Poll::Pending
                }
            }
        }
        JoinHandle::new(RawTask::new::<ShuttleFuture<T>, NoopSchedule>(ShuttleFuture { inner: shuttle_handle }))
        */
        JoinHandle {
            async_handle: Some(shuttle_handle),
            thread_handle: None,
            _p: PhantomData,
        }
    }

    // NOTE: This is for blocking spawn
    pub(crate) fn new_thread_handle(shuttle_handle: shuttle::thread::JoinHandle<T>) -> JoinHandle<T> {
        /*
        struct ShuttleFuture<S> { inner: shuttle::thread::JoinHandle<S> }
        impl<S> Future for ShuttleFuture<S> {
            type Output = super::Result<S>;

            fn poll(self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
                println!("Polled blocking thread handle");
                if let Some(result) = self.inner.result() {
                    Poll::Ready(
                        match result {
                            Ok(res) => Ok(res),
                            Err(err) => Err(super::JoinError::panic(err)),
                            // TODO other error cases when implemented
                        }
                    )
                } else {
                    Poll::Pending
                }
            }
        }
        JoinHandle::new(RawTask::new::<ShuttleFuture<T>, NoopSchedule>(ShuttleFuture { inner: shuttle_handle }))
        */
        JoinHandle {
            async_handle: None,
            thread_handle: Some(shuttle_handle),
            _p: PhantomData,
        }
    }

    /// Abort the task associated with the handle.
    ///
    /// Awaiting a cancelled task might complete as usual if the task was
    /// already completed at the time it was cancelled, but most likely it
    /// will complete with a `Err(JoinError::Cancelled)`.
    ///
    /// ```rust
    /// use tokio::time;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///    let mut handles = Vec::new();
    ///
    ///    handles.push(tokio::spawn(async {
    ///       time::sleep(time::Duration::from_secs(10)).await;
    ///       true
    ///    }));
    ///
    ///    handles.push(tokio::spawn(async {
    ///       time::sleep(time::Duration::from_secs(10)).await;
    ///       false
    ///    }));
    ///
    ///    for handle in &handles {
    ///        handle.abort();
    ///    }
    ///
    ///    for handle in handles {
    ///        assert!(handle.await.unwrap_err().is_cancelled());
    ///    }
    /// }
    /// ```
    pub fn abort(&self) {
        /*
        if let Some(raw) = self.raw {
            raw.shutdown();
        }
        */
    }
}

impl<T> Unpin for JoinHandle<T> {}

impl<T> Future for JoinHandle<T> {
    type Output = super::Result<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        println!("Polled tokio JoinHandle");
        /*
        let mut ret = Poll::Pending;

        // Keep track of task budget
        let coop = ready!(crate::coop::poll_proceed(cx));

        // Raw should always be set. If it is not, this is due to polling after
        // completion
        let raw = self
            .raw
            .as_ref()
            .expect("polling after `JoinHandle` already completed");

        // Poll the raw (since shuttle is managing this future)
        raw.poll();

        // Try to read the task output. If the task is not yet complete, the
        // waker is stored and is notified once the task does complete.
        //
        // The function must go via the vtable, which requires erasing generic
        // types. To do this, the function "return" is placed on the stack
        // **before** calling the function and is passed into the function using
        // `*mut ()`.
        //
        // Safety:
        //
        // The type of `T` must match the task's output type.
        unsafe {
            raw.try_read_output(&mut ret as *mut _ as *mut (), cx.waker());
        }

        if ret.is_ready() {
            coop.made_progress();
        }

        ret
        */
        if let Some(handle) = self.async_handle.as_ref() {
            assert!(self.thread_handle.is_none());
            if let Some(result) = handle.result() {
                Poll::Ready(
                    match result {
                        Ok(res) => Ok(res),
                        Err(shuttle::asynch::JoinError::Cancelled) => Err(super::JoinError::cancelled()),
                        // TODO other error cases when implemented
                    }
                )
            } else {
                Poll::Pending
            }
        } else if let Some(handle) = self.thread_handle.as_ref() {
            assert!(self.async_handle.is_none());
            // On this branch, we're waiting for a blocking operation
            //shuttle::thread::yield_now();
            //while handle.result().is_none() {}
            // Want to set a waiter here
            if let Some(result) = handle.result() {
                Poll::Ready(
                    match result {
                        Ok(res) => Ok(res),
                        Err(err) => Err(super::JoinError::panic(err)),
                        // TODO other error cases when implemented
                    }
                )
            } else {
                handle.set_waiter();
                Poll::Pending
            }
        } else {
            unreachable!()
        }
    }
}

impl<T> Drop for JoinHandle<T> {
    fn drop(&mut self) {
        /*
        if let Some(raw) = self.raw.take() {
            if raw.header().state.drop_join_handle_fast().is_ok() {
                return;
            }

            raw.drop_join_handle_slow();
        }
        */
    }
}

impl<T> fmt::Debug for JoinHandle<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("JoinHandle").finish()
    }
}
