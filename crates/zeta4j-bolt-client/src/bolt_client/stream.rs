//! Bolt client connection stream types.

use std::io::{self, Read, Write};
use std::net::TcpStream;

use rustls::{ClientConnection, StreamOwned};

/// TLS 스트림 타입 별칭
pub(super) type TlsStream = StreamOwned<ClientConnection, TcpStream>;

/// 연결 스트림 (Plain TCP 또는 TLS)
pub(super) enum ConnectionStream {
    /// Plain TCP 연결
    Plain(TcpStream),
    /// TLS 암호화 연결
    Tls(Box<TlsStream>),
}

impl Read for ConnectionStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            ConnectionStream::Plain(s) => s.read(buf),
            ConnectionStream::Tls(s) => s.read(buf),
        }
    }
}

impl Write for ConnectionStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            ConnectionStream::Plain(s) => s.write(buf),
            ConnectionStream::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            ConnectionStream::Plain(s) => s.flush(),
            ConnectionStream::Tls(s) => s.flush(),
        }
    }
}
