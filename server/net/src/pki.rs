//! The cluster's own certificate authority.
//!
//! ```text
//! pki/ca.crt, ca.key           the CA (keep ca.key off the servers)
//! pki/<name>.crt, <name>.key   one certificate per service, admin or tenant
//! pki/token.key, token.pub     the key pair user tokens are signed with
//! ```
//!
//! Service certificates name the service (`babel://<role>/<name>`, see
//! [`crate::tls`]) and serve as both server and client certificates.
//! User tokens are signed with Ed25519: only the Account service holds
//! `token.key`; Edges verify with `token.pub`, so an Edge cannot mint tokens.
//!
//! Both are replaced without stopping anything, each step deployed before
//! the next. The CA: [`Pki::new_ca`] (`ca.crt` holds the new CA, which signs
//! from then on, then the old one: everything trusts both), [`Pki::renew`]
//! (certificates the old one signed are issued again), [`Pki::drop_old_cas`]
//! (only the new one is trusted). The token key: [`Pki::new_token_key`]
//! (`token.next.key`, whose public key `token.pub` adds: Edges accept
//! both), [`Pki::use_new_token_key`] (the Account service signs with it),
//! and once the old one's tokens expired, [`Pki::drop_old_token_keys`].

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use babel_common::Identity;
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose, SanType,
};
use time::{Duration, OffsetDateTime};

const CA_YEARS: i64 = 10;
const SERVICE_YEARS: i64 = 2;

pub struct Pki {
    pub dir: PathBuf,
}

/// Writes `contents` readable by the owner only (keys) or by everyone.
fn write(path: &Path, contents: &str, private: bool) -> anyhow::Result<()> {
    fs::write(path, contents).with_context(|| format!("writing {}", path.display()))?;
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = private;
    Ok(())
}

fn validity(params: &mut CertificateParams, years: i64) {
    let now = OffsetDateTime::now_utc();
    // A little slack for clocks that run behind.
    params.not_before = now - Duration::hours(1);
    params.not_after = now + Duration::days(365 * years);
}

/// The private key in a PEM file, as DER (PKCS#8 for `token.key`).
pub fn read_private_key(path: &Path) -> anyhow::Result<Vec<u8>> {
    use rustls::pki_types::{PrivateKeyDer, pem::PemObject};
    let key = PrivateKeyDer::from_pem_file(path)
        .with_context(|| format!("reading {}", path.display()))?;
    Ok(key.secret_der().to_vec())
}

/// The PEM blocks of `text`, each with its BEGIN and END lines.
pub fn pem_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut block: Option<String> = None;
    for line in text.lines() {
        if line.starts_with("-----BEGIN ") {
            block = Some(String::new());
        }
        if let Some(block) = &mut block {
            block.push_str(line);
            block.push('\n');
        }
        if line.starts_with("-----END ")
            && let Some(block) = block.take()
        {
            blocks.push(block);
        }
    }
    blocks
}

/// A new CA, named after the moment it was made: certificates tell which
/// CA signed them by its name.
fn make_ca() -> anyhow::Result<(KeyPair, rcgen::Certificate)> {
    let now = OffsetDateTime::now_utc();
    let mut params = CertificateParams::new(Vec::<String>::new())?;
    params.distinguished_name.push(
        DnType::CommonName,
        format!(
            "BabelChat cluster CA {} {:02}:{:02}:{:02}",
            now.date(),
            now.hour(),
            now.minute(),
            now.second()
        ),
    );
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    validity(&mut params, CA_YEARS);
    let key = KeyPair::generate()?;
    let cert = params.self_signed(&key)?;
    Ok((key, cert))
}

/// What [`Pki::status`] tells of a certificate.
pub struct CertificateStatus {
    /// Its file, without `.crt`.
    pub name: String,
    /// Whom it was issued for; `None` for a CA.
    pub identity: Option<Identity>,
    /// Signed by the CA that signs now (for a CA: it is that CA).
    pub current: bool,
    pub not_after: OffsetDateTime,
}

fn parse(der: &[u8]) -> anyhow::Result<x509_parser::certificate::X509Certificate<'_>> {
    Ok(x509_parser::parse_x509_certificate(der)
        .map_err(|error| anyhow::anyhow!("not a certificate: {error}"))?
        .1)
}

