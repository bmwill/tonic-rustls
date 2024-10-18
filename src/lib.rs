//! Batteries included server and client.
//!
//! This module provides a set of batteries included, fully featured and
//! fast set of HTTP/2 server and client's. These components each provide a
//! `rustls` tls backend when the respective feature flag is enabled, and
//! provides builders to configure transport behavior.
//!
//! # Features
//!
//! - TLS support via [rustls].
//! - Load balancing
//! - Timeouts
//! - Concurrency Limits
//! - Rate limiting
//!
//! [rustls]: https://docs.rs/rustls

#[cfg(feature = "channel")]
pub mod channel;
#[cfg(feature = "server")]
pub mod server;

mod error;
mod service;

#[doc(inline)]
#[cfg(feature = "channel")]
pub use self::channel::{Channel, Endpoint};
pub use self::error::Error;
pub(crate) use self::error::BoxError;
#[doc(inline)]
#[cfg(feature = "server")]
pub use self::server::Server;

pub use hyper::{body::Body, Uri};
#[cfg(feature = "tls")]
pub use tokio_rustls::rustls::pki_types::CertificateDer;
