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

impl Pki {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, file: &str) -> PathBuf {
        self.dir.join(file)
    }

    /// Creates the CA and the token key pair, unless they exist. Returns
    /// whether anything was created.
    pub fn init(&self) -> anyhow::Result<bool> {
        fs::create_dir_all(&self.dir)?;
        let mut created = false;
        if !self.path("ca.crt").exists() {
            let mut params = CertificateParams::new(Vec::<String>::new())?;
            params
                .distinguished_name
                .push(DnType::CommonName, "BabelChat cluster CA");
            params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
            params.key_usages = vec![
                KeyUsagePurpose::KeyCertSign,
                KeyUsagePurpose::CrlSign,
                KeyUsagePurpose::DigitalSignature,
            ];
            validity(&mut params, CA_YEARS);
            let key = KeyPair::generate()?;
            let cert = params.self_signed(&key)?;
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

    /// Issues a certificate for `identity`, replacing an older one.
    pub fn issue(&self, identity: &Identity) -> anyhow::Result<()> {
        if !crate::config::valid_name(&identity.name) {
            bail!(
                "{:?}: names are lowercase letters, digits and dashes",
                identity.name
            );
        }
        let ca_key_pem = fs::read_to_string(self.path("ca.key"))
            .context("no CA here; run `babel-pki init` first")?;
        let ca_key = KeyPair::from_pem(&ca_key_pem)?;
        let ca_params =
            CertificateParams::from_ca_cert_pem(&fs::read_to_string(self.path("ca.crt"))?)?;
        let ca = ca_params.self_signed(&ca_key)?;

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
}
