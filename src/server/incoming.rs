use std::{
    io,
    net::{SocketAddr, TcpListener as StdTcpListener},
    ops::ControlFlow,
    pin::{pin, Pin},
    task::{ready, Context, Poll},
    time::Duration,
};

use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpListener, TcpStream},
};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_stream::{Stream, StreamExt};
use tracing::warn;

use super::service::ServerIo;
#[cfg(feature = "tls")]
use super::service::TlsAcceptor;

#[cfg(not(feature = "tls"))]
pub(crate) fn tcp_incoming<IO, IE>(
    incoming: impl Stream<Item = Result<IO, IE>>,
) -> impl Stream<Item = Result<ServerIo<IO>, crate::BoxError>>
where
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    IE: Into<crate::BoxError>,
{
    async_stream::try_stream! {
        let mut incoming = pin!(incoming);

        while let Some(item) = incoming.next().await {
            yield match item {
                Ok(_) => item.map(ServerIo::new_io)?,
                Err(e) => match handle_tcp_accept_error(e) {
                    ControlFlow::Continue(()) => continue,
                    ControlFlow::Break(e) => Err(e)?,
                }
            }
        }
    }
}

#[cfg(feature = "tls")]
pub(crate) fn tcp_incoming<IO, IE>(
    incoming: impl Stream<Item = Result<IO, IE>>,
    tls: Option<TlsAcceptor>,
) -> impl Stream<Item = Result<ServerIo<IO>, crate::BoxError>>
where
    IO: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    IE: Into<crate::BoxError>,
{
    async_stream::try_stream! {
        let mut incoming = pin!(incoming);

        let mut tasks = tokio::task::JoinSet::new();

        loop {
            match select(&mut incoming, &mut tasks).await {
                SelectOutput::Incoming(stream) => {
                    if let Some(tls) = &tls {
                        let tls = tls.clone();
                        tasks.spawn(async move {
                            let io = tls.accept(stream).await?;
                            Ok(ServerIo::new_tls_io(io))
                        });
                    } else {
                        yield ServerIo::new_io(stream);
                    }
                }

                SelectOutput::Io(io) => {
                    yield io;
                }

                SelectOutput::TcpErr(e) => match handle_tcp_accept_error(e) {
                    ControlFlow::Continue(()) => continue,
                    ControlFlow::Break(e) => Err(e)?,
                }

                SelectOutput::TlsErr(e) => {
                    tracing::debug!(error = %e, "tls accept error");
                    continue;
                }

                SelectOutput::Done => {
                    break;
                }
            }
        }
    }
}

fn handle_tcp_accept_error(e: impl Into<crate::error::BoxError>) -> ControlFlow<crate::error::BoxError> {
    let e = e.into();
    tracing::debug!(error = %e, "accept loop error");
    if let Some(e) = e.downcast_ref::<io::Error>() {
        if matches!(
            e.kind(),
            io::ErrorKind::ConnectionAborted
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::BrokenPipe
                | io::ErrorKind::Interrupted
                | io::ErrorKind::WouldBlock
                | io::ErrorKind::TimedOut
        ) {
            return ControlFlow::Continue(());
        }
    }

    ControlFlow::Break(e)
}

#[cfg(feature = "tls")]
async fn select<IO: 'static, IE>(
    incoming: &mut (impl Stream<Item = Result<IO, IE>> + Unpin),
    tasks: &mut tokio::task::JoinSet<Result<ServerIo<IO>, crate::BoxError>>,
) -> SelectOutput<IO>
where
    IE: Into<crate::BoxError>,
{
    if tasks.is_empty() {
        return match incoming.try_next().await {
            Ok(Some(stream)) => SelectOutput::Incoming(stream),
            Ok(None) => SelectOutput::Done,
            Err(e) => SelectOutput::TcpErr(e.into()),
        };
    }

    tokio::select! {
        stream = incoming.try_next() => {
            match stream {
                Ok(Some(stream)) => SelectOutput::Incoming(stream),
                Ok(None) => SelectOutput::Done,
                Err(e) => SelectOutput::TcpErr(e.into()),
            }
        }

        accept = tasks.join_next() => {
            match accept.expect("JoinSet should never end") {
                Ok(Ok(io)) => SelectOutput::Io(io),
                Ok(Err(e)) => SelectOutput::TlsErr(e),
                Err(e) => SelectOutput::TlsErr(e.into()),
            }
        }
    }
}

