//! The SDK runs on Tokio and GPUI on its own executor. A Tokio runtime
//! lives beside the app; SDK calls are spawned onto it and awaited from
//! GPUI tasks.

use std::{future::Future, path::PathBuf, sync::Arc};

use babel_sdk::{ApiError, DeviceTranslator, Error, Transport};
use gpui::{App, Global};

use crate::i18n::{Strings, fill};

/// The runtime SDK futures run on.
pub struct Tokio(pub tokio::runtime::Handle);

impl Global for Tokio {}

/// Runs `future` on the Tokio runtime; the returned future can be awaited
/// from any executor.
pub fn spawn<T: Send + 'static>(
    cx: &App,
    future: impl Future<Output = T> + Send + 'static,
) -> impl Future<Output = T> + 'static {
    let task = cx.global::<Tokio>().0.spawn(future);
    async move { task.await.expect("a background task panicked") }
}

/// How the app was started: the profile to use and how sessions connect.
pub struct Config {
    pub server: String,
    pub profile: String,
    pub dir: PathBuf,
    pub cache: bool,
    pub transport: Transport,
    pub translator: Arc<dyn DeviceTranslator>,
    pub system_notifications: bool,
}

impl Global for Config {}

/// An SDK error, said the way a person would want to hear it.
pub fn describe(error: &Error, t: &Strings) -> String {
    match error {
        Error::Api(error) => match error {
            ApiError::NotFound => t.not_found,
            ApiError::Unauthorized => t.not_allowed,
            ApiError::InvalidRequest => t.invalid_request,
            ApiError::InvalidLanguage => t.invalid_language,
            ApiError::LimitExceeded => t.limit_exceeded,
            ApiError::NameTaken => t.name_taken,
            ApiError::InvalidCredentials => t.wrong_password,
            ApiError::RateLimited => t.rate_limited,
            ApiError::Unavailable => t.unavailable,
            ApiError::SignupsClosed => t.signups_closed,
            ApiError::InvalidSignupCode => t.invalid_signup_code,
        }
        .to_string(),
        Error::Unauthorized => t.session_expired.to_string(),
        Error::Transport(reason) => fill(t.cannot_connect, &[reason]),
    }
}
