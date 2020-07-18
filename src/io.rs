use std::cmp;
use std::fmt;
use std::future::Future;
#[doc(no_inline)]
pub use std::io::{Error, ErrorKind, Result};
use std::io::{IoSlice, IoSliceMut, Read, SeekFrom};
use std::mem;
use std::pin::Pin;
use std::str;
use std::task::{Context, Poll};

use futures_core::stream::Stream;
#[doc(no_inline)]
pub use futures_io::{AsyncBufRead, AsyncRead, AsyncSeek, AsyncWrite};
use pin_project_lite::pin_project;

use crate::ready;

const DEFAULT_BUF_SIZE: usize = 8 * 1024;

/// Copies the entire contents of a reader into a writer.
///
/// This function will continuously read data from `reader` and then
/// write it into `writer` in a streaming fashion until `reader`
/// returns EOF.
///
/// On success, the total number of bytes that were copied from
/// `reader` to `writer` is returned.
///
/// If you’re wanting to copy the contents of one file to another and you’re
/// working with filesystem paths, see the [`fs::copy`] function.
///
/// This function is an async version of [`std::io::copy`].
///
/// [`std::io::copy`]: https://doc.rust-lang.org/std/io/fn.copy.html
/// [`fs::copy`]: ../fs/fn.copy.html
///
/// # Errors
///
/// This function will return an error immediately if any call to `read` or
/// `write` returns an error. All instances of `ErrorKind::Interrupted` are
/// handled by this function and the underlying operation is retried.
///
/// # Examples
///
/// ```
/// # fn main() -> std::io::Result<()> { async_std::task::block_on(async {
/// #
/// use async_std::io;
///
/// let mut reader: &[u8] = b"hello";
/// let mut writer = io::stdout();
///
/// io::copy(&mut reader, &mut writer).await?;
/// #
/// # Ok(()) }) }
/// ```
pub async fn copy<R, W>(reader: R, writer: W) -> Result<u64>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    pin_project! {
        struct CopyFuture<R, W> {
            #[pin]
            reader: R,
            #[pin]
            writer: W,
            amt: u64,
        }
    }

    impl<R, W> Future for CopyFuture<R, W>
    where
        R: AsyncBufRead,
        W: AsyncWrite + Unpin,
    {
        type Output = Result<u64>;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let mut this = self.project();
            loop {
                let buffer = ready!(this.reader.as_mut().poll_fill_buf(cx))?;
                if buffer.is_empty() {
                    ready!(this.writer.as_mut().poll_flush(cx))?;
                    return Poll::Ready(Ok(*this.amt));
                }

                let i = ready!(this.writer.as_mut().poll_write(cx, buffer))?;
                if i == 0 {
                    return Poll::Ready(Err(ErrorKind::WriteZero.into()));
                }
                *this.amt += i as u64;
                this.reader.as_mut().consume(i);
            }
        }
    }

    let future = CopyFuture {
        reader: BufReader::new(reader),
        writer,
        amt: 0,
    };
    future.await
}

pin_project! {
    /// Adds buffering to any reader.
    ///
    /// It can be excessively inefficient to work directly with a [`Read`] instance. A `BufReader`
    /// performs large, infrequent reads on the underlying [`Read`] and maintains an in-memory buffer
    /// of the incoming byte stream.
    ///
    /// `BufReader` can improve the speed of programs that make *small* and *repeated* read calls to
    /// the same file or network socket. It does not help when reading very large amounts at once, or
    /// reading just one or a few times. It also provides no advantage when reading from a source that
    /// is already in memory, like a `Vec<u8>`.
    ///
    /// When the `BufReader` is dropped, the contents of its buffer will be discarded. Creating
    /// multiple instances of a `BufReader` on the same stream can cause data loss.
    ///
    /// This type is an async version of [`std::io::BufReader`].
    ///
    /// [`Read`]: trait.Read.html
    /// [`std::io::BufReader`]: https://doc.rust-lang.org/std/io/struct.BufReader.html
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> std::io::Result<()> { async_std::task::block_on(async {
    /// #
    /// use async_std::fs::File;
    /// use async_std::io::BufReader;
    /// use async_std::prelude::*;
    ///
    /// let mut file = BufReader::new(File::open("a.txt").await?);
    ///
    /// let mut line = String::new();
    /// file.read_line(&mut line).await?;
    /// #
    /// # Ok(()) }) }
    /// ```
    pub struct BufReader<R> {
        #[pin]
        inner: R,
        buf: Box<[u8]>,
        pos: usize,
        cap: usize,
    }
}

impl<R: AsyncRead> BufReader<R> {
    /// Creates a buffered reader with default buffer capacity.
    ///
    /// The default capacity is currently 8 KB, but may change in the future.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> std::io::Result<()> { async_std::task::block_on(async {
    /// #
    /// use async_std::fs::File;
    /// use async_std::io::BufReader;
    ///
    /// let f = BufReader::new(File::open("a.txt").await?);
    /// #
    /// # Ok(()) }) }
    /// ```
    pub fn new(inner: R) -> BufReader<R> {
        BufReader::with_capacity(DEFAULT_BUF_SIZE, inner)
    }

    /// Creates a new buffered reader with the specified capacity.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> std::io::Result<()> { async_std::task::block_on(async {
    /// #
    /// use async_std::fs::File;
    /// use async_std::io::BufReader;
    ///
    /// let f = BufReader::with_capacity(1024, File::open("a.txt").await?);
    /// #
    /// # Ok(()) }) }
    /// ```
    pub fn with_capacity(capacity: usize, inner: R) -> BufReader<R> {
        BufReader {
            inner,
            buf: vec![0; capacity].into_boxed_slice(),
            pos: 0,
            cap: 0,
        }
    }
}

impl<R> BufReader<R> {
    /// Gets a reference to the underlying reader.
    ///
    /// It is inadvisable to directly read from the underlying reader.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> std::io::Result<()> { async_std::task::block_on(async {
    /// #
    /// use async_std::fs::File;
    /// use async_std::io::BufReader;
    ///
    /// let f = BufReader::new(File::open("a.txt").await?);
    /// let inner = f.get_ref();
    /// #
    /// # Ok(()) }) }
    /// ```
    pub fn get_ref(&self) -> &R {
        &self.inner
    }

    /// Gets a mutable reference to the underlying reader.
    ///
    /// It is inadvisable to directly read from the underlying reader.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> std::io::Result<()> { async_std::task::block_on(async {
    /// #
    /// use async_std::fs::File;
    /// use async_std::io::BufReader;
    ///
    /// let mut file = BufReader::new(File::open("a.txt").await?);
    /// let inner = file.get_mut();
    /// #
    /// # Ok(()) }) }
    /// ```
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.inner
    }

    /// Gets a pinned mutable reference to the underlying reader.
    ///
    /// It is inadvisable to directly read from the underlying reader.
    fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut R> {
        self.project().inner
    }

    /// Returns a reference to the internal buffer.
    ///
    /// This function will not attempt to fill the buffer if it is empty.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> std::io::Result<()> { async_std::task::block_on(async {
    /// #
    /// use async_std::fs::File;
    /// use async_std::io::BufReader;
    ///
    /// let f = BufReader::new(File::open("a.txt").await?);
    /// let buffer = f.buffer();
    /// #
    /// # Ok(()) }) }
    /// ```
    pub fn buffer(&self) -> &[u8] {
        &self.buf[self.pos..self.cap]
    }

    /// Unwraps the buffered reader, returning the underlying reader.
    ///
    /// Note that any leftover data in the internal buffer is lost.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> std::io::Result<()> { async_std::task::block_on(async {
    /// #
    /// use async_std::fs::File;
    /// use async_std::io::BufReader;
    ///
    /// let f = BufReader::new(File::open("a.txt").await?);
    /// let inner = f.into_inner();
    /// #
    /// # Ok(()) }) }
    /// ```
    pub fn into_inner(self) -> R {
        self.inner
    }

    /// Invalidates all data in the internal buffer.
    #[inline]
    fn discard_buffer(self: Pin<&mut Self>) {
        let this = self.project();
        *this.pos = 0;
        *this.cap = 0;
    }
}

impl<R: AsyncRead> AsyncRead for BufReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize>> {
        // If we don't have any buffered data and we're doing a massive read
        // (larger than our internal buffer), bypass our internal buffer
        // entirely.
        if self.pos == self.cap && buf.len() >= self.buf.len() {
            let res = ready!(self.as_mut().get_pin_mut().poll_read(cx, buf));
            self.discard_buffer();
            return Poll::Ready(res);
        }
        let mut rem = ready!(self.as_mut().poll_fill_buf(cx))?;
        let nread = Read::read(&mut rem, buf)?;
        self.consume(nread);
        Poll::Ready(Ok(nread))
    }

    fn poll_read_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
    ) -> Poll<Result<usize>> {
        let total_len = bufs.iter().map(|b| b.len()).sum::<usize>();
        if self.pos == self.cap && total_len >= self.buf.len() {
            let res = ready!(self.as_mut().get_pin_mut().poll_read_vectored(cx, bufs));
            self.discard_buffer();
            return Poll::Ready(res);
        }
        let mut rem = ready!(self.as_mut().poll_fill_buf(cx))?;
        let nread = Read::read_vectored(&mut rem, bufs)?;
        self.consume(nread);
        Poll::Ready(Ok(nread))
    }
}

