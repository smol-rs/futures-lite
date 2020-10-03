//! A lightweight async prelude.
//!
//! This crate is a subset of [futures] that compiles an order of magnitude faster, fixes minor
//! warts in its API, fills in some obvious gaps, and removes almost all unsafe code from it.
//!
//! In short, this crate aims to be more enjoyable than [futures] but still fully compatible with
//! it.
//!
//! [futures]: https://docs.rs/futures
//!
//! # Examples
//!
#![cfg_attr(feature = "std", doc = "```no_run")]
#![cfg_attr(not(feature = "std"), doc = "```ignore")]
//! use futures_lite::*;
//!
//! fn main() {
//!     future::block_on(async {
//!         println!("Hello world!");
//!     })
//! }
//! ```

#![warn(missing_docs, missing_debug_implementations, rust_2018_idioms)]
#![cfg_attr(not(feature = "std"), no_std)]

#[doc(no_inline)]
pub use crate::future::{Future, FutureExt};

#[cfg(feature = "std")]
#[doc(no_inline)]
pub use crate::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, AsyncWrite,
    AsyncWriteExt,
};

#[doc(no_inline)]
pub use crate::stream::{Stream, StreamExt};

pub mod future;
pub mod stream;

#[cfg(feature = "std")]
pub mod io;

/// Unwraps `Poll<T>` or returns [`Pending`][`core::task::Poll::Pending`].
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
///     a: Pin<&mut impl Future<Output = i32>>,
///     b: Pin<&mut impl Future<Output = i32>>,
/// ) -> Poll<i32> {
///     let x = ready!(a.poll(cx));
///     let y = ready!(b.poll(cx));
///     Poll::Ready(x + y)
/// }
/// ```
#[macro_export]
macro_rules! ready {
    ($e:expr $(,)?) => {
        match $e {
            core::task::Poll::Ready(t) => t,
            core::task::Poll::Pending => return core::task::Poll::Pending,
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
/// # spin_on::spin_on(async {
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
                core::pin::Pin::new_unchecked(&mut $x)
            };
        )*
    }
}
