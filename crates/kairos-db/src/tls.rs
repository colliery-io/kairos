//! TLS for the async connections: the bb8/diesel-async pool and the
//! server's LISTEN connection.
//!
//! The sync path (migrations, the r2d2 bridge) goes through libpq, which
//! follows `sslmode` on its own. The async path is tokio-postgres, which
//! negotiates TLS only with a connector. With `NoTls` (diesel-async's
//! default) a URL with `sslmode=require` cannot connect at all, and with the
//! default `prefer` it connects in plain text even where the server offers
//! TLS — so a managed database that requires TLS (Aurora, Cloud SQL, ...)
//! migrated and then hung on every pooled checkout.
//!
//! [`MakeRustlsConnect`] encrypts and does not verify the server's
//! certificate. That is what libpq does for `prefer` and `require`, the only
//! modes tokio-postgres accepts (it refuses `verify-ca`, `verify-full` and
//! `sslrootcert` when it parses the URL). `sslmode=disable` still skips TLS:
//! tokio-postgres never calls the connector then.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use diesel::ConnectionResult;
use diesel::result::ConnectionError;
use diesel_async::AsyncPgConnection;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_postgres::Socket;
use tokio_postgres::tls::{ChannelBinding, MakeTlsConnect, TlsConnect, TlsStream};

/// Open one [`AsyncPgConnection`] with [`MakeRustlsConnect`]: the pool's
/// setup callback (see [`crate::pool::TenantPool::new`]).
pub async fn establish(database_url: &str) -> ConnectionResult<AsyncPgConnection> {
    let (client, connection) = tokio_postgres::connect(database_url, MakeRustlsConnect::new())
        .await
        .map_err(|e| ConnectionError::BadConnection(e.to_string()))?;
    AsyncPgConnection::try_from_client_and_connection(client, connection).await
}

/// A tokio-postgres TLS connector over rustls that encrypts without
/// verifying the certificate (libpq's `prefer` / `require`).
#[derive(Clone)]
pub struct MakeRustlsConnect {
    config: Arc<ClientConfig>,
}

impl MakeRustlsConnect {
    pub fn new() -> Self {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .expect("the ring provider supports the default protocol versions")
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoCertificateVerification(provider)))
            .with_no_client_auth();
        Self {
            config: Arc::new(config),
        }
    }
}

impl Default for MakeRustlsConnect {
    fn default() -> Self {
        Self::new()
    }
}

impl MakeTlsConnect<Socket> for MakeRustlsConnect {
    type Stream = RustlsStream;
    type TlsConnect = RustlsConnect;
    type Error = rustls::pki_types::InvalidDnsNameError;

    fn make_tls_connect(&mut self, domain: &str) -> Result<RustlsConnect, Self::Error> {
        // A Unix socket has no host name; the name only goes into SNI, and
        // nothing is verified against it.
        let domain = if domain.is_empty() {
            "localhost"
        } else {
            domain
        };
        Ok(RustlsConnect {
            connector: tokio_rustls::TlsConnector::from(self.config.clone()),
            name: ServerName::try_from(domain.to_owned())?,
        })
    }
}

pub struct RustlsConnect {
    connector: tokio_rustls::TlsConnector,
    name: ServerName<'static>,
}

impl TlsConnect<Socket> for RustlsConnect {
    type Stream = RustlsStream;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = io::Result<RustlsStream>> + Send>>;

    fn connect(self, stream: Socket) -> Self::Future {
        Box::pin(async move {
            self.connector
                .connect(self.name, stream)
                .await
                .map(RustlsStream)
        })
    }
}

pub struct RustlsStream(tokio_rustls::client::TlsStream<Socket>);

impl AsyncRead for RustlsStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for RustlsStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}

impl TlsStream for RustlsStream {
    fn channel_binding(&self) -> ChannelBinding {
        // No tls-server-end-point binding: SCRAM still authenticates, as it
        // does under libpq's default `channel_binding=prefer`.
        ChannelBinding::none()
    }
}

/// Accepts any certificate. The handshake signatures are still checked, so
/// the session is encrypted to whoever holds the presented key.
#[derive(Debug)]
struct NoCertificateVerification(Arc<CryptoProvider>);

impl ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