impl<R: AsyncRead> AsyncBufRead for BufReader<R> {
    fn poll_fill_buf<'a>(self: Pin<&'a mut Self>, cx: &mut Context<'_>) -> Poll<Result<&'a [u8]>> {
        let mut this = self.project();

        // If we've reached the end of our internal buffer then we need to fetch
        // some more data from the underlying reader.
        // Branch using `>=` instead of the more correct `==`
        // to tell the compiler that the pos..cap slice is always valid.
        if *this.pos >= *this.cap {
            debug_assert!(*this.pos == *this.cap);
            *this.cap = ready!(this.inner.as_mut().poll_read(cx, this.buf))?;
            *this.pos = 0;
        }
        Poll::Ready(Ok(&this.buf[*this.pos..*this.cap]))
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        let this = self.project();
        *this.pos = cmp::min(*this.pos + amt, *this.cap);
    }
}

impl<R: AsyncRead + fmt::Debug> fmt::Debug for BufReader<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BufReader")
            .field("reader", &self.inner)
            .field(
                "buffer",
                &format_args!("{}/{}", self.cap - self.pos, self.buf.len()),
            )
            .finish()
    }
}

impl<R: AsyncSeek> AsyncSeek for BufReader<R> {
    /// Seeks to an offset, in bytes, in the underlying reader.
    ///
    /// The position used for seeking with `SeekFrom::Current(_)` is the position the underlying
    /// reader would be at if the `BufReader` had no internal buffer.
    ///
    /// Seeking always discards the internal buffer, even if the seek position would otherwise fall
    /// within it. This guarantees that calling `.into_inner()` immediately after a seek yields the
    /// underlying reader at the same position.
    ///
    /// See [`Seek`] for more details.
    ///
    /// Note: In the edge case where you're seeking with `SeekFrom::Current(n)` where `n` minus the
    /// internal buffer length overflows an `i64`, two seeks will be performed instead of one. If
    /// the second seek returns `Err`, the underlying reader will be left at the same position it
    /// would have if you called `seek` with `SeekFrom::Current(0)`.
    ///
    /// [`Seek`]: trait.Seek.html
    fn poll_seek(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        pos: SeekFrom,
    ) -> Poll<Result<u64>> {
        let result: u64;
        if let SeekFrom::Current(n) = pos {
            let remainder = (self.cap - self.pos) as i64;
            // it should be safe to assume that remainder fits within an i64 as the alternative
            // means we managed to allocate 8 exbibytes and that's absurd.
            // But it's not out of the realm of possibility for some weird underlying reader to
            // support seeking by i64::min_value() so we need to handle underflow when subtracting
            // remainder.
            if let Some(offset) = n.checked_sub(remainder) {
                result = ready!(self
                    .as_mut()
                    .get_pin_mut()
                    .poll_seek(cx, SeekFrom::Current(offset)))?;
            } else {
                // seek backwards by our remainder, and then by the offset
                ready!(self
                    .as_mut()
                    .get_pin_mut()
                    .poll_seek(cx, SeekFrom::Current(-remainder)))?;
                self.as_mut().discard_buffer();
                result = ready!(self
                    .as_mut()
                    .get_pin_mut()
                    .poll_seek(cx, SeekFrom::Current(n)))?;
            }
        } else {
            // Seeking with Start/End doesn't care about our buffer length.
            result = ready!(self.as_mut().get_pin_mut().poll_seek(cx, pos))?;
        }
        self.discard_buffer();
        Poll::Ready(Ok(result))
    }
}

