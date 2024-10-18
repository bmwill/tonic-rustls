use std::{fmt, sync::Arc};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::{rustls::ServerConfig, server::TlsStream, TlsAcceptor as RustlsAcceptor};

use crate::service::ALPN_H2;

#[derive(Clone)]
pub(crate) struct TlsAcceptor {
    inner: Arc<ServerConfig>,
}

impl TlsAcceptor {
    pub(crate) fn new(mut config: ServerConfig) -> Result<Self, crate::BoxError> {
        config.alpn_protocols.push(ALPN_H2.into());

        Ok(Self {
            inner: Arc::new(config),
        })
    }

    pub(crate) async fn accept<IO>(&self, io: IO) -> Result<TlsStream<IO>, crate::BoxError>
    where
        IO: AsyncRead + AsyncWrite + Unpin,
    {
        let acceptor = RustlsAcceptor::from(self.inner.clone());
        acceptor.accept(io).await.map_err(Into::into)
    }
}

impl fmt::Debug for TlsAcceptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsAcceptor").finish()
    }
}
