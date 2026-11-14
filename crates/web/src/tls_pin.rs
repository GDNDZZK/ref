//! Certificate fingerprint and pinned TLS verifier shared by the `/api/ping` probe and the
//! integration tests (`#[path]`). No `crate::` imports so integration tests can include it.
#![cfg_attr(test, allow(dead_code))]

use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::WebPkiSupportedAlgorithms;
use rustls::crypto::aws_lc_rs::default_provider;
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::pem::PemObject as _;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, Error as RustlsError, SignatureScheme,
};
use sha2::{Digest as _, Sha256};

/// Colon-separated uppercase SHA-256 hex digest of a DER certificate (the form browsers show).
pub fn cert_fingerprint(der: &[u8]) -> String {
    let digest = Sha256::digest(der);
    let mut out = String::with_capacity(digest.len() * 3);
    for (i, byte) in digest.iter().enumerate() {
        if i > 0 {
            out.push(':');
        }
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02X}");
    }
    out
}

pub fn load_cert_der(path: &Path) -> anyhow::Result<CertificateDer<'static>> {
    CertificateDer::from_pem_file(path)
        .with_context(|| format!("cannot read certificate {}", path.display()))
}

/// A `rustls::ClientConfig` that trusts only the exact leaf certificate `expected_der` (no CA
/// store, no hostname check).
pub fn pinned_client_config(expected_der: CertificateDer<'static>) -> ClientConfig {
    let provider = Arc::new(default_provider());
    let verifier = Arc::new(PinnedCertVerifier {
        expected_der,
        algorithms: provider.signature_verification_algorithms,
    });
    ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("default TLS protocol versions")
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth()
}

#[derive(Debug)]
struct PinnedCertVerifier {
    expected_der: CertificateDer<'static>,
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        if end_entity.as_ref() == self.expected_der.as_ref() {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(RustlsError::InvalidCertificate(
                CertificateError::UnknownIssuer,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}