pin_project! {
    /// Wraps a writer and buffers its output.
    ///
    /// It can be excessively inefficient to work directly with something that
    /// implements [`Write`]. For example, every call to
    /// [`write`][`TcpStream::write`] on [`TcpStream`] results in a system call. A
    /// `BufWriter` keeps an in-memory buffer of data and writes it to an underlying
    /// writer in large, infrequent batches.
    ///
    /// `BufWriter` can improve the speed of programs that make *small* and
    /// *repeated* write calls to the same file or network socket. It does not
    /// help when writing very large amounts at once, or writing just one or a few
    /// times. It also provides no advantage when writing to a destination that is
    /// in memory, like a `Vec<u8>`.
    ///
    /// Unlike the `BufWriter` type in `std`, this type does not write out the
    /// contents of its buffer when it is dropped. Therefore, it is absolutely
    /// critical that users explicitly flush the buffer before dropping a
    /// `BufWriter`.
    ///
    /// This type is an async version of [`std::io::BufWriter`].
    ///
    /// [`std::io::BufWriter`]: https://doc.rust-lang.org/std/io/struct.BufWriter.html
    ///
    /// # Examples
    ///
    /// Let's write the numbers one through ten to a [`TcpStream`]:
    ///
    /// ```no_run
    /// # fn main() -> std::io::Result<()> { async_std::task::block_on(async {
    /// use async_std::net::TcpStream;
    /// use async_std::prelude::*;
    ///
    /// let mut stream = TcpStream::connect("127.0.0.1:34254").await?;
    ///
    /// for i in 0..10 {
    ///     let arr = [i+1];
    ///     stream.write(&arr).await?;
    /// }
    /// #
    /// # Ok(()) }) }
    /// ```
    ///
    /// Because we're not buffering, we write each one in turn, incurring the
    /// overhead of a system call per byte written. We can fix this with a
    /// `BufWriter`:
    ///
    /// ```no_run
    /// # fn main() -> std::io::Result<()> { async_std::task::block_on(async {
    /// use async_std::io::BufWriter;
    /// use async_std::net::TcpStream;
    /// use async_std::prelude::*;
    ///
    /// let mut stream = BufWriter::new(TcpStream::connect("127.0.0.1:34254").await?);
    ///
    /// for i in 0..10 {
    ///     let arr = [i+1];
    ///     stream.write(&arr).await?;
    /// };
    ///
    /// stream.flush().await?;
    /// #
    /// # Ok(()) }) }
    /// ```
    ///
    /// By wrapping the stream with a `BufWriter`, these ten writes are all grouped
    /// together by the buffer, and will all be written out in one system call when
    /// the `stream` is dropped.
    ///
    /// [`Write`]: trait.Write.html
    /// [`TcpStream::write`]: ../net/struct.TcpStream.html#method.write
    /// [`TcpStream`]: ../net/struct.TcpStream.html
    /// [`flush`]: trait.Write.html#tymethod.flush
    pub struct BufWriter<W> {
        #[pin]
        inner: W,
        buf: Vec<u8>,
        written: usize,
    }
}

/// An error returned by `into_inner` which combines an error that
/// happened while writing out the buffer, and the buffered writer object
/// which may be used to recover from the condition.
///
/// # Examples
///
/// ```no_run
/// # fn main() -> std::io::Result<()> { async_std::task::block_on(async {
/// use async_std::io::BufWriter;
/// use async_std::net::TcpStream;
///
/// let buf_writer = BufWriter::new(TcpStream::connect("127.0.0.1:34251").await?);
///
/// // unwrap the TcpStream and flush the buffer
/// let stream = match buf_writer.into_inner().await {
///     Ok(s) => s,
///     Err(e) => {
///         // Here, e is an IntoInnerError
///         panic!("An error occurred");
///     }
/// };
/// #
/// # Ok(()) }) }
///```
#[derive(Debug)]
pub struct IntoInnerError<W>(W, crate::io::Error);

impl<W: AsyncWrite> BufWriter<W> {
    /// Creates a new `BufWriter` with a default buffer capacity. The default is currently 8 KB,
    /// but may change in the future.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # #![allow(unused_mut)]
    /// # fn main() -> std::io::Result<()> { async_std::task::block_on(async {
    /// use async_std::io::BufWriter;
    /// use async_std::net::TcpStream;
    ///
    /// let mut buffer = BufWriter::new(TcpStream::connect("127.0.0.1:34254").await?);
    /// #
    /// # Ok(()) }) }
    /// ```
    pub fn new(inner: W) -> BufWriter<W> {
        BufWriter::with_capacity(DEFAULT_BUF_SIZE, inner)
    }

    /// Creates a new `BufWriter` with the specified buffer capacity.
    ///
    /// # Examples
    ///
    /// Creating a buffer with a buffer of a hundred bytes.
    ///
    /// ```no_run
    /// # #![allow(unused_mut)]
    /// # fn main() -> std::io::Result<()> { async_std::task::block_on(async {
    /// use async_std::io::BufWriter;
    /// use async_std::net::TcpStream;
    ///
    /// let stream = TcpStream::connect("127.0.0.1:34254").await?;
    /// let mut buffer = BufWriter::with_capacity(100, stream);
    /// #
    /// # Ok(()) }) }
    /// ```
    pub fn with_capacity(capacity: usize, inner: W) -> BufWriter<W> {
        BufWriter {
            inner,
            buf: Vec::with_capacity(capacity),
            written: 0,
        }
    }

    /// Gets a reference to the underlying writer.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # #![allow(unused_mut)]
    /// # fn main() -> std::io::Result<()> { async_std::task::block_on(async {
    /// use async_std::io::BufWriter;
    /// use async_std::net::TcpStream;
    ///
    /// let mut buffer = BufWriter::new(TcpStream::connect("127.0.0.1:34254").await?);
    ///
    /// // We can use reference just like buffer
    /// let reference = buffer.get_ref();
    /// #
    /// # Ok(()) }) }
    /// ```
    pub fn get_ref(&self) -> &W {
        &self.inner
    }

