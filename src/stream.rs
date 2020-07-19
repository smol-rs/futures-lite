//! Combinators for the [`Stream`] trait.
//!
//! # Examples
//!
//! ```no_run
//! use futures_lite::*;
//!
//! # blocking::block_on(async {
//! let s = stream::iter(vec![1, 2, 3]);
//!
//! assert_eq!(s.next().await, Some(1));
//! assert_eq!(s.next().await, Some(2));
//! assert_eq!(s.next().await, Some(3));
//! assert_eq!(s.next().await, None);
//! # });
//! ```

// TODO: future() constructor that converts a future to a stream
// TODO: merge() constructor (randomized for fairness)

use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::mem;
use std::pin::Pin;
use std::task::{Context, Poll};

#[doc(no_inline)]
pub use futures_core::stream::Stream;
use pin_project_lite::pin_project;

use crate::ready;

/// Creates an empty stream.
///
/// # Examples
///
/// ```
/// use futures_lite::*;
///
/// # blocking::block_on(async {
/// let mut s = stream::empty::<i32>();
/// assert_eq!(s.next().await, None);
/// # })
/// ```
pub fn empty<T>() -> Empty<T> {
    Empty {
        _marker: PhantomData,
    }
}

/// Stream for the [`empty()`] function.
#[derive(Debug)]
pub struct Empty<T> {
    _marker: PhantomData<T>,
}

impl<T> Unpin for Empty<T> {}

impl<T> Stream for Empty<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(None)
    }
}

/// Creates a stream from an iterator.
///
/// # Examples
///
/// ```
/// use futures_lite::*;
///
/// # blocking::block_on(async {
/// let mut s = stream::iter(vec![1, 2]);
///
/// assert_eq!(s.next().await, Some(1));
/// assert_eq!(s.next().await, Some(2));
/// assert_eq!(s.next().await, None);
/// # })
/// ```
pub fn iter<I: IntoIterator>(iter: I) -> Iter<I::IntoIter> {
    Iter {
        iter: iter.into_iter(),
    }
}

/// Stream for the [`iter()`] function.
#[derive(Debug)]
pub struct Iter<I> {
    iter: I,
}

impl<I> Unpin for Iter<I> {}

impl<I: Iterator> Stream for Iter<I> {
    type Item = I::Item;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.iter.next())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

/// Creates a stream that yields a single item.
///
/// # Examples
///
/// ```
/// use futures_lite::*;
///
/// # blocking::block_on(async {
/// let mut s = stream::once(7);
///
/// assert_eq!(s.next().await, Some(7));
/// assert_eq!(s.next().await, None);
/// # })
/// ```
pub fn once<T>(t: T) -> Once<T> {
    Once { value: Some(t) }
}

pin_project! {
    /// Stream for the [`once()`] function.
    #[derive(Debug)]
    pub struct Once<T> {
        value: Option<T>,
    }
}

impl<T> Stream for Once<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<T>> {
        Poll::Ready(self.project().value.take())
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.value.is_some() {
            (1, Some(1))
        } else {
            (0, Some(0))
        }
    }
}

/// Creates a stream that is always pending.
///
/// # Examples
///
/// ```no_run
/// use futures_lite::*;
///
/// # blocking::block_on(async {
/// let mut s = stream::pending::<i32>();
/// s.next().await;
/// unreachable!();
/// # })
/// ```
pub fn pending<T>() -> Pending<T> {
    Pending {
        _marker: PhantomData,
    }
}

/// Stream for the [`pending()`] function.
#[derive(Debug)]
pub struct Pending<T> {
    _marker: PhantomData<T>,
}

impl<T> Unpin for Pending<T> {}

impl<T> Stream for Pending<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<T>> {
        Poll::Pending
    }
}

/// Creates a stream from a function returning [`Poll`].
///
/// # Examples
///
/// ```
/// use futures_lite::*;
/// use std::task::{Context, Poll};
///
/// # blocking::block_on(async {
/// fn f(_: &mut Context<'_>) -> Poll<Option<i32>> {
///     Poll::Ready(Some(7))
/// }
///
/// assert_eq!(stream::poll_fn(f).next().await, Some(7));
/// # })
/// ```
pub fn poll_fn<T, F>(f: F) -> PollFn<F>
where
    F: FnMut(&mut Context<'_>) -> Poll<Option<T>>,
{
    PollFn { f }
}

/// Stream for the [`poll_fn()`] function.
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

/// Creates an infinite stream that yields the same item repeatedly.
///
/// # Examples
///
/// ```
/// use futures_lite::*;
///
/// # blocking::block_on(async {
/// let mut s = stream::repeat(7);
///
/// assert_eq!(s.next().await, Some(7));
/// assert_eq!(s.next().await, Some(7));
/// # })
/// ```
pub fn repeat<T: Clone>(item: T) -> Repeat<T> {
    Repeat { item }
}

/// Stream for the [`repeat()`] function.
#[derive(Debug)]
pub struct Repeat<T> {
    item: T,
}

impl<T> Unpin for Repeat<T> {}

impl<T: Clone> Stream for Repeat<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(Some(self.item.clone()))
    }
}

