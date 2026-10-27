//! A development stand-in for the Account service.
//!
//! The production service is closed (billing, Discord linking); this stub
//! implements the same API (types/account.tyml) well enough to run BabelChat
//! locally: registration, login, JWTs and personal settings.
//!
//! Tokens are signed with Ed25519 (EdDSA). Only this service holds the
//! private key; Edges verify with the public key, so no Edge can mint
//! tokens. A split deployment hands the key in (`pki/token.key`); otherwise
//! the service makes one and keeps it in its store.
//!
//! Signing out other devices (or changing the password) revokes every token
//! of the user issued until then. Edges follow the stream of revocations
//! (`watch_revocations`) and refuse revoked tokens, still without calling
//! this service per request. A revocation matters only as long as the
//! tokens it revokes would have lived, so the stream keeps a token lifetime.

use std::sync::Arc;

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use async_trait::async_trait;
use babel_api_account::types::{
    AccountError, Credentials, NewSignupCode, PasswordChange, Registration, Revocation,
    RevocationBatch, SignupCode, Token, User, UserSettings,
};
use babel_common::{LogSignals, now_ms, poll_timeout, random_id};
use babel_store::{Batch, Key, KeyReader, KvStore, Scan, StoreError, Table};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use parking_lot::Mutex;
use rand::Rng;
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde::{Deserialize, Serialize};

const USERS: Table = "users";
const NAMES: Table = "names";
const META: Table = "meta";
const SIGNUP_CODES: Table = "signup_codes";
const REVOCATIONS: Table = "revocations";

pub const TABLES: &[Table] = &[USERS, NAMES, META, SIGNUP_CODES, REVOCATIONS];

const TOKEN_LIFETIME_MS: i64 = 7 * 24 * 60 * 60 * 1000;
const TOKEN_KEY: &[u8] = b"token_key";
const MAX_REVOCATIONS: usize = 1000;

/// Who may create an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Signups {
    /// Anyone.
    #[default]
    Open,
    /// People with a sign-up code from an operator.
    Invite,
    /// Nobody.
    Closed,
}

/// The claims of a BabelChat token. Edge APIs decode the same shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub name: String,
    pub iat: i64,
    pub exp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserRecord {
    id: String,
    name: String,
    password_hash: String,
    settings: UserSettings,
    /// Tokens issued before this (ms) are revoked; new ones are issued at
    /// it or later.
    #[serde(default)]
    tokens_after: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SignupRecord {
    uses_left: i64,
    expires_at: i64,
}

fn storage(error: StoreError) -> AccountError {
    tracing::error!(%error, "account storage failed");
    AccountError::Unavailable
}

#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("the token key is not an Ed25519 key in PKCS#8")]
    BadKey,
}

pub struct Account {
    store: Arc<dyn KvStore>,
    signing_key: EncodingKey,
    public_key: Vec<u8>,
    signups: Signups,
    /// Serializes read-check-write changes (names, sign-up codes,
    /// revocations).
    writes: Mutex<()>,
    revocation_version: Mutex<i64>,
    signals: LogSignals<()>,
}

impl Account {
    /// Opens the service. `token_key` is the Ed25519 private key tokens are
    /// signed with (PKCS#8, DER); without one the service uses the key in
    /// its store, made on first open.
    pub fn open(store: Arc<dyn KvStore>, token_key: Option<Vec<u8>>) -> Result<Self, OpenError> {
        let pkcs8 = match token_key {
            Some(key) => key,
            None => match store.get(META, TOKEN_KEY)? {
                Some(key) => key,
                None => {
                    let key = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
                        .map_err(|_| OpenError::BadKey)?;
                    let mut batch = Batch::new();
                    batch.put(META, TOKEN_KEY.to_vec(), key.as_ref().to_vec());
                    store.write(batch)?;
                    key.as_ref().to_vec()
                }
            },
        };
        let pair = Ed25519KeyPair::from_pkcs8(&pkcs8).map_err(|_| OpenError::BadKey)?;
        let revocation_version = store
            .scan(Scan::prefix(REVOCATIONS, &[]).reverse().limit(1))?
            .first()
            .and_then(|(key, _)| KeyReader::new(key).u64())
            .unwrap_or(0) as i64;
        Ok(Self {
            store,
            signing_key: EncodingKey::from_ed_der(&pkcs8),
            public_key: pair.public_key().as_ref().to_vec(),
            signups: Signups::Open,
            writes: Mutex::new(()),
            revocation_version: Mutex::new(revocation_version),
            signals: LogSignals::new(),
        })
    }

