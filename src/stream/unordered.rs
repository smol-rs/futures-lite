//! The `UnorderedFutures` stream object.

extern crate alloc;

use alloc::boxed::Box;
use alloc::sync::Arc;
use core::iter::FromIterator;
use core::pin::Pin;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::{Context, Poll, Waker};

use crate::{Future, Stream};

use pin_project_lite::pin_project;
use waker_fn::waker_fn;

/// A list of futures that polls all of them concurrently.
///
/// There are many cases where a set of futures need to polled.
/// Polling them one after another is inefficient and can lead
/// to starvation. When a future is pushed into this stream,
/// it is polled concurrently with all the other futures.
///
/// The order of the output is not guaranteed.
///
/// # Example
///
/// ```rust
/// use futures_lite::future::ready;
/// use futures_lite::stream::{UnorderedFutures, StreamExt};
///
/// # spin_on::spin_on(async {
/// let mut stream = UnorderedFutures::new();
///
/// for _ in 0..3 {
///     stream.push(ready(1));
/// }
///
/// let v: Vec<_> = stream.collect().await;
/// assert_eq!(v, vec![1, 1, 1]);
/// # });
/// ```
#[derive(Debug, Default, Clone)]
#[must_use = "streams do nothing unless polled"]
pub struct UnorderedFutures<Fut> {
    /// The first future in our linked list to be polled.
    first: Link<Fut>,
    /// The current number of futures in the list.
    len: usize,
}

pin_project! {
    /// The state of a future in the linked list.
    #[derive(Debug, Clone)]
    struct Container<Fut> {
        #[pin]
        future: Fut,
        next: Link<Fut>,
        ready: Arc<AtomicBool>,
    }
}

/// Type alias to make the code more readable.
type Link<Fut> = Option<Pin<Box<Container<Fut>>>>;

impl<Fut: Future> UnorderedFutures<Fut> {
    /// Creates a new, empty `UnorderedFutures`.
    pub const fn new() -> UnorderedFutures<Fut> {
        UnorderedFutures {
            first: None,
            len: 0,
        }
    }

    /// Gets the number of futures in the list.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the list contains no futures.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Adds a future to the list.
    pub fn push(&mut self, future: Fut) {
        let mut container = Container::new(future);

        // Update the linked list.
        container.next = self.first.take();
        self.first = Some(Box::pin(container));
        self.len += 1;
    }

    /// The borrow checker doesn't accept this code in loop form. However,
    /// it seems to have no issues with this recursive form.
    fn poll_next_impl(
        cx: &mut Context<'_>,
        current_slot: &mut Link<Fut>,
        len: &mut usize,
    ) -> Poll<Option<Fut::Output>> {
        // Poll all futures in the list.
        if let Some(current) = current_slot.as_mut() {
            let current = current.as_mut().project();

            // If the future is ready to poll, begin polling it.
            //
            // Relaxed is appropriate to use here, since the stream is
            // only re-polled in a sane executor if the waker is woken,
            // and the value is set before the waker is woken.
            if current.ready.load(Ordering::Relaxed) {
                // Create the context to poll the future in.
                //
                // The context ensures that the future is marked as "ready"
                // when it is woken.
                current.ready.store(false, Ordering::Relaxed);
                let waker = container_waker(&current.ready, cx);
                let mut context = Context::from_waker(&waker);

                // Poll the future.
                if let Poll::Ready(value) = current.future.poll(&mut context) {
                    // The future has completed, so remove it from the list.
                    *current_slot = current.next.take();
                    *len -= 1;
                    return Poll::Ready(Some(value));
                }
            }

            // The future is not ready, so move on to the next one.
            Self::poll_next_impl(cx, current.next, len)
        } else {
            Poll::Pending
        }
    }
}

impl<Fut: Future> Stream for UnorderedFutures<Fut> {
    type Item = Fut::Output;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let Self {
            ref mut first,
            ref mut len,
        } = &mut *self;

        // If the list is empty, return `None`.
        if first.is_none() {
            return Poll::Ready(None);
        }

        Self::poll_next_impl(cx, first, len)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl<Fut: Future> Extend<Fut> for UnorderedFutures<Fut> {
    fn extend<T: IntoIterator<Item = Fut>>(&mut self, iter: T) {
        for future in iter {
            self.push(future);
        }
    }
}

impl<Fut: Future> FromIterator<Fut> for UnorderedFutures<Fut> {
    fn from_iter<T: IntoIterator<Item = Fut>>(iter: T) -> Self {
        let mut stream = UnorderedFutures::new();
        stream.extend(iter);
        stream
    }
}

impl<Fut> Container<Fut> {
    fn new(future: Fut) -> Self {
        Self {
            future,
            next: None,
            ready: Arc::new(AtomicBool::new(true)),
        }
    }
}

/// Create a waker for a given container's state and the global
/// context.
///
/// This would normally be a method on `Container`'s pin projection,
/// but there is no way to add associated methods to the projection.
fn container_waker(state: &Arc<AtomicBool>, cx: &mut Context<'_>) -> Waker {
    let state = state.clone();
    let waker = cx.waker().clone();

    waker_fn(move || {
        // Indicate that the future is ready to be polled, and wake the top-level waker.
        state.store(true, Ordering::SeqCst);
        waker.wake_by_ref();
    })
}

#[cfg(test)]
mod tests {
    use super::UnorderedFutures;
    use crate::stream::StreamExt;

    // Make sure the call is tail-optimized so we don't hit the stack limit.
    #[test]
    fn lots_of_futures() {
        let mut stream = UnorderedFutures::new();

        for _ in 0..10000 {
            stream.push(async { 1 });
        }

        let v: Vec<_> = spin_on::spin_on(stream.collect());
        assert_eq!(v.len(), 10000);
    }
}
