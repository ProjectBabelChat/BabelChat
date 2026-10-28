//! Metrics for Prometheus.
//!
//! Services record with the [`metrics`] macros; with `--metrics <address>`
//! they serve what they recorded at `GET /metrics` on that address, in plain
//! HTTP. Like `node_exporter`, it is meant for the private network
//! (WireGuard): the numbers are counts and timings, never content.
//!
//! [`track`] counts every call of a router and times it, by route and
//! status; services add their own measures next to their work.

use std::{sync::OnceLock, time::Duration};

use anyhow::Context;
use axum::{
    Router,
    extract::{MatchedPath, Request},
    middleware::Next,
    response::Response,
    routing::get,
};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};

/// Buckets of every `*_seconds` histogram: from a quick store call to a
/// slow translation.
const SECONDS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0,
];

static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Records the [`metrics`] macros of the whole process from now on.
fn handle() -> &'static PrometheusHandle {
    HANDLE.get_or_init(|| {
        PrometheusBuilder::new()
            .set_buckets_for_metric(Matcher::Suffix("_seconds".into()), SECONDS)
            .expect("the buckets are not empty")
            .install_recorder()
            .expect("no other metrics recorder is installed")
    })
}

/// Starts recording, and serves the numbers at `GET /metrics` on `address`
/// until the process ends. Needs a Tokio runtime.
pub fn serve(address: &str) -> anyhow::Result<()> {
    let listener = std::net::TcpListener::bind(address)
        .with_context(|| format!("listening for metrics on {address}"))?;
    listener.set_nonblocking(true)?;
    let listener = tokio::net::TcpListener::from_std(listener)?;
    let handle = handle().clone();
    tokio::spawn({
        let handle = handle.clone();
        async move {
            // Histograms keep their samples until drained; drain them even
            // when nobody scrapes.
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                handle.run_upkeep();
            }
        }
    });
    let router = Router::new().route("/metrics", get(move || async move { handle.render() }));
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, router).await {
            tracing::error!(%error, "serving metrics failed");
        }
    });
    tracing::info!(%address, "serving metrics");
    Ok(())
}

/// Counts and times the calls of a router's routes
/// (`router.route_layer(axum::middleware::from_fn(track))`):
/// `babel_requests_total` and `babel_request_duration_seconds` by route and
/// status. Long polls take as long as they wait.
pub async fn track(route: MatchedPath, request: Request, next: Next) -> Response {
    let started = std::time::Instant::now();
    let response = next.run(request).await;
    let route = route.as_str().to_string();
    let status = response.status().as_u16().to_string();
    metrics::histogram!("babel_request_duration_seconds", "route" => route.clone(), "status" => status.clone())
        .record(started.elapsed().as_secs_f64());
    metrics::counter!("babel_requests_total", "route" => route, "status" => status).increment(1);
    response
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn calls_are_counted_by_route() {
        let handle = handle();
        let router = Router::new()
            .route("/things/{id}", get(|| async { "ok" }))
            .route_layer(axum::middleware::from_fn(track));
        for id in ["a", "b"] {
            let request = Request::get(format!("/things/{id}"))
                .body(Body::empty())
                .unwrap();
            router.clone().oneshot(request).await.unwrap();
        }
        let text = handle.render();
        assert!(
            text.contains(r#"babel_requests_total{route="/things/{id}",status="200"} 2"#),
            "{text}"
        );
        assert!(text.contains("babel_request_duration_seconds_bucket"));
    }
}
