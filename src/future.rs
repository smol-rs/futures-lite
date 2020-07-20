//! Combinators for the [`Future`] trait.
//!
//! # Examples
//!
//! ```
//! use futures_lite::*;
//!
//! # blocking::block_on(async {
//! for step in 0..3 {
//!     println!("step {}", step);
//!
//!     // Give other tasks a chance to run.
//!     future::yield_now().await;
//! }
//! # });
//! ```

// TODO: race(), race!, try_race(), try_race! (randomized for fairness)
// TODO: join!, try_join!

use std::fmt;
#[doc(no_inline)]
pub use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::future::{BoxFuture, LocalBoxFuture};
use pin_project_lite::pin_project;

/// Creates a future that is always pending.
///
/// # Examples
///
/// ```no_run
/// use futures_lite::*;
///
/// # blocking::block_on(async {
/// future::pending::<()>().await;
/// unreachable!();
/// # })
/// ```
pub fn pending<T>() -> Pending<T> {
    Pending {
        _marker: PhantomData,
    }
}

/// Future for the [`pending()`] function.
pub struct Pending<T> {
    _marker: PhantomData<T>,
}

impl<T> Unpin for Pending<T> {}

impl<T> fmt::Debug for Pending<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pending").finish()
    }
}

impl<T> Future for Pending<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<T> {
        Poll::Pending
    }
}

/// Polls a future just once and returns an [`Option`] with the result.
///
/// # Examples
///
/// ```
/// use futures_lite::*;
///
/// # blocking::block_on(async {
/// assert_eq!(future::poll_once(future::pending::<()>()).await, None);
/// assert_eq!(future::poll_once(future::ready(42)).await, Some(42));
/// # })
/// ```
pub fn poll_once<T, F>(f: F) -> PollOnce<F>
where
    F: Future<Output = T>,
{
    PollOnce { f }
}

pin_project! {
    /// Future for the [`poll_once()`] function.
    pub struct PollOnce<F> {
        #[pin]
        f: F,
    }
}

impl<F> fmt::Debug for PollOnce<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PollOnce").finish()
    }
}

impl<T, F> Future for PollOnce<F>
where
    F: Future<Output = T>,
{
    type Output = Option<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        match Pin::new(&mut this.f).poll(cx) {
            Poll::Ready(t) => Poll::Ready(Some(t)),
            Poll::Pending => Poll::Ready(None),
        }
    }
}

/// Creates a future from a function returning [`Poll`].
///
/// # Examples
///
/// ```
/// use futures_lite::*;
/// use std::task::{Context, Poll};
///
/// # blocking::block_on(async {
/// fn f(_: &mut Context<'_>) -> Poll<i32> {
///     Poll::Ready(7)
/// }
///
/// assert_eq!(future::poll_fn(f).await, 7);
/// # })
/// ```
pub fn poll_fn<T, F>(f: F) -> PollFn<F>
where
    F: FnMut(&mut Context<'_>) -> Poll<T>,
{
    PollFn { f }
}

pin_project! {
    /// Future for the [`poll_fn()`] function.
    pub struct PollFn<F> {
        f: F,
    }
}

impl<F> fmt::Debug for PollFn<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PollFn").finish()
    }
}

impl<T, F> Future for PollFn<F>
where
    F: FnMut(&mut Context<'_>) -> Poll<T>,
{
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        let this = self.project();
        (this.f)(cx)
    }
}

/// Creates a future that resolves to the provided value.
///
/// # Examples
///
/// ```
/// use futures_lite::*;
///
/// # blocking::block_on(async {
/// assert_eq!(future::ready(7).await, 7);
/// # })
/// ```
pub fn ready<T>(val: T) -> Ready<T> {
    Ready(Some(val))
}

/// Future for the [`ready()`] function.
#[derive(Debug)]
pub struct Ready<T>(Option<T>);

impl<T> Unpin for Ready<T> {}

