//! Serving an internal API over mutual TLS.
//!
//! Each connection is authenticated by its client certificate; each request
//! is then checked against the service's [`Policy`] (who may call which
//! route) and runs with the caller set ([`babel_common::caller`]), so a
//! service can tell, for instance, which tenant is calling.

use std::{convert::Infallible, sync::Arc};

use axum::{Router, body::Body, http::StatusCode, response::IntoResponse};
use babel_common::{Identity, Role, with_caller};
use hyper::{Request, body::Incoming};
use hyper_util::{
    rt::{TokioExecutor, TokioIo},
    server::conn::auto,
};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tower::ServiceExt;

use crate::tls::{Credentials, identity_of};

/// Who may call which route (`/message_cell/send_message`) of a service.
#[derive(Clone)]
pub struct Policy(Arc<dyn Fn(&Identity, &str) -> bool + Send + Sync>);

impl Policy {
    pub fn new(allows: impl Fn(&Identity, &str) -> bool + Send + Sync + 'static) -> Self {
        Self(Arc::new(allows))
    }

    /// Any route, for callers with one of `roles`.
    pub fn roles(roles: &'static [Role]) -> Self {
        Self::new(|caller, _| roles.contains(&caller.role))
    }

    pub fn allows(&self, caller: &Identity, path: &str) -> bool {
        (self.0)(caller, path)
    }
}

/// Serves `router` on `listener` until the process ends.
pub async fn serve(
    router: Router,
    listener: TcpListener,
    credentials: &Credentials,
    policy: Policy,
) -> anyhow::Result<()> {
    let acceptor = TlsAcceptor::from(credentials.server_config()?);
    loop {
        let (stream, address) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                // Out of file descriptors, for instance; keep serving.
                tracing::warn!(%error, "accepting a connection failed");
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                continue;
            }
        };
        let acceptor = acceptor.clone();
        let router = router.clone();
        let policy = policy.clone();
        tokio::spawn(async move {
            let stream = match acceptor.accept(stream).await {
                Ok(stream) => stream,
                Err(error) => {
                    tracing::debug!(%error, %address, "TLS handshake failed");
                    return;
                }
            };
            let caller = stream
                .get_ref()
                .1
                .peer_certificates()
                .and_then(|chain| chain.first())
                .and_then(identity_of);
            let Some(caller) = caller else {
                tracing::warn!(%address, "a client certificate named no service");
                return;
            };
            let service = hyper::service::service_fn(move |request: Request<Incoming>| {
                let router = router.clone();
                let caller = caller.clone();
                let allowed = policy.allows(&caller, request.uri().path());
                async move {
                    if !allowed {
                        tracing::warn!(%caller, path = request.uri().path(), "refused a call");
                        return Ok::<_, Infallible>(StatusCode::FORBIDDEN.into_response());
                    }
                    let request = request.map(Body::new);
                    let response = with_caller(caller, router.oneshot(request)).await;
                    Ok(response.unwrap_or_else(|never| match never {}))
                }
            });
            if let Err(error) = auto::Builder::new(TokioExecutor::new())
                .serve_connection_with_upgrades(TokioIo::new(stream), service)
                .await
            {
                tracing::debug!(%error, %address, "connection ended");
            }
        });
    }
}
