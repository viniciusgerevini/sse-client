use native_tls::{TlsConnector, TlsStream};
use std::net::TcpStream;
use std::io::{self, Result};
use std::time::Duration;

pub(crate) enum MaybeTlsStream {
    Tls(TlsStream<TcpStream>),
    Plain(TcpStream),
}

impl MaybeTlsStream {
    pub(crate) fn try_wrap_tls(plain: TcpStream, sni: &str) -> Result<MaybeTlsStream> {
        let connector = TlsConnector::new().unwrap();
        let stream = connector.connect(sni, plain).map_err(|_| io::Error::from(io::ErrorKind::Other))?;
        Ok(Self::Tls(stream))
    }

    #[inline]
    pub(crate) fn wrap_plain(plain: TcpStream) -> MaybeTlsStream {
        Self::Plain(plain)
    }

    pub(crate) fn set_read_timeout(&self, dur: Option<Duration>) -> Result<()> {
        match self {
            Self::Tls(inner) => inner.get_ref().set_read_timeout(dur),
            Self::Plain(inner) => inner.set_read_timeout(dur),
        }
    }

    pub(crate) fn clone_plain_handle(&self) -> Result<TcpStream> {
        match self {
            Self::Tls(inner) => inner.get_ref().try_clone(),
            Self::Plain(inner) => inner.try_clone(),
        }
    }
}

impl io::Read for MaybeTlsStream {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        match self {
            Self::Tls(inner) => inner.read(buf),
            Self::Plain(inner) => inner.read(buf),
        }
    }
}

impl io::Write for MaybeTlsStream {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        match self {
            Self::Tls(inner) => inner.write(buf),
            Self::Plain(inner) => inner.write(buf),
        }
    }

    #[inline]
    fn flush(&mut self) -> Result<()> {
        match self {
            Self::Tls(inner) => inner.flush(),
            Self::Plain(inner) => inner.flush(),
        }
    }
}
