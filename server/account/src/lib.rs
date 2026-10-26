//! A development stand-in for the Account service.
//!
//! The production service is closed (billing, Discord linking); this stub
//! implements the same API (types/account.tyml) well enough to run BabelChat
//! locally: registration, login, JWTs and personal settings.
//!
//! Tokens are signed with HS256 and a per-installation secret that the
//! all-in-one server hands to its Edge. Production signs with a private key
//! and Edges verify with the public key, so no Edge can mint tokens.

use std::sync::Arc;

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use async_trait::async_trait;
use babel_api_account::types::{
    AccountError, Credentials, Registration, Token, User, UserSettings,
};
use babel_common::{now_ms, random_id};
use babel_store::{Batch, Key, KvStore, StoreError, Table};
use jsonwebtoken::{EncodingKey, Header};
use rand::Rng;
use serde::{Deserialize, Serialize};

const USERS: Table = "users";
const NAMES: Table = "names";
const META: Table = "meta";

pub const TABLES: &[Table] = &[USERS, NAMES, META];

const TOKEN_LIFETIME_MS: i64 = 7 * 24 * 60 * 60 * 1000;

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
    AccountError::InvalidRequest
}

pub struct Account {
    store: Arc<dyn KvStore>,
    secret: Vec<u8>,
}

impl Account {
    pub fn open(store: Arc<dyn KvStore>) -> Result<Self, StoreError> {
        let key = b"jwt_secret".to_vec();
        let secret = match store.get(META, &key)? {
            Some(secret) => secret,
            None => {
                let secret: [u8; 32] = rand::rng().random();
                let mut batch = Batch::new();
                batch.put(META, key, secret.to_vec());
                store.write(batch)?;
                secret.to_vec()
            }
        };
        Ok(Self { store, secret })
    }

    /// The HS256 secret Edges verify tokens with (development only).
    pub fn jwt_secret(&self) -> &[u8] {
        &self.secret
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
        let jwt = jsonwebtoken::encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(&self.secret),
        )
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
        if !(2..=32).contains(&name_len) || body.password.chars().count() < 8 {
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
        let account = Account::open(Arc::new(MemStore::new())).unwrap();
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
}