    /// Sets who may create an account (anyone, by default).
    pub fn with_signups(mut self, signups: Signups) -> Self {
        self.signups = signups;
        self
    }

    /// The raw Ed25519 public key Edges verify tokens with
    /// (`jsonwebtoken::DecodingKey::from_ed_der`).
    pub fn token_public_key(&self) -> &[u8] {
        &self.public_key
    }

    fn user(&self, user_id: &str) -> Result<UserRecord, AccountError> {
        self.store
            .get_value(USERS, &Key::new().str(user_id).build())
            .map_err(storage)?
            .ok_or(AccountError::NotFound)
    }

    fn verify_password(user: &UserRecord, password: &str) -> Result<(), AccountError> {
        let hash =
            PasswordHash::new(&user.password_hash).map_err(|_| AccountError::InvalidCredentials)?;
        Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .map_err(|_| AccountError::InvalidCredentials)
    }

    /// Takes one use of a sign-up code, if sign-ups need one.
    fn admit(&self, code: Option<&str>, batch: &mut Batch) -> Result<(), AccountError> {
        match self.signups {
            Signups::Open => Ok(()),
            Signups::Closed => Err(AccountError::SignupsClosed),
            Signups::Invite => {
                let key = Key::new()
                    .str(code.ok_or(AccountError::InvalidSignupCode)?.trim())
                    .build();
                let mut record: SignupRecord = self
                    .store
                    .get_value(SIGNUP_CODES, &key)
                    .map_err(storage)?
                    .ok_or(AccountError::InvalidSignupCode)?;
                if record.uses_left < 1 || record.expires_at <= now_ms() {
                    return Err(AccountError::InvalidSignupCode);
                }
                record.uses_left -= 1;
                if record.uses_left == 0 {
                    batch.delete(SIGNUP_CODES, key);
                } else {
                    batch.put_value(SIGNUP_CODES, key, &record);
                }
                Ok(())
            }
        }
    }

    /// Revokes every token of `user` issued until now, records it in the
    /// stream Edges follow, and issues a new token. Called with `writes`
    /// held; `batch` carries the caller's other changes to the user.
    fn revoke(&self, mut user: UserRecord, mut batch: Batch) -> Result<Token, AccountError> {
        // Tokens carry their issue time in seconds: revoke this whole
        // second, and issue the new token at the next one. Tokens issued
        // since an earlier revocation carry its time, possibly this second
        // or the next: revoke past it too.
        let not_before = ((now_ms() / 1000 + 1) * 1000).max(user.tokens_after + 1000);
        user.tokens_after = not_before;
        let mut version = self.revocation_version.lock();
        let revocation = Revocation {
            user_id: user.id.clone(),
            not_before,
            version: *version + 1,
        };
        batch.put_value(USERS, Key::new().str(&user.id).build(), &user);
        batch.put_value(
            REVOCATIONS,
            Key::new().u64(revocation.version as u64).build(),
            &revocation,
        );
        // Revocations older than a token's lifetime revoke nothing that has
        // not expired anyway.
        let old: Vec<(Vec<u8>, Revocation)> = self
            .store
            .scan_values(Scan::prefix(REVOCATIONS, &[]).limit(100))
            .map_err(storage)?;
        for (key, old) in old {
            if old.not_before > now_ms() - TOKEN_LIFETIME_MS {
                break;
            }
            batch.delete(REVOCATIONS, key);
        }
        self.store.write(batch).map_err(storage)?;
        *version = revocation.version;
        drop(version);
        self.signals.publish(&(), revocation.version);
        self.issue(&user)
    }

    fn issue(&self, user: &UserRecord) -> Result<Token, AccountError> {
        let iat = (now_ms() / 1000).max(user.tokens_after / 1000);
        let exp = iat + TOKEN_LIFETIME_MS / 1000;
        let claims = Claims {
            sub: user.id.clone(),
            name: user.name.clone(),
            iat,
            exp,
        };
        let jwt = jsonwebtoken::encode(&Header::new(Algorithm::EdDSA), &claims, &self.signing_key)
            .map_err(|_| AccountError::InvalidRequest)?;
        Ok(Token {
            jwt,
            user_id: user.id.clone(),
            name: user.name.clone(),
            expires_at: exp * 1000,
        })
    }
}

