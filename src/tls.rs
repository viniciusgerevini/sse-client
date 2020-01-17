use native_tls::{TlsConnector, TlsStream};
use std::net::TcpStream;
use std::io::{self, Result};
use std::time::Duration;
use std::sync::Arc;
use std::sync::Mutex;

type StreamWrapper<T> = Arc<Mutex<T>>;

pub(crate) enum MaybeTlsStream {
    Tls(StreamWrapper<TlsStream<TcpStream>>),
    Plain(StreamWrapper<TcpStream>),
}

impl MaybeTlsStream {
    pub(crate) fn try_wrap_tls(plain: TcpStream, sni: &str) -> Result<MaybeTlsStream> {
        let connector = TlsConnector::new().unwrap();
        let stream = connector.connect(sni, plain).map_err(|_| io::Error::from(io::ErrorKind::Other))?;
        Ok(Self::Tls(Arc::new(Mutex::new(stream))))
    }

    pub(crate) fn wrap_plain(plain: TcpStream) -> MaybeTlsStream {
        Self::Plain(Arc::new(Mutex::new(plain)))
    }

    pub(crate) fn set_read_timeout(&self, dur: Option<Duration>) -> Result<()> {
        match self {
            Self::Tls(inner) => inner.lock().unwrap().get_ref().set_read_timeout(dur),
            Self::Plain(inner) => inner.lock().unwrap().set_read_timeout(dur),
        }
    }

    pub(crate) fn clone_plain_handle(&self) -> Result<TcpStream> {
        match self {
            Self::Tls(inner) => inner.lock().unwrap().get_ref().try_clone(),
            Self::Plain(inner) => inner.lock().unwrap().try_clone(),
        }
    }
}

impl io::Read for MaybeTlsStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        match self {
            Self::Tls(inner) => inner.lock().unwrap().read(buf),
            Self::Plain(inner) => inner.lock().unwrap().read(buf),
        }
    }
}

impl io::Read for &MaybeTlsStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        match self {
            MaybeTlsStream::Tls(inner) => inner.lock().unwrap().read(buf),
            MaybeTlsStream::Plain(inner) => inner.lock().unwrap().read(buf),
        }
    }
}

impl io::Write for MaybeTlsStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        match self {
            Self::Tls(inner) => inner.lock().unwrap().write(buf),
            Self::Plain(inner) => inner.lock().unwrap().write(buf),
        }
    }

    fn flush(&mut self) -> Result<()> {
        match self {
            Self::Tls(inner) => inner.lock().unwrap().flush(),
            Self::Plain(inner) => inner.lock().unwrap().flush(),
        }
    }
}
