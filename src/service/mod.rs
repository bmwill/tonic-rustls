pub(crate) mod grpc_timeout;
/// h2 alpn in plain format for rustls.
#[cfg(feature = "tls")]
pub(crate) const ALPN_H2: &[u8] = b"h2";

pub(crate) use self::grpc_timeout::GrpcTimeout;