impl<T> Future for Ready<T> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<T> {
        Poll::Ready(self.0.take().expect("`Ready` polled after completion"))
    }
}

/// Wakes the current task and returns [`Poll::Pending`] once.
///
/// This function is useful when we want to cooperatively give time to the task scheduler. It is
/// generally a good idea to yield inside loops because that way we make sure long-running tasks
/// don't prevent other tasks from running.
///
/// # Examples
///
/// ```
/// use futures_lite::*;
///
/// # blocking::block_on(async {
/// future::yield_now().await;
/// # })
/// ```
pub fn yield_now() -> YieldNow {
    YieldNow(false)
}

/// Future for the [`yield_now()`] function.
#[derive(Debug)]
pub struct YieldNow(bool);

impl Future for YieldNow {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if !self.0 {
            self.0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}

/// Joins two futures, waiting for both to complete.
///
/// # Examples
///
/// ```
/// use futures_lite::*;
///
/// # blocking::block_on(async {
/// let a = async { 1 };
/// let b = async { 2 };
///
/// assert_eq!(future::join(a, b).await, (1, 2));
/// # })
/// ```
pub fn join<Fut1, Fut2>(future1: Fut1, future2: Fut2) -> Join<Fut1, Fut2>
where
    Fut1: Future,
    Fut2: Future,
{
    Join {
        future1: future1,
        output1: None,
        future2: future2,
        output2: None,
    }
}

pin_project! {
    /// Future for the [`join()`] function.
    #[derive(Debug)]
    pub struct Join<Fut1, Fut2>
    where
        Fut1: Future,
        Fut2: Future,
    {
        #[pin]
        future1: Fut1,
        output1: Option<Fut1::Output>,
        #[pin]
        future2: Fut2,
        output2: Option<Fut2::Output>,
    }
}

impl<Fut1, Fut2> Future for Join<Fut1, Fut2>
where
    Fut1: Future,
    Fut2: Future,
{
    type Output = (Fut1::Output, Fut2::Output);

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        if this.output1.is_none() {
            if let Poll::Ready(out) = this.future1.poll(cx) {
                *this.output1 = Some(out);
            }
        }

        if this.output2.is_none() {
            if let Poll::Ready(out) = this.future2.poll(cx) {
                *this.output2 = Some(out);
            }
        }

        if this.output1.is_some() && this.output2.is_some() {
            Poll::Ready((this.output1.take().unwrap(), this.output2.take().unwrap()))
        } else {
            Poll::Pending
        }
    }
}

/// Joins two fallible futures, waiting for both to complete or one of them to error.
///
/// # Examples
///
/// ```
/// use futures_lite::*;
///
/// # blocking::block_on(async {
/// let a = async { Ok::<i32, i32>(1) };
/// let b = async { Err::<i32, i32>(2) };
///
/// assert_eq!(future::try_join(a, b).await, Err(2));
/// # })
/// ```
pub fn try_join<T1, T2, E, Fut1, Fut2>(future1: Fut1, future2: Fut2) -> TryJoin<Fut1, Fut2>
where
    Fut1: Future<Output = Result<T1, E>>,
    Fut2: Future<Output = Result<T2, E>>,
{
    TryJoin {
        future1: future1,
        output1: None,
        future2: future2,
        output2: None,
    }
}

pin_project! {
    /// Future for the [`try_join()`] function.
    #[derive(Debug)]
    pub struct TryJoin<Fut1, Fut2>
    where
        Fut1: Future,
        Fut2: Future,
    {
        #[pin]
        future1: Fut1,
        output1: Option<Fut1::Output>,
        #[pin]
        future2: Fut2,
        output2: Option<Fut2::Output>,
    }
}