fn name_key(name: &str) -> Vec<u8> {
    Key::new().str(&name.to_lowercase()).build()
}

#[async_trait]
impl babel_api_account::types::Account for Account {
    async fn register(&self, body: Registration) -> Result<Token, AccountError> {
        let name = body.name.trim().to_string();
        let name_len = name.chars().count();
        // `@name` mentions a user, so a name is one word with no `@`.
        let one_word = name
            .chars()
            .all(|c| !c.is_whitespace() && !c.is_control() && c != '@');
        if !(2..=32).contains(&name_len)
            || !one_word
            || body.password.chars().count() < MIN_PASSWORD
        {
            return Err(AccountError::InvalidRequest);
        }
        babel_lang::Normalizer::new()
            .base(&body.locale)
            .map_err(|_| AccountError::InvalidRequest)?;
        let password_hash = hash_password(&body.password)?;

        let _writes = self.writes.lock();
        let mut batch = Batch::new();
        self.admit(body.signup_code.as_deref(), &mut batch)?;
        if self
            .store
            .get(NAMES, &name_key(&name))
            .map_err(storage)?
            .is_some()
        {
            return Err(AccountError::NameTaken);
        }
        let user = UserRecord {
            id: random_id(),
            name: name.clone(),
            password_hash,
            settings: UserSettings {
                locale: body.locale,
                translate_outside_set: true,
                show_original: false,
            },
            tokens_after: 0,
        };
        batch.put_value(USERS, Key::new().str(&user.id).build(), &user);
        batch.put(NAMES, name_key(&name), user.id.clone().into_bytes());
        self.store.write(batch).map_err(storage)?;
        self.issue(&user)
    }

    async fn login(&self, body: Credentials) -> Result<Token, AccountError> {
        let user_id = self
            .store
            .get(NAMES, &name_key(body.name.trim()))
            .map_err(storage)?
            .ok_or(AccountError::InvalidCredentials)?;
        let user = self
            .user(&String::from_utf8_lossy(&user_id))
            .map_err(|_| AccountError::InvalidCredentials)?;
        Self::verify_password(&user, &body.password)?;
        self.issue(&user)
    }

    async fn get_user(&self, user_id: String) -> Result<User, AccountError> {
        let user = self.user(&user_id)?;
        Ok(User {
            id: user.id,
            name: user.name,
        })
    }

    async fn get_settings(&self, user_id: String) -> Result<UserSettings, AccountError> {
        Ok(self.user(&user_id)?.settings)
    }

    async fn put_settings(
        &self,
        body: UserSettings,
        user_id: String,
    ) -> Result<UserSettings, AccountError> {
        babel_lang::Normalizer::new()
            .base(&body.locale)
            .map_err(|_| AccountError::InvalidRequest)?;
        let _writes = self.writes.lock();
        let mut user = self.user(&user_id)?;
        user.settings = body.clone();
        let mut batch = Batch::new();
        batch.put_value(USERS, Key::new().str(&user.id).build(), &user);
        self.store.write(batch).map_err(storage)?;
        Ok(body)
    }

    async fn change_password(
        &self,
        body: PasswordChange,
        user_id: String,
    ) -> Result<Token, AccountError> {
        if body.new_password.chars().count() < MIN_PASSWORD {
            return Err(AccountError::InvalidRequest);
        }
        let password_hash = hash_password(&body.new_password)?;
        let _writes = self.writes.lock();
        let mut user = self.user(&user_id)?;
        Self::verify_password(&user, &body.password)?;
        user.password_hash = password_hash;
        self.revoke(user, Batch::new())
    }

    async fn sign_out_others(&self, user_id: String) -> Result<Token, AccountError> {
        let _writes = self.writes.lock();
        let user = self.user(&user_id)?;
        self.revoke(user, Batch::new())
    }

    async fn create_signup_code(&self, body: NewSignupCode) -> Result<SignupCode, AccountError> {
        if body.uses < 1 || body.hours < 1 {
            return Err(AccountError::InvalidRequest);
        }
        let code = SignupCode {
            code: random_id(),
            uses_left: body.uses,
            expires_at: now_ms() + body.hours.saturating_mul(60 * 60 * 1000),
        };
        let mut batch = Batch::new();
        batch.put_value(
            SIGNUP_CODES,
            Key::new().str(&code.code).build(),
            &SignupRecord {
                uses_left: code.uses_left,
                expires_at: code.expires_at,
            },
        );
        self.store.write(batch).map_err(storage)?;
        Ok(code)
    }