    /// Gets a mutable reference to the underlying writer.
    ///
    /// It is inadvisable to directly write to the underlying writer.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> std::io::Result<()> { async_std::task::block_on(async {
    /// use async_std::io::BufWriter;
    /// use async_std::net::TcpStream;
    ///
    /// let mut buffer = BufWriter::new(TcpStream::connect("127.0.0.1:34254").await?);
    ///
    /// // We can use reference just like buffer
    /// let reference = buffer.get_mut();
    /// #
    /// # Ok(()) }) }
    /// ```
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.inner
    }

    /// Gets a pinned mutable reference to the underlying writer.
    ///
    /// It is inadvisable to directly write to the underlying writer.
    fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut W> {
        self.project().inner
    }

    /// Consumes BufWriter, returning the underlying writer
    ///
    /// This method will not write leftover data, it will be lost.
    /// For method that will attempt to write before returning the writer see [`poll_into_inner`]
    ///
    /// [`poll_into_inner`]: #method.poll_into_inner
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> std::io::Result<()> { async_std::task::block_on(async {
    /// use async_std::io::BufWriter;
    /// use async_std::net::TcpStream;
    ///
    /// let buf_writer = BufWriter::new(TcpStream::connect("127.0.0.1:34251").await?);
    ///
    /// // unwrap the TcpStream and flush the buffer
    /// let stream = buf_writer.into_inner().await.unwrap();
    /// #
    /// # Ok(()) }) }
    /// ```
    pub async fn into_inner(self) -> std::result::Result<W, IntoInnerError<BufWriter<W>>>
    where
        Self: Unpin,
    {
        let mut this = self;
        match this.flush().await {
            Err(e) => Err(IntoInnerError(this, e)),
            Ok(()) => Ok(this.inner),
        }
    }

    /// Returns a reference to the internally buffered data.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> std::io::Result<()> { async_std::task::block_on(async {
    /// use async_std::io::BufWriter;
    /// use async_std::net::TcpStream;
    ///
    /// let buf_writer = BufWriter::new(TcpStream::connect("127.0.0.1:34251").await?);
    ///
    /// // See how many bytes are currently buffered
    /// let bytes_buffered = buf_writer.buffer().len();
    /// #
    /// # Ok(()) }) }
    /// ```
    pub fn buffer(&self) -> &[u8] {
        &self.buf
    }

    /// Poll buffer flushing until completion
    ///
    /// This is used in types that wrap around BufWrite, one such example: [`LineWriter`]
    ///
    /// [`LineWriter`]: struct.LineWriter.html
    fn poll_flush_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        let mut this = self.project();
        let len = this.buf.len();
        let mut ret = Ok(());
        while *this.written < len {
            match this
                .inner
                .as_mut()
                .poll_write(cx, &this.buf[*this.written..])
            {
                Poll::Ready(Ok(0)) => {
                    ret = Err(Error::new(
                        ErrorKind::WriteZero,
                        "Failed to write buffered data",
                    ));
                    break;
                }
                Poll::Ready(Ok(n)) => *this.written += n,
                Poll::Ready(Err(ref e)) if e.kind() == ErrorKind::Interrupted => {}
                Poll::Ready(Err(e)) => {
                    ret = Err(e);
                    break;
                }
                Poll::Pending => return Poll::Pending,
            }
        }
        if *this.written > 0 {
            this.buf.drain(..*this.written);
        }
        *this.written = 0;
        Poll::Ready(ret)
    }
}

impl<W: AsyncWrite> AsyncWrite for BufWriter<W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize>> {
        if self.buf.len() + buf.len() > self.buf.capacity() {
            ready!(self.as_mut().poll_flush_buf(cx))?;
        }
        if buf.len() >= self.buf.capacity() {
            self.get_pin_mut().poll_write(cx, buf)
        } else {
            Pin::new(&mut *self.project().buf).poll_write(cx, buf)
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        ready!(self.as_mut().poll_flush_buf(cx))?;
        self.get_pin_mut().poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        ready!(self.as_mut().poll_flush_buf(cx))?;
        self.get_pin_mut().poll_close(cx)
    }
}

impl<W: AsyncWrite + fmt::Debug> fmt::Debug for BufWriter<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BufWriter")
            .field("writer", &self.inner)
            .field("buf", &self.buf)
            .finish()
    }
}

impl<W: AsyncWrite + AsyncSeek> AsyncSeek for BufWriter<W> {
    /// Seek to the offset, in bytes, in the underlying writer.
    ///
    /// Seeking always writes out the internal buffer before seeking.
    fn poll_seek(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        pos: SeekFrom,
    ) -> Poll<Result<u64>> {
        ready!(self.as_mut().poll_flush_buf(cx))?;
        self.get_pin_mut().poll_seek(cx, pos)
    }
}

/// Creates a reader that contains no data.
///
/// # Examples
///
/// ```rust
/// # fn main() -> std::io::Result<()> { async_std::task::block_on(async {
/// #
/// use async_std::io;
/// use async_std::prelude::*;
///
/// let mut buf = Vec::new();
/// let mut reader = io::empty();
/// reader.read_to_end(&mut buf).await?;
///
/// assert!(buf.is_empty());
/// #
/// # Ok(()) }) }
/// ```
pub fn empty() -> Empty {
    Empty { _private: () }
}

/// A reader that contains no data.
///
/// This reader is created by the [`empty`] function. See its
/// documentation for more.
///
/// [`empty`]: fn.empty.html
pub struct Empty {
    _private: (),
}

impl fmt::Debug for Empty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad("Empty { .. }")
    }
}

impl AsyncRead for Empty {
    #[inline]
    fn poll_read(self: Pin<&mut Self>, _: &mut Context<'_>, _: &mut [u8]) -> Poll<Result<usize>> {
        Poll::Ready(Ok(0))
    }
}

impl AsyncBufRead for Empty {
    #[inline]
    fn poll_fill_buf<'a>(self: Pin<&'a mut Self>, _: &mut Context<'_>) -> Poll<Result<&'a [u8]>> {
        Poll::Ready(Ok(&[]))
    }

    #[inline]
    fn consume(self: Pin<&mut Self>, _: usize) {}
}

/// Creates an instance of a reader that infinitely repeats one byte.
///
/// All reads from this reader will succeed by filling the specified buffer with the given byte.
///
/// ## Examples
///
/// ```rust
/// # fn main() -> std::io::Result<()> { async_std::task::block_on(async {
/// #
/// use async_std::io;
/// use async_std::prelude::*;
///
/// let mut buffer = [0; 3];
/// io::repeat(0b101).read_exact(&mut buffer).await?;
///
/// assert_eq!(buffer, [0b101, 0b101, 0b101]);
/// #
/// # Ok(()) }) }
/// ```
pub fn repeat(byte: u8) -> Repeat {
    Repeat { byte }
}

/// A reader which yields one byte over and over and over and over and over and...
///
/// This reader is created by the [`repeat`] function. See its
/// documentation for more.
///
/// [`repeat`]: fn.repeat.html
#[derive(Debug)]
pub struct Repeat {
    byte: u8,
}

impl AsyncRead for Repeat {
    #[inline]
    fn poll_read(self: Pin<&mut Self>, _: &mut Context<'_>, buf: &mut [u8]) -> Poll<Result<usize>> {
        for b in &mut *buf {
            *b = self.byte;
        }
        Poll::Ready(Ok(buf.len()))
    }
}

/// Creates a writer that consumes and drops all data.
///
/// # Examples
///
/// ```rust
/// # fn main() -> std::io::Result<()> { async_std::task::block_on(async {
/// #
/// use async_std::io;
/// use async_std::prelude::*;
///
/// let mut writer = io::sink();
/// writer.write(b"hello world").await?;
/// #
/// # Ok(()) }) }
/// ```
pub fn sink() -> Sink {
    Sink { _private: () }
}

/// A writer that consumes and drops all data.
///
/// This writer is constructed by the [`sink`] function. See its documentation
/// for more.
///
/// [`sink`]: fn.sink.html
#[derive(Debug)]
pub struct Sink {
    _private: (),
}

impl AsyncWrite for Sink {
    #[inline]
    fn poll_write(self: Pin<&mut Self>, _: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize>> {
        Poll::Ready(Ok(buf.len()))
    }

    #[inline]
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }

    #[inline]
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }
}

pub trait AsyncBufReadExt: AsyncBufRead {
    fn read_until<'a>(&'a mut self, byte: u8, buf: &'a mut Vec<u8>) -> ReadUntilFuture<'_, Self>
    where
        Self: Unpin,
    {
        ReadUntilFuture {
            reader: self,
            byte,
            buf,
            read: 0,
        }
    }

