use std::fmt;
use std::sync::Arc;

use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::{
    rustls::{pki_types::ServerName, ClientConfig},
    TlsConnector as RustlsConnector,
};

use super::io::BoxedIo;
use crate::service::ALPN_H2;

#[derive(Clone)]
pub(crate) struct TlsConnector {
    config: Arc<ClientConfig>,
    domain: Arc<ServerName<'static>>,
    assume_http2: bool,
}

impl TlsConnector {
    pub(crate) fn new(
        mut config: ClientConfig,
        domain: &str,
        assume_http2: bool,
    ) -> Result<Self, crate::BoxError> {
        config.alpn_protocols.push(ALPN_H2.into());

        Ok(Self {
            config: Arc::new(config),
            domain: Arc::new(ServerName::try_from(domain)?.to_owned()),
            assume_http2,
        })
    }

    pub(crate) async fn connect<I>(&self, io: I) -> Result<BoxedIo, crate::BoxError>
    where
        I: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        let io = RustlsConnector::from(self.config.clone())
            .connect(self.domain.as_ref().to_owned(), io)
            .await?;

        // Generally we require ALPN to be negotiated, but if the user has
        // explicitly set `assume_http2` to true, we'll allow it to be missing.
        let (_, session) = io.get_ref();
        let alpn_protocol = session.alpn_protocol();
        if !(alpn_protocol == Some(ALPN_H2) || self.assume_http2) {
            return Err("HTTP/2 was not negotiated".into());
        }
        Ok(BoxedIo::new(TokioIo::new(io)))
    }
}

impl fmt::Debug for TlsConnector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsConnector").finish()
    }
}