    async fn watch_revocations(
        &self,
        after: i64,
        wait_ms: i64,
    ) -> Result<RevocationBatch, AccountError> {
        let head = *self.revocation_version.lock();
        if after > head {
            // A follower ahead of the stream (this service was restored
            // from a backup): it carries on from here.
            return Ok(RevocationBatch {
                revocations: Vec::new(),
                version: head,
            });
        }
        if after == head {
            self.signals
                .wait_past(&(), after, poll_timeout(wait_ms))
                .await;
        }
        let start = Key::new().u64(after.max(0) as u64).build();
        let revocations: Vec<Revocation> = self
            .store
            .scan_values(
                Scan::prefix(REVOCATIONS, &[])
                    .after(&start)
                    .limit(MAX_REVOCATIONS),
            )
            .map_err(storage)?
            .into_iter()
            .map(|(_, revocation)| revocation)
            .collect();
        Ok(RevocationBatch {
            version: revocations.last().map(|r| r.version).unwrap_or(after),
            revocations,
        })
    }
}

const MIN_PASSWORD: usize = 8;

fn hash_password(password: &str) -> Result<String, AccountError> {
    let salt_bytes: [u8; 16] = rand::rng().random();
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|_| AccountError::InvalidRequest)?;
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| AccountError::InvalidRequest)?
        .to_string())
}

#[cfg(test)]
mod tests {
    use babel_api_account::types::Account as _;
    use babel_store::MemStore;

    use super::*;

    #[tokio::test]
    async fn register_and_login() {
        let account = Account::open(Arc::new(MemStore::new()), None).unwrap();
        let token = account
            .register(Registration {
                name: "Alice".into(),
                password: "correct horse".into(),
                locale: "ja-JP".into(),
                signup_code: None,
            })
            .await
            .unwrap();

        let duplicate = account
            .register(Registration {
                name: "alice".into(),
                password: "another password".into(),
                locale: "en".into(),
                signup_code: None,
            })
            .await;
        assert_eq!(duplicate.unwrap_err(), AccountError::NameTaken);

        let login = account
            .login(Credentials {
                name: "alice".into(),
                password: "correct horse".into(),
            })
            .await
            .unwrap();
        assert_eq!(login.user_id, token.user_id);

        let wrong = account
            .login(Credentials {
                name: "alice".into(),
                password: "wrong password".into(),
            })
            .await;
        assert_eq!(wrong.unwrap_err(), AccountError::InvalidCredentials);

        let settings = account.get_settings(token.user_id).await.unwrap();
        assert_eq!(settings.locale, "ja-JP");
    }

    #[tokio::test]
    async fn names_are_one_mentionable_word() {
        let account = Account::open(Arc::new(MemStore::new()), None).unwrap();
        let register = |name: &str| {
            account.register(Registration {
                name: name.into(),
                password: "correct horse".into(),
                locale: "ja-JP".into(),
                signup_code: None,
            })
        };
        assert!(register("さくら").await.is_ok());
        assert!(register("bob.smith_2").await.is_ok());
        for name in ["bob smith", "bob@home", "x"] {
            assert_eq!(
                register(name).await.unwrap_err(),
                AccountError::InvalidRequest
            );
        }
    }

    #[tokio::test]
    async fn tokens_verify_with_the_public_key_only() {
        let store: Arc<dyn KvStore> = Arc::new(MemStore::new());
        let account = Account::open(store.clone(), None).unwrap();
        let token = account
            .register(Registration {
                name: "alice".into(),
                password: "correct horse".into(),
                locale: "ja-JP".into(),
                signup_code: None,
            })
            .await
            .unwrap();

        let validation = jsonwebtoken::Validation::new(Algorithm::EdDSA);
        let verify = |public_key: &[u8]| {
            jsonwebtoken::decode::<Claims>(
                &token.jwt,
                &jsonwebtoken::DecodingKey::from_ed_der(public_key),
                &validation,
            )
        };
        assert_eq!(
            verify(account.token_public_key()).unwrap().claims.sub,
            token.user_id
        );

        // The key stays across restarts; another key does not verify.
        let reopened = Account::open(store, None).unwrap();
        assert_eq!(reopened.token_public_key(), account.token_public_key());
        let stranger = Account::open(Arc::new(MemStore::new()), None).unwrap();
        assert!(verify(stranger.token_public_key()).is_err());
    }

