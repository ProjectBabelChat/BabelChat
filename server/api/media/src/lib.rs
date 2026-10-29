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
pub fn router<T: crate::types::Media>(api: T) -> Router {
    let api = Arc::new(api);
    let mut router = Router::new();

    let api1 = api.clone();
    router = router.route("/media/get_blob", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api1;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let channel_id = __query.get("channel_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(channel_id) = channel_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let id = __query.get("id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(id) = id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Media>::get_blob(&api, guild_id, channel_id, id, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api2 = api.clone();
    router = router.route("/media/attach_blobs", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api2;
        let __raw = <T as crate::types::Media>::attach_blobs(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api3 = api.clone();
    router = router.route("/media/delete_blobs", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api3;
        let __raw = <T as crate::types::Media>::delete_blobs(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api4 = api.clone();
    router = router.route("/media/delete_channel", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api4;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let channel_id = __query.get("channel_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(channel_id) = channel_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Media>::delete_channel(&api, guild_id, channel_id, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api5 = api.clone();
    router = router.route("/media/delete_guild", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api5;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Media>::delete_guild(&api, guild_id, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api6 = api.clone();
    router = router.route("/media/list_blobs", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api6;
        let before = __query.get("before").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(before) = before else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let kind = __query.get("kind").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(kind) = kind else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let after = __query.get("after").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(after) = after else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let limit = __query.get("limit").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(limit) = limit else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Media>::list_blobs(&api, before, kind, after, limit, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api7 = api.clone();
    router = router.route("/media/get_usage", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api7;
        let __raw = <T as crate::types::Media>::get_usage(&api, ).await;
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
pub async fn serve<T: crate::types::Media>(api: T, address: &str) -> Result<(), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, router(api)).await?;
    Ok(())
}