use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

#[doc(no_inline)]
pub use futures_core::stream::Stream;
use pin_project_lite::pin_project;

use crate::ready;

/// Creates a stream that doesn't yield any items.
///
/// This `struct` is created by the [`empty`] function. See its
/// documentation for more.
///
/// [`empty`]: fn.empty.html
///
/// # Examples
///
/// ```
/// # async_std::task::block_on(async {
/// #
/// use async_std::prelude::*;
/// use async_std::stream;
///
/// let mut s = stream::empty::<i32>();
///
/// assert_eq!(s.next().await, None);
/// #
/// # })
/// ```
pub fn empty<T>() -> Empty<T> {
    Empty {
        _marker: PhantomData,
    }
}

/// A stream that doesn't yield any items.
///
/// This stream is constructed by the [`empty`] function.
///
/// [`empty`]: fn.empty.html
#[derive(Debug)]
pub struct Empty<T> {
    _marker: PhantomData<T>,
}

impl<T> Stream for Empty<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(None)
    }
}

pin_project! {
    /// A stream that was created from iterator.
    ///
    /// This stream is created by the [`iter`] function.
    /// See it documentation for more.
    ///
    /// [`iter`]: fn.iter.html
    #[derive(Clone, Debug)]
    pub struct FromIter<I> {
        iter: I,
    }
}

/// Converts an iterator into a stream.
///
/// # Examples
///
/// ```
/// # async_std::task::block_on(async {
/// #
/// use async_std::prelude::*;
/// use async_std::stream;
///
/// let mut s = stream::iter(vec![0, 1, 2, 3]);
///
/// assert_eq!(s.next().await, Some(0));
/// assert_eq!(s.next().await, Some(1));
/// assert_eq!(s.next().await, Some(2));
/// assert_eq!(s.next().await, Some(3));
/// assert_eq!(s.next().await, None);
/// #
/// # })
/// ```
pub fn iter<I: IntoIterator>(iter: I) -> FromIter<I::IntoIter> {
    FromIter {
        iter: iter.into_iter(),
    }
}

impl<I: Iterator> Stream for FromIter<I> {
    type Item = I::Item;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.iter.next())
    }
}

/// Creates a stream that yields a single item.
///
/// # Examples
///
/// ```
/// # async_std::task::block_on(async {
/// #
/// use async_std::prelude::*;
/// use async_std::stream;
///
/// let mut s = stream::once(7);
///
/// assert_eq!(s.next().await, Some(7));
/// assert_eq!(s.next().await, None);
/// #
/// # })
/// ```
pub fn once<T>(t: T) -> Once<T> {
    Once { value: Some(t) }
}

pin_project! {
    /// A stream that yields a single item.
    ///
    /// This stream is created by the [`once`] function. See its
    /// documentation for more.
    ///
    /// [`once`]: fn.once.html
    #[derive(Clone, Debug)]
    pub struct Once<T> {
        value: Option<T>,
    }
}

impl<T> Stream for Once<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<T>> {
        Poll::Ready(self.project().value.take())
    }
}

/// A stream that never returns any items.
///
/// This stream is created by the [`pending`] function. See its
/// documentation for more.
///
/// [`pending`]: fn.pending.html
#[derive(Debug)]
pub struct Pending<T> {
    _marker: PhantomData<T>,
}

/// Creates a stream that never returns any items.
///
/// The returned stream will always return `Pending` when polled.
/// # Examples
///
/// ```
/// # async_std::task::block_on(async {
/// #
/// use std::time::Duration;
///
/// use async_std::prelude::*;
/// use async_std::stream;
///
/// let dur = Duration::from_millis(100);
/// let mut s = stream::pending::<()>().timeout(dur);
///
/// let item = s.next().await;
///
/// assert!(item.is_some());
/// assert!(item.unwrap().is_err());
///
/// #
/// # })
/// ```
pub fn pending<T>() -> Pending<T> {
    Pending {
        _marker: PhantomData,
    }
}

impl<T> Stream for Pending<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<T>> {
        Poll::Pending
    }
}

