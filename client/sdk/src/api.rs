//! Signing in, and the generated Edge client behind one connection pool.

use std::{sync::Arc, time::Duration};

use babel_api_edge_client::types::{
    ApiError, Auth, Credentials, Gateway, Guilds, Me, Messages, Registration, ThrowsOrOtherError,
    Token,
};

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
    token: Token,
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
    /// language this user reads and usually writes.
    pub async fn register(server: &str, name: &str, password: &str, locale: &str) -> Result<Self> {
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
                token,
            }),
        }
    }

    pub fn server(&self) -> &str {
        &self.inner.server
    }

    /// Save this to sign in again without a password.
    pub fn token(&self) -> &Token {
        &self.inner.token
    }

    pub fn user_id(&self) -> &str {
        &self.inner.token.user_id
    }

    pub fn user_name(&self) -> &str {
        &self.inner.token.name
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
        self.inner.token.jwt.clone()
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