/// Creates an infinite stream from a closure that generates items.
///
/// # Examples
///
/// ```
/// use futures_lite::*;
///
/// # blocking::block_on(async {
/// let mut s = stream::repeat(7);
///
/// assert_eq!(s.next().await, Some(7));
/// assert_eq!(s.next().await, Some(7));
/// # })
/// ```
pub fn repeat_with<T, F>(repeater: F) -> RepeatWith<F>
where
    F: FnMut() -> T,
{
    RepeatWith { f: repeater }
}

/// Stream for the [`repeat_with()`] function.
#[derive(Debug)]
pub struct RepeatWith<F> {
    f: F,
}

impl<F> Unpin for RepeatWith<F> {}

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

/// Creates a stream from a seed value and an async closure operating on it.
///
/// # Examples
///
/// ```
/// use futures_lite::*;
///
/// # blocking::block_on(async {
/// let s = stream::unfold(0, |mut n| async move {
///     if n < 2 {
///         let m = n + 1;
///         Some((n, m))
///     } else {
///         None
///     }
/// });
///
/// let v: Vec<i32> = s.collect().await;
/// assert_eq!(v, [0, 1]);
/// # })
/// ```
pub fn unfold<T, F, Fut, Item>(seed: T, f: F) -> Unfold<T, F, Fut>
where
    F: FnMut(T) -> Fut,
    Fut: Future<Output = Option<(Item, T)>>,
{
    Unfold {
        f,
        state: Some(seed),
        fut: None,
    }
}

pin_project! {
    /// Stream for the [`unfold()`] function.
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
    F: FnMut(T) -> Fut,
    Fut: Future<Output = Option<(Item, T)>>,
{
    type Item = Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        if let Some(state) = this.state.take() {
            this.fut.set(Some((this.f)(state)));
        }

        let step = ready!(this
            .fut
            .as_mut()
            .as_pin_mut()
            .expect("`Unfold` must not be polled after it returned `Poll::Ready(None)`")
            .poll(cx));
        this.fut.set(None);

        if let Some((item, next_state)) = step {
            *this.state = Some(next_state);
            Poll::Ready(Some(item))
        } else {
            Poll::Ready(None)
        }
    }
}

/// Creates a stream from a seed value and a fallible async closure operating on it.
///
/// # Examples
///
/// ```
/// use futures_lite::*;
///
/// # blocking::block_on(async {
/// let s = stream::try_unfold(0, |mut n| async move {
///     if n < 2 {
///         let m = n + 1;
///         Ok(Some((n, m)))
///     } else {
///         std::io::Result::Ok(None)
///     }
/// });
///
/// let v: Vec<i32> = s.try_collect().await?;
/// assert_eq!(v, [0, 1]);
/// # std::io::Result::Ok(()) });
/// ```
pub fn try_unfold<T, E, F, Fut, Item>(init: T, f: F) -> TryUnfold<T, F, Fut>
where
    F: FnMut(T) -> Fut,
    Fut: Future<Output = Result<Option<(Item, T)>, E>>,
{
    TryUnfold {
        f,
        state: Some(init),
        fut: None,
    }
}

pin_project! {
    /// Stream for the [`try_unfold()`] function.
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
    F: FnMut(T) -> Fut,
    Fut: Future<Output = Result<Option<(Item, T)>, E>>,
{
    type Item = Result<Item, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        if let Some(state) = this.state.take() {
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
                    Ok(Some((item, next_state))) => {
                        *this.state = Some(next_state);
                        Poll::Ready(Some(Ok(item)))
                    }
                    Ok(None) => Poll::Ready(None),
                    Err(e) => Poll::Ready(Some(Err(e))),
                }
            }
        }
    }
}

