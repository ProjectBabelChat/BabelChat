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
pub fn router<T: crate::types::TranslationCell>(api: T) -> Router {
    let api = Arc::new(api);
    let mut router = Router::new();

    let api1 = api.clone();
    router = router.route("/translation_cell/put_space", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api1;
        let __raw = <T as crate::types::TranslationCell>::put_space(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api2 = api.clone();
    router = router.route("/translation_cell/put_node", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api2;
        let __raw = <T as crate::types::TranslationCell>::put_node(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api3 = api.clone();
    router = router.route("/translation_cell/delete_space", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api3;
        let space = __query.get("space").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(space) = space else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::TranslationCell>::delete_space(&api, space, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api4 = api.clone();
    router = router.route("/translation_cell/get_policy", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api4;
        let space = __query.get("space").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(space) = space else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::TranslationCell>::get_policy(&api, space, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api5 = api.clone();
    router = router.route("/translation_cell/put_policy", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api5;
        let __raw = <T as crate::types::TranslationCell>::put_policy(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api6 = api.clone();
    router = router.route("/translation_cell/set_plan", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api6;
        let space = __query.get("space").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(space) = space else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let plan = __query.get("plan").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(plan) = plan else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::TranslationCell>::set_plan(&api, space, plan, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api7 = api.clone();
    router = router.route("/translation_cell/normalize", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api7;
        let space = __query.get("space").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(space) = space else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let tag = __query.get("tag").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(tag) = tag else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::TranslationCell>::normalize(&api, space, tag, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api8 = api.clone();
    router = router.route("/translation_cell/resolve_viewer", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api8;
        let space = __query.get("space").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(space) = space else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let locale = __query.get("locale").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(locale) = locale else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let translate_outside_set = __query.get("translate_outside_set").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(translate_outside_set) = translate_outside_set else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::TranslationCell>::resolve_viewer(&api, space, locale, translate_outside_set, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api9 = api.clone();
    router = router.route("/translation_cell/ingest", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api9;
        let __raw = <T as crate::types::TranslationCell>::ingest(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api10 = api.clone();
    router = router.route("/translation_cell/delete_message", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api10;
        let __raw = <T as crate::types::TranslationCell>::delete_message(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api11 = api.clone();
    router = router.route("/translation_cell/get_translations", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api11;
        let __raw = <T as crate::types::TranslationCell>::get_translations(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api12 = api.clone();
    router = router.route("/translation_cell/poll_events", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api12;
        let space = __query.get("space").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(space) = space else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let after = __query.get("after").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(after) = after else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let wait_ms = __query.get("wait_ms").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(wait_ms) = wait_ms else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::TranslationCell>::poll_events(&api, space, after, wait_ms, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api13 = api.clone();
    router = router.route("/translation_cell/get_usage", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api13;
        let space = __query.get("space").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(space) = space else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::TranslationCell>::get_usage(&api, space, ).await;
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
pub async fn serve<T: crate::types::TranslationCell>(api: T, address: &str) -> Result<(), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, router(api)).await?;
    Ok(())
}