    fn read_line<'a>(&'a mut self, buf: &'a mut String) -> ReadLineFuture<'_, Self>
    where
        Self: Unpin,
    {
        ReadLineFuture {
            reader: self,
            bytes: unsafe { mem::replace(buf.as_mut_vec(), Vec::new()) },
            buf,
            read: 0,
        }
    }

    fn lines(self) -> Lines<Self>
    where
        Self: Unpin + Sized,
    {
        Lines {
            reader: self,
            buf: String::new(),
            bytes: Vec::new(),
            read: 0,
        }
    }

    fn split(self, byte: u8) -> Split<Self>
    where
        Self: Sized,
    {
        Split {
            reader: self,
            buf: Vec::new(),
            delim: byte,
            read: 0,
        }
    }
}

impl<R: AsyncBufRead + ?Sized> AsyncBufReadExt for R {}

pub struct ReadUntilFuture<'a, T: Unpin + ?Sized> {
    reader: &'a mut T,
    byte: u8,
    buf: &'a mut Vec<u8>,
    read: usize,
}

impl<T: AsyncBufRead + Unpin + ?Sized> Future for ReadUntilFuture<'_, T> {
    type Output = Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Self {
            reader,
            byte,
            buf,
            read,
        } = &mut *self;
        read_until_internal(Pin::new(reader), cx, *byte, buf, read)
    }
}

fn read_until_internal<R: AsyncBufReadExt + ?Sized>(
    mut reader: Pin<&mut R>,
    cx: &mut Context<'_>,
    byte: u8,
    buf: &mut Vec<u8>,
    read: &mut usize,
) -> Poll<Result<usize>> {
    loop {
        let (done, used) = {
            let available = ready!(reader.as_mut().poll_fill_buf(cx))?;
            if let Some(i) = memchr::memchr(byte, available) {
                buf.extend_from_slice(&available[..=i]);
                (true, i + 1)
            } else {
                buf.extend_from_slice(available);
                (false, available.len())
            }
        };
        reader.as_mut().consume(used);
        *read += used;
        if done || used == 0 {
            return Poll::Ready(Ok(mem::replace(read, 0)));
        }
    }
}

pub struct ReadLineFuture<'a, T: Unpin + ?Sized> {
    reader: &'a mut T,
    buf: &'a mut String,
    bytes: Vec<u8>,
    read: usize,
}

impl<T: AsyncBufRead + Unpin + ?Sized> Future for ReadLineFuture<'_, T> {
    type Output = Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Self {
            reader,
            buf,
            bytes,
            read,
        } = &mut *self;
        let reader = Pin::new(reader);

        let ret = ready!(read_until_internal(reader, cx, b'\n', bytes, read));
        if str::from_utf8(&bytes).is_err() {
            Poll::Ready(ret.and_then(|_| {
                Err(Error::new(
                    ErrorKind::InvalidData,
                    "stream did not contain valid UTF-8",
                ))
            }))
        } else {
            #[allow(clippy::debug_assert_with_mut_call)]
            {
                debug_assert!(buf.is_empty());
                debug_assert_eq!(*read, 0);
            }

            // Safety: `bytes` is a valid UTF-8 because `str::from_utf8` returned `Ok`.
            mem::swap(unsafe { buf.as_mut_vec() }, bytes);
            Poll::Ready(ret)
        }
    }
}

pin_project! {
    /// A stream of lines in a byte stream.
    ///
    /// This stream is created by the [`lines`] method on types that implement [`BufRead`].
    ///
    /// This type is an async version of [`std::io::Lines`].
    ///
    /// [`lines`]: trait.BufRead.html#method.lines
    /// [`BufRead`]: trait.BufRead.html
    /// [`std::io::Lines`]: https://doc.rust-lang.org/std/io/struct.Lines.html
    #[derive(Debug)]
    pub struct Lines<R> {
        #[pin]
        reader: R,
        buf: String,
        bytes: Vec<u8>,
        read: usize,
    }
}

impl<R: AsyncBufRead> Stream for Lines<R> {
    type Item = Result<String>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        let n = ready!(read_line_internal(
            this.reader,
            cx,
            this.buf,
            this.bytes,
            this.read
        ))?;
        if n == 0 && this.buf.is_empty() {
            return Poll::Ready(None);
        }
        if this.buf.ends_with('\n') {
            this.buf.pop();
            if this.buf.ends_with('\r') {
                this.buf.pop();
            }
        }
        Poll::Ready(Some(Ok(mem::replace(this.buf, String::new()))))
    }
}

fn read_line_internal<R: AsyncBufRead + ?Sized>(
    reader: Pin<&mut R>,
    cx: &mut Context<'_>,
    buf: &mut String,
    bytes: &mut Vec<u8>,
    read: &mut usize,
) -> Poll<Result<usize>> {
    let ret = ready!(read_until_internal(reader, cx, b'\n', bytes, read));
    if str::from_utf8(&bytes).is_err() {
        Poll::Ready(ret.and_then(|_| {
            Err(Error::new(
                ErrorKind::InvalidData,
                "stream did not contain valid UTF-8",
            ))
        }))
    } else {
        debug_assert!(buf.is_empty());
        debug_assert_eq!(*read, 0);
        // Safety: `bytes` is a valid UTF-8 because `str::from_utf8` returned `Ok`.
        mem::swap(unsafe { buf.as_mut_vec() }, bytes);
        Poll::Ready(ret)
    }
}

pin_project! {
    /// A stream over the contents of an instance of [`BufRead`] split on a particular byte.
    ///
    /// This stream is created by the [`split`] method on types that implement [`BufRead`].
    ///
    /// This type is an async version of [`std::io::Split`].
    ///
    /// [`split`]: trait.BufRead.html#method.lines
    /// [`BufRead`]: trait.BufRead.html
    /// [`std::io::Split`]: https://doc.rust-lang.org/std/io/struct.Split.html
    #[derive(Debug)]
    pub struct Split<R> {
        #[pin]
        reader: R,
        buf: Vec<u8>,
        read: usize,
        delim: u8,
    }
}

impl<R: AsyncBufRead> Stream for Split<R> {
    type Item = Result<Vec<u8>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        let n = ready!(read_until_internal(
            this.reader,
            cx,
            *this.delim,
            this.buf,
            this.read
        ))?;
        if n == 0 && this.buf.is_empty() {
            return Poll::Ready(None);
        }
        if this.buf[this.buf.len() - 1] == *this.delim {
            this.buf.pop();
        }
        Poll::Ready(Some(Ok(mem::replace(this.buf, vec![]))))
    }
}

pub trait AsyncReadExt: AsyncRead {
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> ReadFuture<'a, Self>
    where
        Self: Unpin,
    {
        ReadFuture { reader: self, buf }
    }