#[cfg(feature = "tls")]
enum SelectOutput<A> {
    Incoming(A),
    Io(ServerIo<A>),
    TcpErr(crate::BoxError),
    TlsErr(crate::BoxError),
    Done,
}

/// Binds a socket address for a [Router](super::Router)
///
/// An incoming stream, usable with [Router::serve_with_incoming](super::Router::serve_with_incoming),
/// of `AsyncRead + AsyncWrite` that communicate with clients that connect to a socket address.
#[derive(Debug)]
pub struct TcpIncoming {
    inner: TcpListenerStream,
    nodelay: bool,
    keepalive: Option<Duration>,
}

impl TcpIncoming {
    /// Creates an instance by binding (opening) the specified socket address
    /// to which the specified TCP 'nodelay' and 'keepalive' parameters are applied.
    /// Returns a TcpIncoming if the socket address was successfully bound.
    ///
    /// # Examples
    /// ```no_run
    /// # use tower_service::Service;
    /// # use http::{request::Request, response::Response};
    /// # use tonic::{body::BoxBody, server::NamedService};
    /// # use tonic_rustls::{Server, server::TcpIncoming};
    /// # use core::convert::Infallible;
    /// # use std::error::Error;
    /// # fn main() { }  // Cannot have type parameters, hence instead define:
    /// # fn run<S>(some_service: S) -> Result<(), Box<dyn Error + Send + Sync>>
    /// # where
    /// #   S: Service<Request<BoxBody>, Response = Response<BoxBody>, Error = Infallible> + NamedService + Clone + Send + 'static,
    /// #   S::Future: Send + 'static,
    /// # {
    /// // Find a free port
    /// let mut port = 1322;
    /// let tinc = loop {
    ///    let addr = format!("127.0.0.1:{}", port).parse().unwrap();
    ///    match TcpIncoming::new(addr, true, None) {
    ///       Ok(t) => break t,
    ///       Err(_) => port += 1
    ///    }
    /// };
    /// Server::builder()
    ///    .add_service(some_service)
    ///    .serve_with_incoming(tinc);
    /// # Ok(())
    /// # }
    pub fn new(
        addr: SocketAddr,
        nodelay: bool,
        keepalive: Option<Duration>,
    ) -> Result<Self, crate::BoxError> {
        let std_listener = StdTcpListener::bind(addr)?;
        std_listener.set_nonblocking(true)?;

        let inner = TcpListenerStream::new(TcpListener::from_std(std_listener)?);
        Ok(Self {
            inner,
            nodelay,
            keepalive,
        })
    }

    /// Creates a new `TcpIncoming` from an existing `tokio::net::TcpListener`.
    pub fn from_listener(
        listener: TcpListener,
        nodelay: bool,
        keepalive: Option<Duration>,
    ) -> Result<Self, crate::BoxError> {
        Ok(Self {
            inner: TcpListenerStream::new(listener),
            nodelay,
            keepalive,
        })
    }
}

impl Stream for TcpIncoming {
    type Item = Result<TcpStream, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match ready!(Pin::new(&mut self.inner).poll_next(cx)) {
            Some(Ok(stream)) => {
                set_accepted_socket_options(&stream, self.nodelay, self.keepalive);
                Some(Ok(stream)).into()
            }
            other => Poll::Ready(other),
        }
    }
}

// Consistent with hyper-0.14, this function does not return an error.
fn set_accepted_socket_options(stream: &TcpStream, nodelay: bool, keepalive: Option<Duration>) {
    if nodelay {
        if let Err(e) = stream.set_nodelay(true) {
            warn!("error trying to set TCP nodelay: {}", e);
        }
    }

    if let Some(timeout) = keepalive {
        let sock_ref = socket2::SockRef::from(&stream);
        let sock_keepalive = socket2::TcpKeepalive::new().with_time(timeout);

        if let Err(e) = sock_ref.set_tcp_keepalive(&sock_keepalive) {
            warn!("error trying to set TCP keepalive: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::server::TcpIncoming;
    #[tokio::test]
    async fn one_tcpincoming_at_a_time() {
        let addr = "127.0.0.1:1322".parse().unwrap();
        {
            let _t1 = TcpIncoming::new(addr, true, None).unwrap();
            let _t2 = TcpIncoming::new(addr, true, None).unwrap_err();
        }
        let _t3 = TcpIncoming::new(addr, true, None).unwrap();
    }
}
