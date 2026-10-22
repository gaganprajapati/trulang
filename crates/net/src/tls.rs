//! Native TLS 1.3 Termination Engine.
//!
//! Provides pure-Rust TLS termination using `rustls 0.23+` without OpenSSL dependencies.
//! Implements a non-blocking TLS poller state machine, ALPN negotiation (http/1.1),
//! and a 3-tier Anti-Slowloris defense.

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::ServerConfig;
use rustls::ServerConnection;
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Hard limits for TLS Anti-Slowloris & DDoS Defense.
pub const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(3);
pub const MAX_IN_FLIGHT_HANDSHAKES: usize = 1024;
pub const MAX_RECORD_FRAGMENT_BYTES: usize = 16 * 1024; // 16 KB

/// TLS Acceptor Configuration Builder.
#[derive(Clone)]
pub struct TlsAcceptorConfig {
    pub server_config: Arc<ServerConfig>,
}

impl TlsAcceptorConfig {
    /// Constructs a `TlsAcceptorConfig` from PEM-formatted certificate chain and private key bytes.
    pub fn from_pem_bytes(cert_pem: &[u8], key_pem: &[u8]) -> io::Result<Self> {
        let certs = Self::load_certs_from_reader(&mut BufReader::new(cert_pem))?;
        let key = Self::load_key_from_reader(&mut BufReader::new(key_pem))?;
        Self::from_certs_and_key(certs, key)
    }

    /// Constructs a `TlsAcceptorConfig` from PEM certificate and key file paths.
    pub fn from_pem_files<P: AsRef<Path>, Q: AsRef<Path>>(
        cert_path: P,
        key_path: Q,
    ) -> io::Result<Self> {
        let cert_file = File::open(cert_path.as_ref()).map_err(|e| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("Failed to open cert file: {}", e),
            )
        })?;
        let key_file = File::open(key_path.as_ref()).map_err(|e| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("Failed to open key file: {}", e),
            )
        })?;

        let certs = Self::load_certs_from_reader(&mut BufReader::new(cert_file))?;
        let key = Self::load_key_from_reader(&mut BufReader::new(key_file))?;
        Self::from_certs_and_key(certs, key)
    }

    /// Constructs a `TlsAcceptorConfig` directly from DER certificates and key.
    pub fn from_certs_and_key(
        certs: Vec<CertificateDer<'static>>,
        key: PrivateKeyDer<'static>,
    ) -> io::Result<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();

        let mut config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Invalid TLS configuration: {}", e),
                )
            })?;

        // ADR-7.1 / ALPN Resolution: strictly advertise http/1.1 in Phase 7
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        config.max_fragment_size = Some(MAX_RECORD_FRAGMENT_BYTES);

        Ok(Self {
            server_config: Arc::new(config),
        })
    }

    fn load_certs_from_reader<R: io::BufRead>(
        reader: &mut R,
    ) -> io::Result<Vec<CertificateDer<'static>>> {
        let certs = rustls_pemfile::certs(reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Malformed certificate PEM: {}", e),
                )
            })?;

        if certs.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "No valid certificates found in PEM stream",
            ));
        }

        Ok(certs)
    }

    fn load_key_from_reader<R: io::BufRead>(reader: &mut R) -> io::Result<PrivateKeyDer<'static>> {
        let key = rustls_pemfile::private_key(reader).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Malformed private key PEM: {}", e),
            )
        })?;

        match key {
            Some(k) => Ok(k),
            None => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "No private key found in PEM stream",
            )),
        }
    }
}

/// State of a TLS connection during lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsConnectionState {
    Handshaking { started_at: Instant },
    Established,
}

/// Transport Stream abstraction: Plain TCP or TLS Encrypted TCP.
pub enum TransportStream {
    Plain(TcpStream),
    Tls {
        stream: TcpStream,
        session: Box<ServerConnection>,
        state: TlsConnectionState,
    },
}

impl TransportStream {
    /// Creates a plain TCP transport.
    pub fn plain(stream: TcpStream) -> Self {
        Self::Plain(stream)
    }

    /// Creates a TLS encrypted transport beginning in Handshaking state.
    pub fn tls(stream: TcpStream, config: Arc<ServerConfig>) -> io::Result<Self> {
        let session = ServerConnection::new(config)
            .map_err(|e| io::Error::other(format!("Failed to create TLS session: {}", e)))?;

        Ok(Self::Tls {
            stream,
            session: Box::new(session),
            state: TlsConnectionState::Handshaking {
                started_at: Instant::now(),
            },
        })
    }

    /// Returns a reference to the underlying raw `TcpStream`.
    pub fn tcp_stream(&self) -> &TcpStream {
        match self {
            Self::Plain(s) => s,
            Self::Tls { stream, .. } => stream,
        }
    }

    /// Returns `true` if this transport is a TLS stream currently handshaking.
    pub fn is_handshaking(&self) -> bool {
        match self {
            Self::Plain(_) => false,
            Self::Tls { state, .. } => matches!(state, TlsConnectionState::Handshaking { .. }),
        }
    }