    fn read_vectored<'a>(
        &'a mut self,
        bufs: &'a mut [IoSliceMut<'a>],
    ) -> ReadVectoredFuture<'a, Self>
    where
        Self: Unpin,
    {
        ReadVectoredFuture { reader: self, bufs }
    }

    fn read_to_end<'a>(&'a mut self, buf: &'a mut Vec<u8>) -> ReadToEndFuture<'a, Self>
    where
        Self: Unpin,
    {
        let start_len = buf.len();
        ReadToEndFuture {
            reader: self,
            buf,
            start_len,
        }
    }

    fn read_to_string<'a>(&'a mut self, buf: &'a mut String) -> ReadToStringFuture<'a, Self>
    where
        Self: Unpin,
    {
        let start_len = buf.len();
        ReadToStringFuture {
            reader: self,
            bytes: unsafe { mem::replace(buf.as_mut_vec(), Vec::new()) },
            buf,
            start_len,
        }
    }

    fn read_exact<'a>(&'a mut self, buf: &'a mut [u8]) -> ReadExactFuture<'a, Self>
    where
        Self: Unpin,
    {
        ReadExactFuture { reader: self, buf }
    }

    fn take(self, limit: u64) -> Take<Self>
    where
        Self: Sized,
    {
        Take { inner: self, limit }
    }

    fn bytes(self) -> Bytes<Self>
    where
        Self: Sized,
    {
        Bytes { inner: self }
    }

    fn chain<R: Read>(self, next: R) -> Chain<Self, R>
    where
        Self: Sized,
    {
        Chain {
            first: self,
            second: next,
            done_first: false,
        }
    }
}

impl<R: AsyncRead + ?Sized> AsyncReadExt for R {}

pub struct ReadFuture<'a, T: Unpin + ?Sized> {
    reader: &'a mut T,
    buf: &'a mut [u8],
}

impl<T: AsyncRead + Unpin + ?Sized> Future for ReadFuture<'_, T> {
    type Output = Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Self { reader, buf } = &mut *self;
        Pin::new(reader).poll_read(cx, buf)
    }
}

pub struct ReadVectoredFuture<'a, T: Unpin + ?Sized> {
    reader: &'a mut T,
    bufs: &'a mut [IoSliceMut<'a>],
}

impl<T: AsyncRead + Unpin + ?Sized> Future for ReadVectoredFuture<'_, T> {
    type Output = Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Self { reader, bufs } = &mut *self;
        Pin::new(reader).poll_read_vectored(cx, bufs)
    }
}

pub struct ReadToEndFuture<'a, T: Unpin + ?Sized> {
    reader: &'a mut T,
    buf: &'a mut Vec<u8>,
    start_len: usize,
}

impl<T: AsyncRead + Unpin + ?Sized> Future for ReadToEndFuture<'_, T> {
    type Output = Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Self {
            reader,
            buf,
            start_len,
        } = &mut *self;
        read_to_end_internal(Pin::new(reader), cx, buf, *start_len)
    }
}

pub struct ReadToStringFuture<'a, T: Unpin + ?Sized> {
    reader: &'a mut T,
    buf: &'a mut String,
    bytes: Vec<u8>,
    start_len: usize,
}

impl<T: AsyncRead + Unpin + ?Sized> Future for ReadToStringFuture<'_, T> {
    type Output = Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Self {
            reader,
            buf,
            bytes,
            start_len,
        } = &mut *self;
        let reader = Pin::new(reader);

        let ret = ready!(read_to_end_internal(reader, cx, bytes, *start_len));
        if str::from_utf8(&bytes).is_err() {
            Poll::Ready(ret.and_then(|_| {
                Err(Error::new(
                    ErrorKind::InvalidData,
                    "stream did not contain valid UTF-8",
                ))
            }))
        } else {
            #[allow(clippy::debug_assert_with_mut_call)]
            {
                debug_assert!(buf.is_empty());
            }

            // Safety: `bytes` is a valid UTF-8 because `str::from_utf8` returned `Ok`.
            mem::swap(unsafe { buf.as_mut_vec() }, bytes);
            Poll::Ready(ret)
        }
    }
}

// This uses an adaptive system to extend the vector when it fills. We want to
// avoid paying to allocate and zero a huge chunk of memory if the reader only
// has 4 bytes while still making large reads if the reader does have a ton
// of data to return. Simply tacking on an extra DEFAULT_BUF_SIZE space every
// time is 4,500 times (!) slower than this if the reader has a very small
// amount of data to return.
//
// Because we're extending the buffer with uninitialized data for trusted
// readers, we need to make sure to truncate that if any of this panics.
fn read_to_end_internal<R: AsyncRead + ?Sized>(
    mut rd: Pin<&mut R>,
    cx: &mut Context<'_>,
    buf: &mut Vec<u8>,
    start_len: usize,
) -> Poll<Result<usize>> {
    struct Guard<'a> {
        buf: &'a mut Vec<u8>,
        len: usize,
    }

    impl Drop for Guard<'_> {
        fn drop(&mut self) {
            unsafe {
                self.buf.set_len(self.len);
            }
        }
    }

    let mut g = Guard {
        len: buf.len(),
        buf,
    };
    let ret;
    loop {
        if g.len == g.buf.len() {
            unsafe {
                g.buf.reserve(32);
                let capacity = g.buf.capacity();
                g.buf.set_len(capacity);
                for byte in &mut g.buf[g.len..] {
                    *byte = 0;
                }
            }
        }

        match ready!(rd.as_mut().poll_read(cx, &mut g.buf[g.len..])) {
            Ok(0) => {
                ret = Poll::Ready(Ok(g.len - start_len));
                break;
            }
            Ok(n) => g.len += n,
            Err(e) => {
                ret = Poll::Ready(Err(e));
                break;
            }
        }
    }

    ret
}

pub struct ReadExactFuture<'a, T: Unpin + ?Sized> {
    reader: &'a mut T,
    buf: &'a mut [u8],
}

impl<T: AsyncRead + Unpin + ?Sized> Future for ReadExactFuture<'_, T> {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Self { reader, buf } = &mut *self;

        while !buf.is_empty() {
            let n = ready!(Pin::new(&mut *reader).poll_read(cx, buf))?;
            let (_, rest) = mem::replace(buf, &mut []).split_at_mut(n);
            *buf = rest;

            if n == 0 {
                return Poll::Ready(Err(ErrorKind::UnexpectedEof.into()));
            }
        }

        Poll::Ready(Ok(()))
    }
}

pin_project! {
    /// Reader adaptor which limits the bytes read from an underlying reader.
    ///
    /// This struct is generally created by calling [`take`] on a reader.
    /// Please see the documentation of [`take`] for more details.
    ///
    /// [`take`]: trait.Read.html#method.take
    #[derive(Debug)]
    pub struct Take<T> {
        #[pin]
        inner: T,
        limit: u64,
    }
}

