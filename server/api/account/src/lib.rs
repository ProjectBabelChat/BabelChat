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
    let api6 = api.clone();
    router = router.route("/account/change_password", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api6;
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Account>::change_password(&api, __body, user_id, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api7 = api.clone();
    router = router.route("/account/sign_out_others", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api7;
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Account>::sign_out_others(&api, user_id, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api8 = api.clone();
    router = router.route("/account/create_signup_code", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api8;
        let __raw = <T as crate::types::Account>::create_signup_code(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api9 = api.clone();
    router = router.route("/account/watch_revocations", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api9;
        let after = __query.get("after").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(after) = after else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let wait_ms = __query.get("wait_ms").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(wait_ms) = wait_ms else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Account>::watch_revocations(&api, after, wait_ms, ).await;
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