fn first_der(pem: &str) -> anyhow::Result<Vec<u8>> {
    use rustls::pki_types::{CertificateDer, pem::PemObject};
    Ok(CertificateDer::from_pem_slice(pem.as_bytes())?.to_vec())
}

impl Pki {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, file: &str) -> PathBuf {
        self.dir.join(file)
    }

    fn read(&self, file: &str) -> anyhow::Result<String> {
        fs::read_to_string(self.path(file))
            .with_context(|| format!("reading {}", self.path(file).display()))
    }

    /// Creates the CA and the token key pair, unless they exist. Returns
    /// whether anything was created.
    pub fn init(&self) -> anyhow::Result<bool> {
        fs::create_dir_all(&self.dir)?;
        let mut created = false;
        if !self.path("ca.crt").exists() {
            let (key, cert) = make_ca()?;
            write(&self.path("ca.key"), &key.serialize_pem(), true)?;
            write(&self.path("ca.crt"), &cert.pem(), false)?;
            created = true;
        }
        if !self.path("token.key").exists() {
            let key = KeyPair::generate_for(&rcgen::PKCS_ED25519)?;
            write(&self.path("token.key"), &key.serialize_pem(), true)?;
            write(&self.path("token.pub"), &key.public_key_pem(), false)?;
            created = true;
        }
        Ok(created)
    }

    /// The CA that signs: `ca.key`, and the first certificate of `ca.crt`
    /// (the others are older CAs, still trusted).
    fn signing_ca(&self) -> anyhow::Result<(KeyPair, rcgen::Certificate, Vec<u8>)> {
        let ca_key = KeyPair::from_pem(
            &fs::read_to_string(self.path("ca.key"))
                .context("no CA here; run `babel-pki init` first")?,
        )?;
        let pem = self.read("ca.crt")?;
        let der = first_der(&pem)?;
        let parsed = parse(&der)?;
        if parsed.public_key().raw != ca_key.public_key_der().as_slice() {
            bail!("ca.key is not the key of the first certificate in ca.crt");
        }
        let subject = parsed.subject().as_raw().to_vec();
        let ca = CertificateParams::from_ca_cert_pem(&pem)?.self_signed(&ca_key)?;
        Ok((ca_key, ca, subject))
    }

    /// Issues a certificate for `identity`, replacing an older one.
    pub fn issue(&self, identity: &Identity) -> anyhow::Result<()> {
        if !crate::config::valid_name(&identity.name) {
            bail!(
                "{:?}: names are lowercase letters, digits and dashes",
                identity.name
            );
        }
        let (ca_key, ca, _) = self.signing_ca()?;

        let mut params = CertificateParams::new(Vec::<String>::new())?;
        params
            .distinguished_name
            .push(DnType::CommonName, identity.to_string());
        params.subject_alt_names =
            vec![SanType::URI(crate::tls::identity_uri(identity).try_into()?)];
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ServerAuth,
            ExtendedKeyUsagePurpose::ClientAuth,
        ];
        params.use_authority_key_identifier_extension = true;
        validity(&mut params, SERVICE_YEARS);
        let key = KeyPair::generate()?;
        let cert = params.signed_by(&key, &ca, &ca_key)?;
        write(
            &self.path(&format!("{}.key", identity.name)),
            &key.serialize_pem(),
            true,
        )?;
        write(
            &self.path(&format!("{}.crt", identity.name)),
            &cert.pem(),
            false,
        )?;
        Ok(())
    }

    /// Every CA trusted, then every certificate issued here.
    pub fn status(&self) -> anyhow::Result<Vec<CertificateStatus>> {
        let (_, _, signing) = self.signing_ca()?;
        let mut all = Vec::new();
        for (index, block) in pem_blocks(&self.read("ca.crt")?).iter().enumerate() {
            let der = first_der(block)?;
            let cert = parse(&der)?;
            all.push(CertificateStatus {
                name: if index == 0 { "ca" } else { "ca (old)" }.into(),
                identity: None,
                current: index == 0,
                not_after: cert.validity().not_after.to_datetime(),
            });
        }
        let mut issued = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let path = entry?.path();
            let Some(name) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".crt"))
            else {
                continue;
            };
            if name == "ca" {
                continue;
            }
            let der = first_der(&fs::read_to_string(&path)?)?;
            let cert = parse(&der)?;
            issued.push(CertificateStatus {
                name: name.to_string(),
                identity: crate::tls::identity_of(&der.clone().into()),
                current: cert.issuer().as_raw() == signing.as_slice(),
                not_after: cert.validity().not_after.to_datetime(),
            });
        }
        issued.sort_by(|a, b| a.name.cmp(&b.name));
        all.extend(issued);
        Ok(all)
    }

    /// Issues again every certificate here that expires within `within`,
    /// or that a CA other than the signing one signed. Returns their
    /// identities.
    pub fn renew(&self, within: Duration) -> anyhow::Result<Vec<Identity>> {
        let soon = OffsetDateTime::now_utc() + within;
        let mut renewed = Vec::new();
        for status in self.status()? {
            let Some(identity) = status.identity else {
                continue;
            };
            if !status.current || status.not_after < soon {
                self.issue(&identity)?;
                renewed.push(identity);
            }
        }
        Ok(renewed)
    }

    /// Makes a new CA, which signs from now on; `ca.crt` lists it first and
    /// keeps trusting the old one.
    pub fn new_ca(&self) -> anyhow::Result<()> {
        let bundle = self.read("ca.crt")?;
        if pem_blocks(&bundle).len() > 1 {
            bail!("a new CA is being brought in already: `renew`, then `drop-old-cas`");
        }
        let (key, cert) = make_ca()?;
        write(&self.path("ca.key"), &key.serialize_pem(), true)?;
        write(&self.path("ca.crt"), &(cert.pem() + &bundle), false)?;
        Ok(())
    }

    /// Trusts only the signing CA. Returns whether any other was dropped.
    pub fn drop_old_cas(&self) -> anyhow::Result<bool> {
        let blocks = pem_blocks(&self.read("ca.crt")?);
        if blocks.len() < 2 {
            return Ok(false);
        }
        if self
            .status()?
            .iter()
            .any(|status| status.identity.is_some() && !status.current)
        {
            bail!("some certificates here were signed by an old CA: `renew` first");
        }
        write(&self.path("ca.crt"), &blocks[0], false)?;
        Ok(true)
    }

    /// Makes `token.next.key` and adds its public key to `token.pub`.
    pub fn new_token_key(&self) -> anyhow::Result<()> {
        if self.path("token.next.key").exists() {
            bail!("a new token key is waiting already: `use-new-token-key`");
        }
        let key = KeyPair::generate_for(&rcgen::PKCS_ED25519)?;
        let public = self.read("token.pub")?;
        write(&self.path("token.next.key"), &key.serialize_pem(), true)?;
        write(
            &self.path("token.pub"),
            &(public + &key.public_key_pem()),
            false,
        )?;
        Ok(())
    }

    /// Makes `token.next.key` the key tokens are signed with.
    pub fn use_new_token_key(&self) -> anyhow::Result<()> {
        if !self.path("token.next.key").exists() {
            bail!("no new token key: `new-token-key` first");
        }
        fs::rename(self.path("token.next.key"), self.path("token.key"))?;
        Ok(())
    }

    /// Accepts only tokens of `token.key`. Returns whether any other key
    /// was dropped.
    pub fn drop_old_token_keys(&self) -> anyhow::Result<bool> {
        if self.path("token.next.key").exists() {
            bail!("a new token key is waiting: `use-new-token-key` first");
        }
        let public = KeyPair::from_pem(&self.read("token.key")?)?.public_key_pem();
        if pem_blocks(&self.read("token.pub")?) == pem_blocks(&public) {
            return Ok(false);
        }
        write(&self.path("token.pub"), &public, false)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use babel_common::Role;

    use super::*;
    use crate::tls::Credentials;

    #[test]
    fn issued_certificates_name_their_service() {
        let dir = tempfile::tempdir().unwrap();
        let pki = Pki::new(dir.path());
        assert!(pki.init().unwrap());
        assert!(!pki.init().unwrap());
        let identity = Identity {
            role: Role::MessageCell,
            name: "message-cell-0".into(),
        };
        pki.issue(&identity).unwrap();

        let credentials = Credentials::load(
            &dir.path().join("ca.crt"),
            &dir.path().join("message-cell-0.crt"),
            &dir.path().join("message-cell-0.key"),
        )
        .unwrap();
        assert_eq!(credentials.identity, identity);
        credentials.expect(&identity).unwrap();
        assert!(
            credentials
                .expect(&Identity {
                    role: Role::Edge,
                    name: "message-cell-0".into(),
                })
                .is_err()
        );
        assert!(
            pki.issue(&Identity {
                role: Role::Edge,
                name: "Edge 0".into(),
            })
            .is_err()
        );
    }

    /// Loads `name`'s credentials, trusting the CAs of `ca`.
    fn credentials(dir: &Path, ca: &str, name: &str) -> Credentials {
        Credentials::load(
            &dir.join(ca),
            &dir.join(format!("{name}.crt")),
            &dir.join(format!("{name}.key")),
        )
        .unwrap()
    }

    /// Whether `client` completes a TLS handshake with `server`.
    async fn handshake(client: &Credentials, server: &Credentials) -> bool {
        let (a, b) = tokio::io::duplex(1 << 16);
        let acceptor = tokio_rustls::TlsAcceptor::from(server.server_config().unwrap());
        let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(
            client.client_config(&server.identity).unwrap(),
        ));
        let name = rustls::pki_types::ServerName::try_from("babel").unwrap();
        let (accepted, connected) = tokio::join!(acceptor.accept(b), connector.connect(name, a));
        accepted.is_ok() && connected.is_ok()
    }

    #[tokio::test]
    async fn the_ca_is_replaced_without_a_moment_of_distrust() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        let pki = Pki::new(dir);
        pki.init().unwrap();
        let cell = Identity {
            role: Role::MessageCell,
            name: "message-cell-0".into(),
        };
        let edge = Identity {
            role: Role::Edge,
            name: "edge-0".into(),
        };
        pki.issue(&cell).unwrap();
        pki.issue(&edge).unwrap();
        assert!(pki.renew(Duration::days(30)).unwrap().is_empty());
        assert_eq!(pki.renew(Duration::days(800)).unwrap().len(), 2);
        // Both CAs trusted, by machines with their old certificates still.
        pki.new_ca().unwrap();
        assert!(pki.new_ca().is_err());
        assert!(pki.drop_old_cas().is_err());
        let old_edge = credentials(dir, "ca.crt", "edge-0");

        // New certificates from the new CA, reaching machines not given
        // theirs yet, both ways.
        assert_eq!(pki.renew(Duration::days(30)).unwrap().len(), 2);
        let new_cell = credentials(dir, "ca.crt", "message-cell-0");
        assert!(handshake(&old_edge, &new_cell).await);
        assert!(handshake(&new_cell, &old_edge).await);

        assert!(pki.drop_old_cas().unwrap());
        assert!(!pki.drop_old_cas().unwrap());
        assert_eq!(
            pki.status()
                .unwrap()
                .iter()
                .filter(|status| status.identity.is_none())
                .count(),
            1
        );
        let edge_now = credentials(dir, "ca.crt", "edge-0");
        let cell_now = credentials(dir, "ca.crt", "message-cell-0");
        assert!(handshake(&edge_now, &cell_now).await);
        // A certificate of the old CA is no longer accepted (`old_edge`
        // trusts both CAs: the server refuses it).
        assert!(!handshake(&old_edge, &cell_now).await);
    }

    #[test]
    fn the_token_key_is_replaced_with_both_accepted_meanwhile() {
        let dir = tempfile::tempdir().unwrap();
        let pki = Pki::new(dir.path());
        pki.init().unwrap();
        let read = |file: &str| fs::read_to_string(dir.path().join(file)).unwrap();
        let old = read("token.key");
        assert!(!pki.drop_old_token_keys().unwrap());
        assert!(pki.use_new_token_key().is_err());

        pki.new_token_key().unwrap();
        assert!(pki.new_token_key().is_err());
        assert_eq!(read("token.key"), old);
        assert_eq!(pem_blocks(&read("token.pub")).len(), 2);
        assert!(pki.drop_old_token_keys().is_err());

        pki.use_new_token_key().unwrap();
        assert_ne!(read("token.key"), old);
        assert!(pki.drop_old_token_keys().unwrap());
        assert_eq!(pem_blocks(&read("token.pub")).len(), 1);
        assert_eq!(
            read("token.pub"),
            KeyPair::from_pem(&read("token.key"))
                .unwrap()
                .public_key_pem()
        );
    }
}
