use std::sync::Arc;

use quinn::ClientConfig;

// Implementation of `ServerCertVerifier` that verifies everything as trustworthy.
pub struct SkipServerVerification;

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl rustls::client::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}

pub fn configure_client() -> ClientConfig {
    let crypto = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();

    ClientConfig::new(Arc::new(crypto))
}

pub fn generate_self_signed_cert() -> (rustls::Certificate, rustls::PrivateKey) {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
        .expect("Generate simple self signed cert");
    let key = rustls::PrivateKey(cert.serialize_private_key_der());
    (
        rustls::Certificate(cert.serialize_der().expect("Can generate certs")),
        key,
    )
}
