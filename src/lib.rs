//! A lightweight async prelude.
//!
//! This crate is a subset of [futures] that compiles an order of magnitude faster, fixes minor
//! warts in its API, fills in some obvious gaps, and removes all unsafe code from it.
//!
//! In short, this crate aims to be more enjoyable than [futures] but still fully compatible with
//! it.
//!
//! [futures]: https://docs.rs/futures
//!
//! # Examples
//!
//! ```no_run
//! use futures_lite::*;
//!
//! fn main() {
//!     future::block_on(async {
//!         println!("Hello world!");
//!     })
//! }
//! ```

#![warn(missing_docs, missing_debug_implementations, rust_2018_idioms)]

#[doc(no_inline)]
pub use std::future::Future;

#[doc(no_inline)]
pub use futures_core::Stream;
#[doc(no_inline)]
pub use futures_io::{AsyncBufRead, AsyncRead, AsyncSeek, AsyncWrite};

#[doc(no_inline)]
pub use crate::future::FutureExt;
#[doc(no_inline)]
pub use crate::io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
#[doc(no_inline)]
pub use crate::stream::StreamExt;

pub mod future;
pub mod io;
pub mod stream;

/// Unwraps `Poll<T>` or returns [`Pending`][`std::task::Poll::Pending`].
///
/// # Examples
///
/// ```
/// use futures_lite::*;
/// use std::pin::Pin;
/// use std::task::{Context, Poll};
///
/// // Polls two futures and sums their results.
/// fn poll_sum(
///     cx: &mut Context<'_>,
///     mut a: impl Future<Output = i32> + Unpin,
///     mut b: impl Future<Output = i32> + Unpin,
/// ) -> Poll<i32> {
///     let x = ready!(Pin::new(&mut a).poll(cx));
///     let y = ready!(Pin::new(&mut b).poll(cx));
///     Poll::Ready(x + y)
/// }
/// ```
#[macro_export]
macro_rules! ready {
    ($e:expr $(,)?) => {
        match $e {
            std::task::Poll::Ready(t) => t,
            std::task::Poll::Pending => return std::task::Poll::Pending,
        }
    };
}

/// Pins a variable of type `T` on the stack and rebinds it as `Pin<&mut T>`.
///
/// ```
/// use futures_lite::*;
/// use std::fmt::Debug;
/// use std::pin::Pin;
/// use std::time::Instant;
///
/// // Inspects each invocation of `Future::poll()`.
/// async fn inspect<T: Debug>(f: impl Future<Output = T>) -> T {
///     pin!(f);
///     future::poll_fn(|cx| dbg!(f.as_mut().poll(cx))).await
/// }
///
/// # future::block_on(async {
/// let f = async { 1 + 2 };
/// inspect(f).await;
/// # })
/// ```
#[macro_export]
macro_rules! pin {
    ($($x:ident),* $(,)?) => {
        $(
            let mut $x = $x;
            #[allow(unused_mut)]
            let mut $x = unsafe {
                std::pin::Pin::new_unchecked(&mut $x)
            };
        )*
    }
}
