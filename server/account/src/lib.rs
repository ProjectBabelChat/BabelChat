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

use std::sync::Arc;

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use async_trait::async_trait;
use babel_api_account::types::{
    AccountError, Credentials, Registration, Token, User, UserSettings,
};
use babel_common::{now_ms, random_id};
use babel_store::{Batch, Key, KvStore, StoreError, Table};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use rand::Rng;
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde::{Deserialize, Serialize};

const USERS: Table = "users";
const NAMES: Table = "names";
const META: Table = "meta";

pub const TABLES: &[Table] = &[USERS, NAMES, META];

const TOKEN_LIFETIME_MS: i64 = 7 * 24 * 60 * 60 * 1000;
const TOKEN_KEY: &[u8] = b"token_key";

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
        Ok(Self {
            store,
            signing_key: EncodingKey::from_ed_der(&pkcs8),
            public_key: pair.public_key().as_ref().to_vec(),
        })
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

    fn issue(&self, user: &UserRecord) -> Result<Token, AccountError> {
        let iat = now_ms() / 1000;
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
        if !(2..=32).contains(&name_len) || !one_word || body.password.chars().count() < 8 {
            return Err(AccountError::InvalidRequest);
        }
        babel_lang::Normalizer::new()
            .base(&body.locale)
            .map_err(|_| AccountError::InvalidRequest)?;
        if self
            .store
            .get(NAMES, &name_key(&name))
            .map_err(storage)?
            .is_some()
        {
            return Err(AccountError::NameTaken);
        }

        let salt_bytes: [u8; 16] = rand::rng().random();
        let salt = SaltString::encode_b64(&salt_bytes).map_err(|_| AccountError::InvalidRequest)?;
        let password_hash = Argon2::default()
            .hash_password(body.password.as_bytes(), &salt)
            .map_err(|_| AccountError::InvalidRequest)?
            .to_string();
        let user = UserRecord {
            id: random_id(),
            name: name.clone(),
            password_hash,
            settings: UserSettings {
                locale: body.locale,
                translate_outside_set: true,
                show_original: false,
            },
        };

        let mut batch = Batch::new();
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
        let hash =
            PasswordHash::new(&user.password_hash).map_err(|_| AccountError::InvalidCredentials)?;
        Argon2::default()
            .verify_password(body.password.as_bytes(), &hash)
            .map_err(|_| AccountError::InvalidCredentials)?;
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
        let mut user = self.user(&user_id)?;
        user.settings = body.clone();
        let mut batch = Batch::new();
        batch.put_value(USERS, Key::new().str(&user.id).build(), &user);
        self.store.write(batch).map_err(storage)?;
        Ok(body)
    }
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
            })
            .await
            .unwrap();

        let duplicate = account
            .register(Registration {
                name: "alice".into(),
                password: "another password".into(),
                locale: "en".into(),
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
}