impl<T> Take<T> {
    /// Returns the number of bytes that can be read before this instance will
    /// return EOF.
    ///
    /// # Note
    ///
    /// This instance may reach `EOF` after reading fewer bytes than indicated by
    /// this method if the underlying [`Read`] instance reaches EOF.
    ///
    /// [`Read`]: trait.Read.html
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> async_std::io::Result<()> { async_std::task::block_on(async {
    /// #
    /// use async_std::prelude::*;
    /// use async_std::fs::File;
    ///
    /// let f = File::open("foo.txt").await?;
    ///
    /// // read at most five bytes
    /// let handle = f.take(5);
    ///
    /// println!("limit: {}", handle.limit());
    /// #
    /// #     Ok(()) }) }
    /// ```
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// Sets the number of bytes that can be read before this instance will
    /// return EOF. This is the same as constructing a new `Take` instance, so
    /// the amount of bytes read and the previous limit value don't matter when
    /// calling this method.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> async_std::io::Result<()> { async_std::task::block_on(async {
    /// #
    /// use async_std::prelude::*;
    /// use async_std::fs::File;
    ///
    /// let f = File::open("foo.txt").await?;
    ///
    /// // read at most five bytes
    /// let mut handle = f.take(5);
    /// handle.set_limit(10);
    ///
    /// assert_eq!(handle.limit(), 10);
    /// #
    /// # Ok(()) }) }
    /// ```
    pub fn set_limit(&mut self, limit: u64) {
        self.limit = limit;
    }

    /// Consumes the `Take`, returning the wrapped reader.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> async_std::io::Result<()> { async_std::task::block_on(async {
    /// #
    /// use async_std::prelude::*;
    /// use async_std::fs::File;
    ///
    /// let file = File::open("foo.txt").await?;
    ///
    /// let mut buffer = [0; 5];
    /// let mut handle = file.take(5);
    /// handle.read(&mut buffer).await?;
    ///
    /// let file = handle.into_inner();
    /// #
    /// # Ok(()) }) }
    /// ```
    pub fn into_inner(self) -> T {
        self.inner
    }

    /// Gets a reference to the underlying reader.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> async_std::io::Result<()> { async_std::task::block_on(async {
    /// #
    /// use async_std::prelude::*;
    /// use async_std::fs::File;
    ///
    /// let file = File::open("foo.txt").await?;
    ///
    /// let mut buffer = [0; 5];
    /// let mut handle = file.take(5);
    /// handle.read(&mut buffer).await?;
    ///
    /// let file = handle.get_ref();
    /// #
    /// # Ok(()) }) }
    /// ```
    pub fn get_ref(&self) -> &T {
        &self.inner
    }

    /// Gets a mutable reference to the underlying reader.
    ///
    /// Care should be taken to avoid modifying the internal I/O state of the
    /// underlying reader as doing so may corrupt the internal limit of this
    /// `Take`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> async_std::io::Result<()> { async_std::task::block_on(async {
    /// #
    /// use async_std::prelude::*;
    /// use async_std::fs::File;
    ///
    /// let file = File::open("foo.txt").await?;
    ///
    /// let mut buffer = [0; 5];
    /// let mut handle = file.take(5);
    /// handle.read(&mut buffer).await?;
    ///
    /// let file = handle.get_mut();
    /// #
    /// # Ok(()) }) }
    /// ```
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T: AsyncRead> AsyncRead for Take<T> {
    /// Attempt to read from the `AsyncRead` into `buf`.
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize>> {
        let this = self.project();
        take_read_internal(this.inner, cx, buf, this.limit)
    }
}

fn take_read_internal<R: AsyncRead + ?Sized>(
    mut rd: Pin<&mut R>,
    cx: &mut Context<'_>,
    buf: &mut [u8],
    limit: &mut u64,
) -> Poll<Result<usize>> {
    // Don't call into inner reader at all at EOF because it may still block
    if *limit == 0 {
        return Poll::Ready(Ok(0));
    }

    let max = cmp::min(buf.len() as u64, *limit) as usize;

    match ready!(rd.as_mut().poll_read(cx, &mut buf[..max])) {
        Ok(n) => {
            *limit -= n as u64;
            Poll::Ready(Ok(n))
        }
        Err(e) => Poll::Ready(Err(e)),
    }
}

impl<T: AsyncBufRead> AsyncBufRead for Take<T> {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<&[u8]>> {
        let this = self.project();

        if *this.limit == 0 {
            return Poll::Ready(Ok(&[]));
        }

        match ready!(this.inner.poll_fill_buf(cx)) {
            Ok(buf) => {
                let cap = cmp::min(buf.len() as u64, *this.limit) as usize;
                Poll::Ready(Ok(&buf[..cap]))
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        let this = self.project();
        // Don't let callers reset the limit by passing an overlarge value
        let amt = cmp::min(amt as u64, *this.limit) as usize;
        *this.limit -= amt as u64;

        this.inner.consume(amt);
    }
}

/// A stream over `u8` values of a reader.
///
/// This struct is generally created by calling [`bytes`] on a reader.
/// Please see the documentation of [`bytes`] for more details.
///
/// [`bytes`]: trait.Read.html#method.bytes
#[derive(Debug)]
pub struct Bytes<T> {
    inner: T,
}

impl<T: AsyncRead + Unpin> Stream for Bytes<T> {
    type Item = Result<u8>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut byte = 0;

        let rd = Pin::new(&mut self.inner);

        match ready!(rd.poll_read(cx, std::slice::from_mut(&mut byte))) {
            Ok(0) => Poll::Ready(None),
            Ok(..) => Poll::Ready(Some(Ok(byte))),
            Err(ref e) if e.kind() == ErrorKind::Interrupted => Poll::Pending,
            Err(e) => Poll::Ready(Some(Err(e))),
        }
    }
}

pin_project! {
    /// Adaptor to chain together two readers.
    ///
    /// This struct is generally created by calling [`chain`] on a reader.
    /// Please see the documentation of [`chain`] for more details.
    ///
    /// [`chain`]: trait.Read.html#method.chain
    pub struct Chain<T, U> {
        #[pin]
        first: T,
        #[pin]
        second: U,
        done_first: bool,
    }
}

impl<T, U> Chain<T, U> {
    /// Consumes the `Chain`, returning the wrapped readers.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> async_std::io::Result<()> { async_std::task::block_on(async {
    /// #
    /// use async_std::prelude::*;
    /// use async_std::fs::File;
    ///
    /// let foo_file = File::open("foo.txt").await?;
    /// let bar_file = File::open("bar.txt").await?;
    ///
    /// let chain = foo_file.chain(bar_file);
    /// let (foo_file, bar_file) = chain.into_inner();
    /// #
    /// # Ok(()) }) }
    /// ```
    pub fn into_inner(self) -> (T, U) {
        (self.first, self.second)
    }

    /// Gets references to the underlying readers in this `Chain`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> async_std::io::Result<()> { async_std::task::block_on(async {
    /// #
    /// use async_std::prelude::*;
    /// use async_std::fs::File;
    ///
    /// let foo_file = File::open("foo.txt").await?;
    /// let bar_file = File::open("bar.txt").await?;
    ///
    /// let chain = foo_file.chain(bar_file);
    /// let (foo_file, bar_file) = chain.get_ref();
    /// #
    /// # Ok(()) }) }
    /// ```
    pub fn get_ref(&self) -> (&T, &U) {
        (&self.first, &self.second)
    }

    /// Gets mutable references to the underlying readers in this `Chain`.
    ///
    /// Care should be taken to avoid modifying the internal I/O state of the
    /// underlying readers as doing so may corrupt the internal state of this
    /// `Chain`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> async_std::io::Result<()> { async_std::task::block_on(async {
    /// #
    /// use async_std::prelude::*;
    /// use async_std::fs::File;
    ///
    /// let foo_file = File::open("foo.txt").await?;
    /// let bar_file = File::open("bar.txt").await?;
    ///
    /// let mut chain = foo_file.chain(bar_file);
    /// let (foo_file, bar_file) = chain.get_mut();
    /// #
    /// # Ok(()) }) }
    /// ```
    pub fn get_mut(&mut self) -> (&mut T, &mut U) {
        (&mut self.first, &mut self.second)
    }
}

impl<T: fmt::Debug, U: fmt::Debug> fmt::Debug for Chain<T, U> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Chain")
            .field("t", &self.first)
            .field("u", &self.second)
            .finish()
    }
}

impl<T: AsyncRead, U: AsyncRead> AsyncRead for Chain<T, U> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize>> {
        let this = self.project();
        if !*this.done_first {
            match ready!(this.first.poll_read(cx, buf)) {
                Ok(0) if !buf.is_empty() => *this.done_first = true,
                Ok(n) => return Poll::Ready(Ok(n)),
                Err(err) => return Poll::Ready(Err(err)),
            }
        }

        this.second.poll_read(cx, buf)
    }

    fn poll_read_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
    ) -> Poll<Result<usize>> {
        let this = self.project();
        if !*this.done_first {
            match ready!(this.first.poll_read_vectored(cx, bufs)) {
                Ok(0) if !bufs.is_empty() => *this.done_first = true,
                Ok(n) => return Poll::Ready(Ok(n)),
                Err(err) => return Poll::Ready(Err(err)),
            }
        }

        this.second.poll_read_vectored(cx, bufs)
    }
}

impl<T: AsyncBufRead, U: AsyncBufRead> AsyncBufRead for Chain<T, U> {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<&[u8]>> {
        let this = self.project();
        if !*this.done_first {
            match ready!(this.first.poll_fill_buf(cx)) {
                Ok(buf) if buf.is_empty() => {
                    *this.done_first = true;
                }
                Ok(buf) => return Poll::Ready(Ok(buf)),
                Err(err) => return Poll::Ready(Err(err)),
            }
        }