/// Extension trait for [`Stream`] with a variety of combinators.
pub trait StreamExt: Stream {
    /// Retrieves the next item in the stream.
    ///
    /// Returns [`None`] when iteration is finished. Stream implementations may choose to or not to
    /// resume iteration after that.
    ///
    /// # Examples
    ///
    /// ```
    /// use futures_lite::*;
    ///
    /// # blocking::block_on(async {
    /// let mut s = stream::iter(1..=3);
    ///
    /// assert_eq!(s.next().await, Some(1));
    /// assert_eq!(s.next().await, Some(2));
    /// assert_eq!(s.next().await, Some(3));
    /// assert_eq!(s.next().await, None);
    /// # });
    /// ```
    fn next(&mut self) -> NextFuture<'_, Self>
    where
        Self: Unpin,
    {
        NextFuture { stream: self }
    }

    /// Collects all items in the stream into a collection.
    ///
    /// # Examples
    ///
    /// ```
    /// use futures_lite::*;
    ///
    /// # blocking::block_on(async {
    /// let mut s = stream::iter(1..=3);
    ///
    /// let items: Vec<_> = s.collect().await;
    /// assert_eq!(items, [1, 2, 3]);
    /// # });
    /// ```
    fn collect<C: Default + Extend<Self::Item>>(self) -> CollectFuture<Self, C>
    where
        Self: Sized,
    {
        CollectFuture {
            stream: self,
            collection: Default::default(),
        }
    }

    /// Collects all items in the fallible stream into a collection.
    ///
    /// ```
    /// # blocking::block_on(async {
    /// use futures_lite::*;
    ///
    /// let s = stream::iter(vec![Ok(1), Err(2), Ok(3)]);
    /// let res: Result<Vec<i32>, i32> = s.try_collect().await;
    /// assert_eq!(res, Err(2));
    ///
    /// let s = stream::iter(vec![Ok(1), Ok(2), Ok(3)]);
    /// let res: Result<Vec<i32>, i32> = s.try_collect().await;
    /// assert_eq!(res, Ok(vec![1, 2, 3]));
    /// # })
    /// ```
    fn try_collect<T, C: Default + Extend<T>>(self) -> TryCollectFuture<Self, C>
    where
        Self: Sized,
        Self::Item: try_hack::Result<Ok = T>,
    {
        TryCollectFuture {
            stream: self,
            items: Default::default(),
        }
    }
}

impl<T: ?Sized> StreamExt for T where T: Stream {}

/// Future for the [`StreamExt::next()`] method.
pub struct NextFuture<'a, T: Unpin + ?Sized> {
    stream: &'a mut T,
}

impl<St: ?Sized + Unpin> Unpin for NextFuture<'_, St> {}

impl<T: Stream + Unpin + ?Sized> Future for NextFuture<'_, T> {
    type Output = Option<T::Item>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut *self.stream).poll_next(cx)
    }
}

pin_project! {
    /// Future for the [`StreamExt::collect()`] method.
    #[derive(Debug)]
    pub struct CollectFuture<St, C> {
        #[pin]
        stream: St,
        collection: C,
    }
}

impl<St, C> Future for CollectFuture<St, C>
where
    St: Stream,
    C: Default + Extend<St::Item>,
{
    type Output = C;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<C> {
        let mut this = self.as_mut().project();
        loop {
            match ready!(this.stream.as_mut().poll_next(cx)) {
                Some(e) => this.collection.extend(Some(e)),
                None => {
                    return Poll::Ready({
                        mem::replace(self.project().collection, Default::default())
                    })
                }
            }
        }
    }
}

pin_project! {
    /// Future for the [`StreamExt::try_collect()`] method.
    #[derive(Debug)]
    pub struct TryCollectFuture<St, C> {
        #[pin]
        stream: St,
        items: C,
    }
}

impl<T, E, St, C> Future for TryCollectFuture<St, C>
where
    St: Stream<Item = Result<T, E>>,
    C: Default + Extend<T>,
{
    type Output = Result<C, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        Poll::Ready(Ok(loop {
            match ready!(this.stream.as_mut().poll_next(cx)?) {
                Some(x) => this.items.extend(Some(x)),
                None => break mem::replace(this.items, Default::default()),
            }
        }))
    }
}

/// The `Try` trait is not stable yet, so we use this hack to constrain types to `Result<T, E>`.
mod try_hack {
    pub trait Result {
        type Ok;
        type Err;
    }

    impl<T, E> Result for std::result::Result<T, E> {
        type Ok = T;
        type Err = E;
    }
}
