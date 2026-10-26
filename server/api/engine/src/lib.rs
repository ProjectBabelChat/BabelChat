#![allow(unused)]
#![allow(clippy::all)]
use std::sync::Arc;
use std::collections::HashMap;
use serde_json;
use axum::{
    Json, Router,
    body::Body,
    extract::Query,
    http::StatusCode,
    response::{IntoResponse, Response},
};

pub mod types;

/// Builds the generated Axum router without binding a network listener.
#[allow(unused)]
pub fn router<T: crate::types::Engine>(api: T) -> Router {
    let api = Arc::new(api);
    let mut router = Router::new();

    let api1 = api.clone();
    router = router.route("/engine/translate", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api1;
        let __raw = <T as crate::types::Engine>::translate(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api2 = api.clone();
    router = router.route("/engine/summarize", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api2;
        let __raw = <T as crate::types::Engine>::summarize(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api3 = api.clone();
    router = router.route("/engine/info", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api3;
        let __raw = <T as crate::types::Engine>::info(&api, ).await;
        let result = __raw;
        (StatusCode::OK, Json(result)).into_response()
    }));

    router
}

/// Serves the generated Axum router on the specified address.
#[allow(unused)]
pub async fn serve<T: crate::types::Engine>(api: T, address: &str) -> Result<(), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, router(api)).await?;
    Ok(())
}