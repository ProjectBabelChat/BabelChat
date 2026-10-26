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
pub fn router<T: crate::types::Directory>(api: T) -> Router {
    let api = Arc::new(api);
    let mut router = Router::new();

    let api1 = api.clone();
    router = router.route("/directory/create_guild", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api1;
        let __raw = <T as crate::types::Directory>::create_guild(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api2 = api.clone();
    router = router.route("/directory/get_placement", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api2;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Directory>::get_placement(&api, guild_id, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api3 = api.clone();
    router = router.route("/directory/watch_placements", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api3;
        let after = __query.get("after").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(after) = after else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let wait_ms = __query.get("wait_ms").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(wait_ms) = wait_ms else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Directory>::watch_placements(&api, after, wait_ms, ).await;
        let result = __raw;
        (StatusCode::OK, Json(result)).into_response()
    }));
    let api4 = api.clone();
    router = router.route("/directory/add_membership", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api4;
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Directory>::add_membership(&api, user_id, guild_id, ).await;
        let result = __raw;
        Response::builder().status(StatusCode::OK).body(Body::empty()).unwrap()
    }));
    let api5 = api.clone();
    router = router.route("/directory/remove_membership", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api5;
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Directory>::remove_membership(&api, user_id, guild_id, ).await;
        let result = __raw;
        Response::builder().status(StatusCode::OK).body(Body::empty()).unwrap()
    }));
    let api6 = api.clone();
    router = router.route("/directory/list_memberships", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api6;
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Directory>::list_memberships(&api, user_id, ).await;
        let result = __raw;
        (StatusCode::OK, Json(result)).into_response()
    }));

    router
}

/// Serves the generated Axum router on the specified address.
#[allow(unused)]
pub async fn serve<T: crate::types::Directory>(api: T, address: &str) -> Result<(), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, router(api)).await?;
    Ok(())
}