impl<T1, T2, E, Fut1, Fut2> Future for TryJoin<Fut1, Fut2>
where
    Fut1: Future<Output = Result<T1, E>>,
    Fut2: Future<Output = Result<T2, E>>,
{
    type Output = Result<(T1, T2), E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        if this.output1.is_none() {
            if let Poll::Ready(out) = this.future1.poll(cx) {
                match out {
                    Ok(t) => *this.output1 = Some(Ok(t)),
                    Err(err) => return Poll::Ready(Err(err)),
                }
            }
        }

        if this.output2.is_none() {
            if let Poll::Ready(out) = this.future2.poll(cx) {
                match out {
                    Ok(t) => *this.output2 = Some(Ok(t)),
                    Err(err) => return Poll::Ready(Err(err)),
                }
            }
        }

        if this.output1.is_some() && this.output2.is_some() {
            let res1 = this.output1.take().unwrap();
            let res2 = this.output2.take().unwrap();
            let t1 = res1.map_err(|_| unreachable!()).unwrap();
            let t2 = res2.map_err(|_| unreachable!()).unwrap();
            Poll::Ready(Ok((t1, t2)))
        } else {
            Poll::Pending
        }
    }
}

/// Returns the result of the future that completes first.
///
/// Each time [`Race`] is polled, the two inner futures are polled in random order. Therefore, no
/// future takes precedence over the other if both can complete at the same time.
///
/// # Examples
///
/// ```
/// use futures_lite::*;
///
/// # blocking::block_on(async {
/// let a = future::pending();
/// let b = future::ready(7);
///
/// assert_eq!(future::race(a, b).await, 7);
/// # })
/// ```
pub fn race<T, A, B>(future1: A, future2: B) -> Race<A, B>
where
    A: Future<Output = T>,
    B: Future<Output = T>,
{
    Race { future1, future2 }
}

pin_project! {
    /// Future for the [`race()`] function.
    #[derive(Debug)]
    pub struct Race<A, B> {
        #[pin]
        future1: A,
        #[pin]
        future2: B,
    }
}

impl<T, A, B> Future for Race<A, B>
where
    A: Future<Output = T>,
    B: Future<Output = T>,
{
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        if fastrand::bool() {
            if let Poll::Ready(t) = this.future1.poll(cx) {
                return Poll::Ready(t);
            }
            if let Poll::Ready(t) = this.future2.poll(cx) {
                return Poll::Ready(t);
            }
        } else {
            if let Poll::Ready(t) = this.future1.poll(cx) {
                return Poll::Ready(t);
            }
            if let Poll::Ready(t) = this.future2.poll(cx) {
                return Poll::Ready(t);
            }
        }
        Poll::Pending
    }
}

impl<T: ?Sized> FutureExt for T where T: Future {}

/// Extension trait for [`Future`].
pub trait FutureExt: Future {
    /// Wrap the future in a Box, pinning it.
    ///
    /// # Examples
    ///
    /// ```
    /// use futures_lite::*;
    ///
    /// # blocking::block_on(async {
    /// async fn select<F: Future + Send>(_a: F, _b: F) { }
    ///
    /// let a = future::ready('a');
    /// let b = future::pending();
    ///
    /// select(a.boxed(), b.boxed());
    /// # })
    /// ```
    fn boxed<'a>(self) -> BoxFuture<'a, Self::Output>
    where
        Self: Sized + Send + 'a,
    {
        Box::pin(self)
    }

    /// Wrap the future in a Box which can't be sent to threads, pinning it.
    ///
    /// # Examples
    ///
    /// ```
    /// use futures_lite::*;
    ///
    /// # blocking::block_on(async {
    /// async fn select_no_send<F: Future>(_a: F, _b: F) { }
    ///
    /// let a = future::ready('a');
    /// let b = future::pending();
    ///
    /// select_no_send(a.boxed_local(), b.boxed_local());
    /// # })
    /// ```
    fn boxed_local<'a>(self) -> LocalBoxFuture<'a, Self::Output>
    where
        Self: Sized + 'a,
    {
        Box::pin(self)
    }
}
