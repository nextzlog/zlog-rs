use std::fmt;
use std::sync::{Arc, Mutex};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{ring as provider, verify_tls13_signature, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, SignatureScheme};

use crate::crypto::{hex, unhex_vec, Fingerprint};

const NAME: &str = "peer.invalid";

#[derive(Clone, PartialEq, Eq)]
pub struct Identity {
    cert: Vec<u8>,
    key: Vec<u8>,
}

impl Identity {
    pub fn generate() -> Result<Identity, String> {
        let c = rcgen::generate_simple_self_signed(vec![NAME.into()]).map_err(|e| e.to_string())?;
        Ok(Identity {
            cert: c.cert.der().to_vec(),
            key: c.signing_key.serialize_der(),
        })
    }

    pub fn from_der(cert: Vec<u8>, key: Vec<u8>) -> Identity {
        Identity { cert, key }
    }

    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint::of(&self.cert)
    }

    pub fn to_text(&self) -> String {
        format!("{}:{}", hex(&self.cert, ""), hex(&self.key, ""))
    }

    pub fn from_text(text: &str) -> Option<Identity> {
        let (c, k) = text.trim().split_once(':')?;
        Some(Identity::from_der(unhex_vec(c)?, unhex_vec(k)?))
    }

    fn chain(&self) -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
        (
            vec![CertificateDer::from(self.cert.clone())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.clone())),
        )
    }

    pub(crate) fn server_config(&self) -> Result<rustls::ServerConfig, rustls::Error> {
        let provider = provider::default_provider();
        let any = Arc::new(AnyClient(provider.signature_verification_algorithms));
        let (chain, key) = self.chain();
        let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(provider))
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_client_cert_verifier(any)
            .with_single_cert(chain, key)?;
        config.send_tls13_tickets = 0;
        Ok(config)
    }

    pub(crate) fn client_config(
        &self,
        pin: Option<Fingerprint>,
    ) -> Result<(Arc<Pinning>, rustls::ClientConfig), rustls::Error> {
        let provider = provider::default_provider();
        let pinning = Arc::new(Pinning {
            pin,
            seen: Mutex::new(None),
            algs: provider.signature_verification_algorithms,
        });
        let (chain, key) = self.chain();
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(provider))
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .dangerous()
            .with_custom_certificate_verifier(pinning.clone())
            .with_client_auth_cert(chain, key)?;
        Ok((pinning, config))
    }
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Identity({})", self.fingerprint())
    }
}

pub(crate) fn server_name() -> ServerName<'static> {
    ServerName::try_from(NAME).expect("a valid name")
}

#[derive(Debug)]
struct AnyClient(WebPkiSupportedAlgorithms);

impl ClientCertVerifier for AnyClient {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2 is not supported".into()))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.0)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_schemes()
    }
}

#[derive(Debug)]
pub(crate) struct Pinning {
    pin: Option<Fingerprint>,
    pub seen: Mutex<Option<Fingerprint>>,
    algs: WebPkiSupportedAlgorithms,
}

impl Pinning {
    pub fn seen(&self) -> Option<Fingerprint> {
        *self.seen.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl ServerCertVerifier for Pinning {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let found = Fingerprint::of(end_entity);
        *self.seen.lock().unwrap_or_else(|e| e.into_inner()) = Some(found);
        match self.pin {
            Some(p) if p != found => Err(rustls::Error::General("fingerprint mismatch".into())),
            _ => Ok(ServerCertVerified::assertion()),
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Err(rustls::Error::General("TLS 1.2 is not supported".into()))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algs.supported_schemes()
    }
}

pub(crate) fn pairing_code(
    app: &str,
    exporter: impl FnOnce(&mut [u8; 8], &[u8]) -> bool,
) -> Option<String> {
    let mut out = [0u8; 8];
    let label = format!("EXPORTER-{app}-pairing");
    exporter(&mut out, label.as_bytes())
        .then(|| format!("{:06}", u64::from_be_bytes(out) % 1_000_000))
}
