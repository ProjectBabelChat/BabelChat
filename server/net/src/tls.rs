//! Mutual TLS between services.
//!
//! Every service holds a certificate from the cluster's own CA (see
//! [`crate::pki`]) naming what it is: a URI `babel://<role>/<name>`, e.g.
//! `babel://message-cell/message-cell-0`.
//!
//! - Servers accept only clients with such a certificate and learn who is
//!   calling from it ([`identity_of`]).
//! - Clients check that the server holds the certificate of the service they
//!   meant to reach. Host names are not checked: the identity is what
//!   matters, so certificates do not need to know addresses, and a service
//!   can move to another host (or behind another WireGuard address) without
//!   a new certificate.

use std::{path::Path, sync::Arc};

use anyhow::{Context, anyhow, bail};
use babel_common::{Identity, Role};
use rustls::{
    CertificateError, DigitallySignedStruct, RootCertStore, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature},
    pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime, pem::PemObject},
    server::{ParsedCertificate, WebPkiClientVerifier},
};

const SCHEME: &str = "babel://";

/// The URI naming `identity` in its certificate.
pub fn identity_uri(identity: &Identity) -> String {
    format!("{SCHEME}{}/{}", identity.role, identity.name)
}

fn parse_identity_uri(uri: &str) -> Option<Identity> {
    let (role, name) = uri.strip_prefix(SCHEME)?.split_once('/')?;
    Some(Identity {
        role: Role::parse(role)?,
        name: name.to_string(),
    })
}

/// The identity a certificate was issued for, if it has exactly one.
pub fn identity_of(cert: &CertificateDer<'_>) -> Option<Identity> {
    let (_, parsed) = x509_parser::parse_x509_certificate(cert.as_ref()).ok()?;
    let names = parsed.subject_alternative_name().ok()??;
    let mut identities = names
        .value
        .general_names
        .iter()
        .filter_map(|name| match name {
            x509_parser::extensions::GeneralName::URI(uri) => parse_identity_uri(uri),
            _ => None,
        });
    let identity = identities.next()?;
    identities.next().is_none().then_some(identity)
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// A service's certificate, its key and the cluster CA, read from PEM files.
pub struct Credentials {
    pub identity: Identity,
    /// When the certificate stops being accepted (Unix seconds): its own
    /// end, or its CA's if sooner.
    pub expires: i64,
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
    roots: Arc<RootCertStore>,
}

impl Clone for Credentials {
    fn clone(&self) -> Self {
        Self {
            identity: self.identity.clone(),
            expires: self.expires,
            chain: self.chain.clone(),
            key: self.key.clone_key(),
            roots: self.roots.clone(),
        }
    }
}

impl Credentials {
    pub fn load(ca: &Path, cert: &Path, key: &Path) -> anyhow::Result<Self> {
        let mut roots = RootCertStore::empty();
        let mut cas = Vec::new();
        for cert in CertificateDer::pem_file_iter(ca)
            .with_context(|| format!("reading {}", ca.display()))?
        {
            let cert = cert?;
            cas.push(cert.clone());
            roots.add(cert)?;
        }
        let chain = CertificateDer::pem_file_iter(cert)
            .with_context(|| format!("reading {}", cert.display()))?
            .collect::<Result<Vec<_>, _>>()?;
        let key = PrivateKeyDer::from_pem_file(key)
            .with_context(|| format!("reading {}", key.display()))?;
        let identity = chain
            .first()
            .and_then(identity_of)
            .ok_or_else(|| anyhow!("{} names no BabelChat service", cert.display()))?;
        let expires =
            expiry(&chain[0], &cas).with_context(|| format!("reading {}", cert.display()))?;
        Ok(Self {
            identity,
            expires,
            chain,
            key,
            roots: Arc::new(roots),
        })
    }

    /// Fails unless these credentials are `expected`'s.
    pub fn expect(&self, expected: &Identity) -> anyhow::Result<()> {
        if self.identity != *expected {
            bail!("the certificate is for {}, not {}", self.identity, expected);
        }
        Ok(())
    }

    /// TLS for serving: clients must present a certificate from the CA.
    pub fn server_config(&self) -> anyhow::Result<Arc<rustls::ServerConfig>> {
        let provider = provider();
        let verifier =
            WebPkiClientVerifier::builder_with_provider(self.roots.clone(), provider.clone())
                .build()?;
        let mut config = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_client_cert_verifier(verifier)
            .with_single_cert(self.chain.clone(), self.key.clone_key())?;
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    }

    /// TLS for calling `peer`: presents these credentials and accepts only
    /// `peer`'s certificate.
    pub fn client_config(&self, peer: &Identity) -> anyhow::Result<rustls::ClientConfig> {
        let provider = provider();
        let mut config = rustls::ClientConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(PeerVerifier {
                roots: self.roots.clone(),
                expected: peer.clone(),
                provider,
            }))
            .with_client_auth_cert(self.chain.clone(), self.key.clone_key())?;
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Ok(config)
    }

    /// An HTTP client for calling `peer`.
    pub fn http_client(&self, peer: &Identity) -> anyhow::Result<reqwest::Client> {
        Ok(reqwest::Client::builder()
            .use_preconfigured_tls(self.client_config(peer)?)
            .connect_timeout(std::time::Duration::from_secs(5))
            // Long polls wait up to 30 seconds.
            .timeout(std::time::Duration::from_secs(60))
            .no_proxy()
            .build()?)
    }
}

/// When `cert` stops being accepted: its end, or its CA's (among `cas`)
/// if sooner.
fn expiry(cert: &CertificateDer<'_>, cas: &[CertificateDer<'_>]) -> anyhow::Result<i64> {
    let parse = |der: &[u8]| {
        x509_parser::parse_x509_certificate(der)
            .map(|(_, cert)| {
                (
                    cert.subject().as_raw().to_vec(),
                    cert.issuer().as_raw().to_vec(),
                    cert.validity().not_after.timestamp(),
                )
            })
            .map_err(|error| anyhow!("not a certificate: {error}"))
    };
    let (_, issuer, mut expires) = parse(cert)?;
    for ca in cas {
        let (subject, _, ca_expires) = parse(ca)?;
        if subject == issuer {
            expires = expires.min(ca_expires);
        }
    }
    Ok(expires)
}

/// Verifies a server certificate against the cluster CA and the identity
/// the caller expects, instead of a host name.
#[derive(Debug)]
struct PeerVerifier {
    roots: Arc<RootCertStore>,
    expected: Identity,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for PeerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let cert = ParsedCertificate::try_from(end_entity)?;
        rustls::client::verify_server_cert_signed_by_trust_anchor(
            &cert,
            &self.roots,
            intermediates,
            now,
            self.provider.signature_verification_algorithms.all,
        )?;
        if identity_of(end_entity).as_ref() != Some(&self.expected) {
            return Err(rustls::Error::InvalidCertificate(
                CertificateError::NotValidForName,
            ));
        }
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
            &self.provider.signature_verification_algorithms,
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
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_uris_round_trip() {
        let identity = Identity {
            role: Role::TranslationCell,
            name: "translation-cell-3".into(),
        };
        let uri = identity_uri(&identity);
        assert_eq!(uri, "babel://translation-cell/translation-cell-3");
        assert_eq!(parse_identity_uri(&uri), Some(identity));
        assert_eq!(parse_identity_uri("babel://wizard/merlin"), None);
        assert_eq!(parse_identity_uri("https://edge/edge-0"), None);
    }
}