/// Creates a new stream wrapping a function returning `Poll<Option<T>>`.
///
/// Polling the returned stream calls the wrapped function.
///
/// # Examples
///
/// ```
/// use futures::stream::poll_fn;
/// use futures::task::Poll;
///
/// let mut counter = 1usize;
///
/// let read_stream = poll_fn(move |_| -> Poll<Option<String>> {
///     if counter == 0 { return Poll::Ready(None); }
///     counter -= 1;
///     Poll::Ready(Some("Hello, World!".to_owned()))
/// });
/// ```
pub fn poll_fn<T, F>(f: F) -> PollFn<F>
where
    F: FnMut(&mut Context<'_>) -> Poll<Option<T>>,
{
    PollFn { f }
}

/// Stream for the [`poll_fn`] function.
pub struct PollFn<F> {
    f: F,
}

impl<F> Unpin for PollFn<F> {}

impl<F> fmt::Debug for PollFn<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PollFn").finish()
    }
}

impl<T, F> Stream for PollFn<F>
where
    F: FnMut(&mut Context<'_>) -> Poll<Option<T>>,
{
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        (&mut self.f)(cx)
    }
}

/// Creates a stream that yields the same item repeatedly.
///
/// # Examples
///
/// ```
/// # async_std::task::block_on(async {
/// #
/// use async_std::prelude::*;
/// use async_std::stream;
///
/// let mut s = stream::repeat(7);
///
/// assert_eq!(s.next().await, Some(7));
/// assert_eq!(s.next().await, Some(7));
/// #
/// # })
/// ```
pub fn repeat<T>(item: T) -> Repeat<T>
where
    T: Clone,
{
    Repeat { item }
}

/// A stream that yields the same item repeatedly.
///
/// This stream is created by the [`repeat`] function. See its
/// documentation for more.
///
/// [`repeat`]: fn.repeat.html
#[derive(Clone, Debug)]
pub struct Repeat<T> {
    item: T,
}

impl<T: Clone> Stream for Repeat<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(Some(self.item.clone()))
    }
}

/// A stream that repeats elements of type `T` endlessly by applying a provided closure.
///
/// This stream is created by the [`repeat_with`] function. See its
/// documentation for more.
///
/// [`repeat_with`]: fn.repeat_with.html
#[derive(Clone, Debug)]
pub struct RepeatWith<F> {
    f: F,
}

impl<F> Unpin for RepeatWith<F> {}

/// Creates a new stream that repeats elements of type `A` endlessly by applying the provided closure.
///
/// # Examples
///
/// Basic usage:
///
/// ```
/// # async_std::task::block_on(async {
/// #
/// use async_std::prelude::*;
/// use async_std::stream;
///
/// let s = stream::repeat_with(|| 1);
///
/// pin_utils::pin_mut!(s);
///
/// assert_eq!(s.next().await, Some(1));
/// assert_eq!(s.next().await, Some(1));
/// assert_eq!(s.next().await, Some(1));
/// assert_eq!(s.next().await, Some(1));
/// # })
/// ```
///
/// Going finite:
///
/// ```
/// # async_std::task::block_on(async {
/// #
/// use async_std::prelude::*;
/// use async_std::stream;
///
/// let mut n = 1;
/// let s = stream::repeat_with(|| {
///     let item = n;
///     n *= 2;
///     item
/// })
/// .take(4);
///
/// pin_utils::pin_mut!(s);
///
/// assert_eq!(s.next().await, Some(1));
/// assert_eq!(s.next().await, Some(2));
/// assert_eq!(s.next().await, Some(4));
/// assert_eq!(s.next().await, Some(8));
/// assert_eq!(s.next().await, None);
/// # })
/// ```
pub fn repeat_with<T, F>(repeater: F) -> RepeatWith<F>
where
    F: FnMut() -> T,
{
    RepeatWith { f: repeater }
}

impl<T, F> Stream for RepeatWith<F>
where
    F: FnMut() -> T,
{
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let item = (&mut self.f)();
        Poll::Ready(Some(item))
    }
}