    fn registration(name: &str, signup_code: Option<&str>) -> Registration {
        Registration {
            name: name.into(),
            password: "correct horse".into(),
            locale: "ja-JP".into(),
            signup_code: signup_code.map(Into::into),
        }
    }

    #[tokio::test]
    async fn sign_ups_by_invitation() {
        let account = Account::open(Arc::new(MemStore::new()), None)
            .unwrap()
            .with_signups(Signups::Invite);
        assert_eq!(
            account
                .register(registration("alice", None))
                .await
                .unwrap_err(),
            AccountError::InvalidSignupCode
        );
        let code = account
            .create_signup_code(NewSignupCode { uses: 2, hours: 1 })
            .await
            .unwrap();
        assert!(
            account
                .register(registration("alice", Some(&code.code)))
                .await
                .is_ok()
        );
        // A refused sign-up keeps its use.
        assert_eq!(
            account
                .register(registration("alice", Some(&code.code)))
                .await
                .unwrap_err(),
            AccountError::NameTaken
        );
        assert!(
            account
                .register(registration("bob", Some(&code.code)))
                .await
                .is_ok()
        );
        assert_eq!(
            account
                .register(registration("carol", Some(&code.code)))
                .await
                .unwrap_err(),
            AccountError::InvalidSignupCode
        );

        let closed = Account::open(Arc::new(MemStore::new()), None)
            .unwrap()
            .with_signups(Signups::Closed);
        assert_eq!(
            closed
                .register(registration("alice", None))
                .await
                .unwrap_err(),
            AccountError::SignupsClosed
        );
    }

    #[tokio::test]
    async fn signing_out_others_revokes_older_tokens() {
        let store: Arc<dyn KvStore> = Arc::new(MemStore::new());
        let account = Account::open(store.clone(), None).unwrap();
        let first = account.register(registration("alice", None)).await.unwrap();
        let start = account.watch_revocations(0, 0).await.unwrap();
        assert!(start.revocations.is_empty());

        let watch = account.watch_revocations(start.version, 5_000);
        let (batch, fresh) = tokio::join!(watch, account.sign_out_others(first.user_id.clone()));
        let fresh = fresh.unwrap();
        let batch = batch.unwrap();
        assert_eq!(batch.revocations.len(), 1);
        let revocation = &batch.revocations[0];
        assert_eq!(revocation.user_id, first.user_id);

        // Tokens issued from now on, on this device or another, are on the
        // right side of the revocation.
        let issued_at = |token: &Token| {
            let mut validation = jsonwebtoken::Validation::new(Algorithm::EdDSA);
            validation.validate_exp = false;
            jsonwebtoken::decode::<Claims>(
                &token.jwt,
                &jsonwebtoken::DecodingKey::from_ed_der(account.token_public_key()),
                &validation,
            )
            .unwrap()
            .claims
            .iat * 1000
        };
        assert!(issued_at(&first) < revocation.not_before);
        assert!(issued_at(&fresh) >= revocation.not_before);
        let other = account
            .login(Credentials {
                name: "alice".into(),
                password: "correct horse".into(),
            })
            .await
            .unwrap();
        assert!(issued_at(&other) >= revocation.not_before);

        // Changing the password revokes too, and needs the old one.
        let wrong = account
            .change_password(
                PasswordChange {
                    password: "wrong password".into(),
                    new_password: "battery staple".into(),
                },
                first.user_id.clone(),
            )
            .await;
        assert_eq!(wrong.unwrap_err(), AccountError::InvalidCredentials);
        account
            .change_password(
                PasswordChange {
                    password: "correct horse".into(),
                    new_password: "battery staple".into(),
                },
                first.user_id.clone(),
            )
            .await
            .unwrap();
        let login = |password: &str| {
            account.login(Credentials {
                name: "alice".into(),
                password: password.into(),
            })
        };
        assert!(login("correct horse").await.is_err());
        assert!(login("battery staple").await.is_ok());
        // Within a second of the first revocation, the second still covers
        // the token issued between them.
        let second = account.watch_revocations(batch.version, 0).await.unwrap();
        assert!(issued_at(&other) < second.revocations[0].not_before);

        // A restarted service carries the stream on.
        let reopened = Account::open(store, None).unwrap();
        let all = reopened.watch_revocations(0, 0).await.unwrap();
        assert_eq!(all.revocations.len(), 2);
        assert_eq!(all.version, 2);
    }
}