    /// Checks if a handshaking TLS connection has timed out under the 3.0s Anti-Slowloris limit.
    pub fn has_handshake_timed_out(&self) -> bool {
        match self {
            Self::Plain(_) => false,
            Self::Tls {
                state: TlsConnectionState::Handshaking { started_at },
                ..
            } => started_at.elapsed() >= TLS_HANDSHAKE_TIMEOUT,
            Self::Tls {
                state: TlsConnectionState::Established,
                ..
            } => false,
        }
    }

    /// Determines if the transport requires socket write readiness (`POLLOUT`).
    /// In TLS, this consults `session.wants_write()`.
    pub fn wants_write(&self) -> bool {
        match self {
            Self::Plain(_) => false,
            Self::Tls { session, .. } => session.wants_write(),
        }
    }

    /// Progresses the TLS handshake or packet exchange.
    /// Returns `true` when TLS is fully established and ready for application data.
    pub fn progress_tls(&mut self) -> io::Result<bool> {
        match self {
            Self::Plain(_) => Ok(true),
            Self::Tls {
                stream,
                session,
                state,
            } => {
                // 1. If session has outgoing TLS records (flights, server hello, etc.), write them to the socket.
                while session.wants_write() {
                    match session.write_tls(stream) {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e),
                    }
                }

                // 2. Read incoming encrypted TLS packets from socket into session buffer
                let mut read_any = false;
                loop {
                    match session.read_tls(stream) {
                        Ok(0) => {
                            // Peer closed connection
                            if session.is_handshaking() {
                                return Err(io::Error::new(
                                    io::ErrorKind::UnexpectedEof,
                                    "TLS peer closed connection during handshake",
                                ));
                            }
                            break;
                        }
                        Ok(_) => {
                            read_any = true;
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e),
                    }
                }

                // 3. Process new TLS packets
                if read_any {
                    let io_state = session.process_new_packets().map_err(|e| {
                        io::Error::new(io::ErrorKind::InvalidData, format!("TLS error: {}", e))
                    })?;
                    let _ = io_state;
                }

                // Flush any responses generated by packet processing
                while session.wants_write() {
                    match session.write_tls(stream) {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e),
                    }
                }

                // 4. Update handshake state
                if !session.is_handshaking() {
                    *state = TlsConnectionState::Established;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
        }
    }

    /// Reads decrypted application plaintext bytes.
    pub fn read_plaintext(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.read(buf),
            Self::Tls {
                stream,
                session,
                state,
            } => {
                // Read encrypted bytes from the wire into session if needed
                loop {
                    match session.read_tls(stream) {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e),
                    }
                }

                let _ = session.process_new_packets().map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("TLS processing error: {}", e),
                    )
                })?;

                if !session.is_handshaking() {
                    *state = TlsConnectionState::Established;
                }

                // Drain decrypted application plaintext
                match session.reader().read(buf) {
                    Ok(n) => Ok(n),
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "TLS plaintext would block",
                    )),
                    Err(e) => Err(e),
                }
            }
        }
    }

    /// Writes plaintext by encrypting into TLS records and flushing to the socket.
    pub fn write_plaintext(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.write(buf),
            Self::Tls {
                stream, session, ..
            } => {
                let written = session
                    .writer()
                    .write(buf)
                    .map_err(|e| io::Error::other(format!("TLS write error: {}", e)))?;

                // Flush TLS ciphertext to socket
                while session.wants_write() {
                    match session.write_tls(stream) {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e),
                    }
                }

                Ok(written)
            }
        }
    }

    /// Flushes any buffered ciphertext to the underlying network socket.
    pub fn flush_ciphertext(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(stream) => stream.flush(),
            Self::Tls {
                stream, session, ..
            } => {
                while session.wants_write() {
                    match session.write_tls(stream) {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                        Err(e) => return Err(e),
                    }
                }
                stream.flush()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::generate_simple_self_signed;

    #[test]
    fn test_tls_acceptor_config_generation() {
        let subject_alt_names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
        let cert =
            generate_simple_self_signed(subject_alt_names).expect("Failed to generate test cert");
        let cert_pem = cert.cert.pem();
        let key_pem = cert.key_pair.serialize_pem();

        let config = TlsAcceptorConfig::from_pem_bytes(cert_pem.as_bytes(), key_pem.as_bytes())
            .expect("Failed to create TlsAcceptorConfig");

        assert_eq!(
            config.server_config.alpn_protocols,
            vec![b"http/1.1".to_vec()]
        );
        assert_eq!(
            config.server_config.max_fragment_size,
            Some(MAX_RECORD_FRAGMENT_BYTES)
        );
    }

    #[test]
    fn test_anti_slowloris_handshake_timeout() {
        let subject_alt_names = vec!["localhost".to_string()];
        let cert = generate_simple_self_signed(subject_alt_names).unwrap();
        let config = TlsAcceptorConfig::from_pem_bytes(
            cert.cert.pem().as_bytes(),
            cert.key_pair.serialize_pem().as_bytes(),
        )
        .unwrap();

        // Simulate a connected plain stream that gets wrapped in TLS
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client_stream = TcpStream::connect(addr).unwrap();
        let (server_stream, _) = listener.accept().unwrap();

        let transport = TransportStream::tls(server_stream, config.server_config).unwrap();
        assert!(transport.is_handshaking());
        assert!(!transport.has_handshake_timed_out());
        drop(client_stream);
    }
}