/// Creates a stream from a seed and a closure returning a future.
///
/// # Examples
///
/// ```
/// use futures_lite::*;
///
/// let s = stream::unfold(0, |n| async move {
///     if n < 5 {
///         n += 1;
///         Some(*n)
///     } else {
///         None
///     }
/// });
///
/// let result = s.collect::<Vec<i32>>().await;
/// assert_eq!(result, vec![1, 2, 3, 4, 5]);
/// ```
pub fn unfold<T, F, Fut, Item>(init: T, f: F) -> Unfold<T, F, Fut>
where
    F: FnMut(&mut T) -> Fut,
    Fut: Future<Output = Option<Item>>,
{
    Unfold {
        f,
        state: Some(init),
        fut: None,
    }
}

pin_project! {
    /// Stream for the [`unfold`] function.
    pub struct Unfold<T, F, Fut> {
        f: F,
        state: Option<T>,
        #[pin]
        fut: Option<Fut>,
    }
}

impl<T, F, Fut> fmt::Debug for Unfold<T, F, Fut>
where
    T: fmt::Debug,
    Fut: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Unfold")
            .field("state", &self.state)
            .field("fut", &self.fut)
            .finish()
    }
}

impl<T, F, Fut, Item> Stream for Unfold<T, F, Fut>
where
    F: FnMut(&mut T) -> Fut,
    Fut: Future<Output = Option<Item>>,
{
    type Item = Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        if let Some(state) = this.state.as_mut() {
            this.fut.set(Some((this.f)(state)));
        }

        let fut = this
            .fut
            .as_mut()
            .as_pin_mut()
            .expect("`Unfold` must not be polled after it returned `Poll::Ready(None)`");
        let step = ready!(fut.poll(cx));
        this.fut.set(None);

        if let Some(item) = step {
            Poll::Ready(Some(item))
        } else {
            Poll::Ready(None)
        }
    }
}

/// Creates a [`TryStream`] from a seed and a closure returning a `TryFuture`.
pub fn try_unfold<T, E, F, Fut, Item>(init: T, f: F) -> TryUnfold<T, F, Fut>
where
    F: FnMut(&mut T) -> Fut,
    Fut: Future<Output = Result<Option<Item>, E>>,
{
    TryUnfold {
        f,
        state: Some(init),
        fut: None,
    }
}

pin_project! {
    /// Stream for the [`try_unfold`] function.
    pub struct TryUnfold<T, F, Fut> {
        f: F,
        state: Option<T>,
        #[pin]
        fut: Option<Fut>,
    }
}

impl<T, F, Fut> fmt::Debug for TryUnfold<T, F, Fut>
where
    T: fmt::Debug,
    Fut: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TryUnfold")
            .field("state", &self.state)
            .field("fut", &self.fut)
            .finish()
    }
}

impl<T, E, F, Fut, Item> Stream for TryUnfold<T, F, Fut>
where
    F: FnMut(&mut T) -> Fut,
    Fut: Future<Output = Result<Option<Item>, E>>,
{
    type Item = Result<Item, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Item, E>>> {
        let mut this = self.project();

        if let Some(state) = this.state.as_mut() {
            this.fut.set(Some((this.f)(state)));
        }

        match this.fut.as_mut().as_pin_mut() {
            None => {
                // The future previously errored
                Poll::Ready(None)
            }
            Some(future) => {
                let step = ready!(future.poll(cx));
                this.fut.set(None);

                match step {
                    Ok(Some(item)) => Poll::Ready(Some(Ok(item))),
                    Ok(None) => Poll::Ready(None),
                    Err(e) => Poll::Ready(Some(Err(e))),
                }
            }
        }
    }
}

pub trait StreamExt: Stream {
    fn next(&mut self) -> NextFuture<'_, Self>
    where
        Self: Unpin,
    {
        NextFuture(self)
    }
}

impl<T: ?Sized> StreamExt for T where T: Stream {}

#[doc(hidden)]
pub struct NextFuture<'a, T: Unpin + ?Sized>(&'a mut T);

impl<St: ?Sized + Unpin> Unpin for NextFuture<'_, St> {}

impl<T: Stream + Unpin + ?Sized> Future for NextFuture<'_, T> {
    type Output = Option<T::Item>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut *self.0).poll_next(cx)
    }
}
