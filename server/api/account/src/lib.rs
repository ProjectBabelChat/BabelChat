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
pub fn router<T: crate::types::Account>(api: T) -> Router {
    let api = Arc::new(api);
    let mut router = Router::new();

    let api1 = api.clone();
    router = router.route("/account/register", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api1;
        let __raw = <T as crate::types::Account>::register(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api2 = api.clone();
    router = router.route("/account/login", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api2;
        let __raw = <T as crate::types::Account>::login(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api3 = api.clone();
    router = router.route("/account/get_user", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api3;
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Account>::get_user(&api, user_id, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api4 = api.clone();
    router = router.route("/account/get_settings", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api4;
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Account>::get_settings(&api, user_id, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api5 = api.clone();
    router = router.route("/account/put_settings", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api5;
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Account>::put_settings(&api, __body, user_id, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));

    router
}

/// Serves the generated Axum router on the specified address.
#[allow(unused)]
pub async fn serve<T: crate::types::Account>(api: T, address: &str) -> Result<(), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, router(api)).await?;
    Ok(())
}