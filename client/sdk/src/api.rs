//! Signing in, and the generated Edge client behind one connection pool.

use std::{sync::Arc, time::Duration};

use babel_api_edge_client::types::{
    ApiError, Auth, Credentials, Gateway, Guilds, Me, Messages, PasswordChange, Registration,
    ThrowsOrOtherError, Token,
};
use parking_lot::RwLock;

use crate::{DeviceTranslator, Session, SessionConfig};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Error {
    /// The server refused: `not_found`, `name_taken`, `rate_limited`...
    #[error("the server refused: {0:?}")]
    Api(ApiError),
    /// The token was rejected or has expired; sign in again.
    #[error("signed out: the token was rejected")]
    Unauthorized,
    /// The server could not be reached, or answered nonsense.
    #[error("connection failed: {0}")]
    Transport(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl From<ThrowsOrOtherError<ApiError>> for Error {
    fn from(error: ThrowsOrOtherError<ApiError>) -> Self {
        match error {
            ThrowsOrOtherError::Throws(error) => Error::Api(error),
            ThrowsOrOtherError::Other(error)
                if error.status() == Some(reqwest::StatusCode::UNAUTHORIZED) =>
            {
                Error::Unauthorized
            }
            ThrowsOrOtherError::Other(error) => Error::Transport(error.to_string()),
        }
    }
}

/// Long polls wait up to 25 s on the server; give them room.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// A signed-in user on one server. Cheap to clone.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

struct Inner {
    server: String,
    http: reqwest::Client,
    user_id: String,
    user_name: String,
    /// Replaced when the user signs other devices out or changes the
    /// password.
    token: RwLock<Token>,
    /// Held while the token is being replaced, so that a session refused
    /// with the old one waits for the new one instead of signing out.
    renewing: tokio::sync::Mutex<()>,
}

fn http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .map_err(|error| Error::Transport(error.to_string()))
}

fn server_url(server: &str) -> String {
    server.trim_end_matches('/').to_string()
}

impl Client {
    /// Creates an account. `locale` is a BCP 47 tag such as "ja-JP": the
    /// language this user reads and usually writes. Servers that take new
    /// accounts by invitation only want a `signup_code`
    /// (`invalid_signup_code` without one).
    pub async fn register(
        server: &str,
        name: &str,
        password: &str,
        locale: &str,
        signup_code: Option<&str>,
    ) -> Result<Self> {
        let server = server_url(server);
        let http = http()?;
        let token = Auth {
            url: server.clone(),
            client: http.clone(),
        }
        .register(Registration {
            name: name.to_string(),
            password: password.to_string(),
            locale: locale.to_string(),
            signup_code: signup_code
                .map(str::trim)
                .filter(|code| !code.is_empty())
                .map(str::to_string),
        })
        .await?;
        Ok(Self::new(server, http, token))
    }

    pub async fn login(server: &str, name: &str, password: &str) -> Result<Self> {
        let server = server_url(server);
        let http = http()?;
        let token = Auth {
            url: server.clone(),
            client: http.clone(),
        }
        .login(Credentials {
            name: name.to_string(),
            password: password.to_string(),
        })
        .await?;
        Ok(Self::new(server, http, token))
    }

    /// Resumes with a token saved earlier. It is checked on first use.
    pub fn with_token(server: &str, token: Token) -> Result<Self> {
        Ok(Self::new(server_url(server), http()?, token))
    }

    fn new(server: String, http: reqwest::Client, token: Token) -> Self {
        Self {
            inner: Arc::new(Inner {
                server,
                http,
                user_id: token.user_id.clone(),
                user_name: token.name.clone(),
                token: RwLock::new(token),
                renewing: tokio::sync::Mutex::new(()),
            }),
        }
    }

    pub fn server(&self) -> &str {
        &self.inner.server
    }

    /// Save this to sign in again without a password. It changes when the
    /// user signs other devices out or changes the password: save it again
    /// then.
    pub fn token(&self) -> Token {
        self.inner.token.read().clone()
    }

    pub fn user_id(&self) -> &str {
        &self.inner.user_id
    }

    pub fn user_name(&self) -> &str {
        &self.inner.user_name
    }

    /// Signs every other device of the user out: their tokens stop working
    /// and their sessions end. This client carries on with a new token
    /// (returned, to be saved); its sessions reconnect by themselves.
    pub async fn sign_out_others(&self) -> Result<Token> {
        let _renewing = self.inner.renewing.lock().await;
        let token = self.me().sign_out_others(self.jwt()).await?;
        *self.inner.token.write() = token.clone();
        Ok(token)
    }

    /// Changes the password, which signs the other devices out like
    /// [`Self::sign_out_others`]. A wrong `password` is
    /// `Api(InvalidCredentials)`.
    pub async fn change_password(&self, password: &str, new_password: &str) -> Result<Token> {
        let _renewing = self.inner.renewing.lock().await;
        let token = self
            .me()
            .change_password(
                self.jwt(),
                PasswordChange {
                    password: password.to_string(),
                    new_password: new_password.to_string(),
                },
            )
            .await?;
        *self.inner.token.write() = token.clone();
        Ok(token)
    }

    /// After a request was refused with `jwt`: waits for a token being
    /// replaced, and tells whether there is a newer one to retry with.
    pub(crate) async fn renewed_since(&self, jwt: &str) -> bool {
        let _renewing = self.inner.renewing.lock().await;
        self.jwt() != jwt
    }

    /// Opens a live session. `translator` translates on this device when
    /// the server does not translate into the reader's language.
    pub async fn connect(&self, translator: Arc<dyn DeviceTranslator>) -> Result<Session> {
        Session::connect(self.clone(), translator, SessionConfig::default()).await
    }

    pub async fn connect_with(
        &self,
        translator: Arc<dyn DeviceTranslator>,
        config: SessionConfig,
    ) -> Result<Session> {
        Session::connect(self.clone(), translator, config).await
    }

    pub(crate) fn jwt(&self) -> String {
        self.inner.token.read().jwt.clone()
    }

    pub(crate) fn me(&self) -> Me {
        Me {
            url: self.inner.server.clone(),
            client: self.inner.http.clone(),
        }
    }

    pub(crate) fn guilds(&self) -> Guilds {
        Guilds {
            url: self.inner.server.clone(),
            client: self.inner.http.clone(),
        }
    }

    pub(crate) fn messages(&self) -> Messages {
        Messages {
            url: self.inner.server.clone(),
            client: self.inner.http.clone(),
        }
    }

    pub(crate) fn gateway(&self) -> Gateway {
        Gateway {
            url: self.inner.server.clone(),
            client: self.inner.http.clone(),
        }
    }
}