        this.second.poll_fill_buf(cx)
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        let this = self.project();
        if !*this.done_first {
            this.first.consume(amt)
        } else {
            this.second.consume(amt)
        }
    }
}

pub trait AsyncSeekExt: AsyncSeek {
    /// Creates a future which will seek an IO object, and then yield the
    /// new position in the object and the object itself.
    ///
    /// In the case of an error the buffer and the object will be discarded, with
    /// the error yielded.
    fn seek(&mut self, pos: SeekFrom) -> SeekFuture<'_, Self>
    where
        Self: Unpin,
    {
        SeekFuture { seeker: self, pos }
    }
}

impl<S: AsyncSeek + ?Sized> AsyncSeekExt for S {}

pub struct SeekFuture<'a, T: Unpin + ?Sized> {
    seeker: &'a mut T,
    pos: SeekFrom,
}

impl<T: AsyncSeek + Unpin + ?Sized> Future for SeekFuture<'_, T> {
    type Output = Result<u64>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let pos = self.pos;
        Pin::new(&mut *self.seeker).poll_seek(cx, pos)
    }
}

pub trait AsyncWriteExt: AsyncWrite {
    fn write<'a>(&'a mut self, buf: &'a [u8]) -> WriteFuture<'a, Self>
    where
        Self: Unpin,
    {
        WriteFuture { writer: self, buf }
    }

    fn flush(&mut self) -> FlushFuture<'_, Self>
    where
        Self: Unpin,
    {
        FlushFuture { writer: self }
    }

    fn write_vectored<'a>(&'a mut self, bufs: &'a [IoSlice<'a>]) -> WriteVectoredFuture<'a, Self>
    where
        Self: Unpin,
    {
        WriteVectoredFuture { writer: self, bufs }
    }

    fn write_all<'a>(&'a mut self, buf: &'a [u8]) -> WriteAllFuture<'a, Self>
    where
        Self: Unpin,
    {
        WriteAllFuture { writer: self, buf }
    }
}

impl<R: AsyncWrite + ?Sized> AsyncWriteExt for R {}

pub struct WriteFuture<'a, T: Unpin + ?Sized> {
    writer: &'a mut T,
    buf: &'a [u8],
}

impl<T: AsyncWrite + Unpin + ?Sized> Future for WriteFuture<'_, T> {
    type Output = Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let buf = self.buf;
        Pin::new(&mut *self.writer).poll_write(cx, buf)
    }
}

pub struct FlushFuture<'a, T: Unpin + ?Sized> {
    writer: &'a mut T,
}

impl<T: AsyncWrite + Unpin + ?Sized> Future for FlushFuture<'_, T> {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut *self.writer).poll_flush(cx)
    }
}

pub struct WriteVectoredFuture<'a, T: Unpin + ?Sized> {
    writer: &'a mut T,
    bufs: &'a [IoSlice<'a>],
}

impl<T: AsyncWrite + Unpin + ?Sized> Future for WriteVectoredFuture<'_, T> {
    type Output = Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let bufs = self.bufs;
        Pin::new(&mut *self.writer).poll_write_vectored(cx, bufs)
    }
}

pub struct WriteAllFuture<'a, T: Unpin + ?Sized> {
    writer: &'a mut T,
    buf: &'a [u8],
}

impl<T: AsyncWrite + Unpin + ?Sized> Future for WriteAllFuture<'_, T> {
    type Output = Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Self { writer, buf } = &mut *self;

        while !buf.is_empty() {
            let n = futures_core::ready!(Pin::new(&mut **writer).poll_write(cx, buf))?;
            let (_, rest) = mem::replace(buf, &[]).split_at(n);
            *buf = rest;

            if n == 0 {
                return Poll::Ready(Err(ErrorKind::WriteZero.into()));
            }
        }

        Poll::Ready(Ok(()))
    }
}
