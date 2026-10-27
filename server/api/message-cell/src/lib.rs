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
pub fn router<T: crate::types::MessageCell>(api: T) -> Router {
    let api = Arc::new(api);
    let mut router = Router::new();

    let api1 = api.clone();
    router = router.route("/message_cell/create_guild", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api1;
        let __raw = <T as crate::types::MessageCell>::create_guild(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api2 = api.clone();
    router = router.route("/message_cell/get_guild", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api2;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::MessageCell>::get_guild(&api, guild_id, user_id, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api3 = api.clone();
    router = router.route("/message_cell/join_guild", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api3;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let invite = __query.get("invite").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(invite) = invite else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let user_name = __query.get("user_name").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_name) = user_name else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::MessageCell>::join_guild(&api, guild_id, invite, user_id, user_name, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api4 = api.clone();
    router = router.route("/message_cell/create_invite", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api4;
        let __raw = <T as crate::types::MessageCell>::create_invite(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api5 = api.clone();
    router = router.route("/message_cell/revoke_invite", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api5;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let secret = __query.get("secret").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(secret) = secret else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::MessageCell>::revoke_invite(&api, guild_id, secret, user_id, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api6 = api.clone();
    router = router.route("/message_cell/get_invites", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api6;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::MessageCell>::get_invites(&api, guild_id, user_id, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api7 = api.clone();
    router = router.route("/message_cell/leave_guild", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api7;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::MessageCell>::leave_guild(&api, guild_id, user_id, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api8 = api.clone();
    router = router.route("/message_cell/remove_member", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api8;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let member_id = __query.get("member_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(member_id) = member_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let ban = __query.get("ban").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(ban) = ban else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::MessageCell>::remove_member(&api, guild_id, user_id, member_id, ban, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api9 = api.clone();
    router = router.route("/message_cell/unban_member", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api9;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let member_id = __query.get("member_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(member_id) = member_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::MessageCell>::unban_member(&api, guild_id, user_id, member_id, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api10 = api.clone();
    router = router.route("/message_cell/get_bans", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api10;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::MessageCell>::get_bans(&api, guild_id, user_id, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api11 = api.clone();
    router = router.route("/message_cell/ack", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api11;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let channel_id = __query.get("channel_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(channel_id) = channel_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let message_id = __query.get("message_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(message_id) = message_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::MessageCell>::ack(&api, guild_id, channel_id, user_id, message_id, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api12 = api.clone();
    router = router.route("/message_cell/put_notification_settings", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api12;
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::MessageCell>::put_notification_settings(&api, __body, user_id, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api13 = api.clone();
    router = router.route("/message_cell/check_permission", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api13;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let permission = __query.get("permission").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(permission) = permission else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::MessageCell>::check_permission(&api, guild_id, user_id, permission, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api14 = api.clone();
    router = router.route("/message_cell/create_channel", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api14;
        let __raw = <T as crate::types::MessageCell>::create_channel(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api15 = api.clone();
    router = router.route("/message_cell/send_message", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api15;
        let __raw = <T as crate::types::MessageCell>::send_message(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api16 = api.clone();
    router = router.route("/message_cell/edit_message", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api16;
        let __raw = <T as crate::types::MessageCell>::edit_message(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api17 = api.clone();
    router = router.route("/message_cell/delete_message", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api17;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let channel_id = __query.get("channel_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(channel_id) = channel_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let message_id = __query.get("message_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(message_id) = message_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::MessageCell>::delete_message(&api, guild_id, channel_id, message_id, user_id, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api18 = api.clone();
    router = router.route("/message_cell/get_messages", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api18;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let channel_id = __query.get("channel_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(channel_id) = channel_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let before = __query.get("before").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(before) = before else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let limit = __query.get("limit").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(limit) = limit else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::MessageCell>::get_messages(&api, guild_id, channel_id, user_id, before, limit, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api19 = api.clone();
    router = router.route("/message_cell/poll_events", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api19;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let after = __query.get("after").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(after) = after else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let wait_ms = __query.get("wait_ms").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(wait_ms) = wait_ms else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::MessageCell>::poll_events(&api, guild_id, after, wait_ms, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api20 = api.clone();
    router = router.route("/message_cell/freeze_guild", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api20;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::MessageCell>::freeze_guild(&api, guild_id, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api21 = api.clone();
    router = router.route("/message_cell/export_guild", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api21;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let cursor = __query.get("cursor").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(cursor) = cursor else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let limit = __query.get("limit").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(limit) = limit else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::MessageCell>::export_guild(&api, guild_id, cursor, limit, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api22 = api.clone();
    router = router.route("/message_cell/import_guild", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api22;
        let __raw = <T as crate::types::MessageCell>::import_guild(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api23 = api.clone();
    router = router.route("/message_cell/finish_move", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api23;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let moved = __query.get("moved").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(moved) = moved else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::MessageCell>::finish_move(&api, guild_id, moved, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api24 = api.clone();
    router = router.route("/message_cell/discard_guild", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, | {
        let api = api24;
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::MessageCell>::discard_guild(&api, guild_id, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));

    router
}

/// Serves the generated Axum router on the specified address.
#[allow(unused)]
pub async fn serve<T: crate::types::MessageCell>(api: T, address: &str) -> Result<(), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, router(api)).await?;
    Ok(())
}