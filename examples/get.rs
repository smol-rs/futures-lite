use async_net::TcpStream;
use blocking::{block_on, Unblock};
use futures_lite::*;

fn main() -> io::Result<()> {
    block_on(async {
        let mut stream = TcpStream::connect("example.com:80").await?;
        let req = b"GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n";
        stream.write_all(req).await?;

        let mut stdout = Unblock::new(std::io::stdout());
        io::copy(&stream, &mut stdout).await?;
        Ok(())
    })
}
