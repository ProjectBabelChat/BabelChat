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
pub fn router<T: crate::types::Auth + crate::types::Me + crate::types::Guilds + crate::types::Messages + crate::types::Gateway + crate::types::JwtValidator>(api: T) -> Router {
    let api = Arc::new(api);
    let mut router = Router::new();

    let api1 = api.clone();
    router = router.route("/auth/register", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api1;
        let __raw = <T as crate::types::Auth>::register(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api2 = api.clone();
    router = router.route("/auth/login", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       Json(__body): Json<_>| {
        let api = api2;
        let __raw = <T as crate::types::Auth>::login(&api, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api3 = api.clone();
    router = router.route("/me/get_settings", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api3;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let __raw = <T as crate::types::Me>::get_settings(&api, __claim, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api4 = api.clone();
    router = router.route("/me/put_settings", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, 
       Json(__body): Json<_>| {
        let api = api4;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let __raw = <T as crate::types::Me>::put_settings(&api, __claim, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api5 = api.clone();
    router = router.route("/me/change_password", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, 
       Json(__body): Json<_>| {
        let api = api5;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let __raw = <T as crate::types::Me>::change_password(&api, __claim, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api6 = api.clone();
    router = router.route("/me/sign_out_others", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api6;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let __raw = <T as crate::types::Me>::sign_out_others(&api, __claim, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api7 = api.clone();
    router = router.route("/guilds/create_guild", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, 
       Json(__body): Json<_>| {
        let api = api7;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let __raw = <T as crate::types::Guilds>::create_guild(&api, __claim, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api8 = api.clone();
    router = router.route("/guilds/create_invite", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, 
       Json(__body): Json<_>| {
        let api = api8;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let __raw = <T as crate::types::Guilds>::create_invite(&api, __claim, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api9 = api.clone();
    router = router.route("/guilds/revoke_invite", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api9;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let code = __query.get("code").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(code) = code else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Guilds>::revoke_invite(&api, __claim, code, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api10 = api.clone();
    router = router.route("/guilds/get_invites", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api10;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Guilds>::get_invites(&api, __claim, guild_id, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api11 = api.clone();
    router = router.route("/guilds/join_guild", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api11;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let invite = __query.get("invite").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(invite) = invite else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Guilds>::join_guild(&api, __claim, invite, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api12 = api.clone();
    router = router.route("/guilds/get_guild", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api12;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Guilds>::get_guild(&api, __claim, guild_id, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api13 = api.clone();
    router = router.route("/guilds/update_guild", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, 
       Json(__body): Json<_>| {
        let api = api13;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let __raw = <T as crate::types::Guilds>::update_guild(&api, __claim, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api14 = api.clone();
    router = router.route("/guilds/delete_guild", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api14;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Guilds>::delete_guild(&api, __claim, guild_id, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api15 = api.clone();
    router = router.route("/guilds/create_channel", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, 
       Json(__body): Json<_>| {
        let api = api15;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let __raw = <T as crate::types::Guilds>::create_channel(&api, __claim, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api16 = api.clone();
    router = router.route("/guilds/update_channel", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, 
       Json(__body): Json<_>| {
        let api = api16;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let __raw = <T as crate::types::Guilds>::update_channel(&api, __claim, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api17 = api.clone();
    router = router.route("/guilds/delete_channel", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api17;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let channel_id = __query.get("channel_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(channel_id) = channel_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Guilds>::delete_channel(&api, __claim, guild_id, channel_id, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api18 = api.clone();
    router = router.route("/guilds/put_language_policy", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, 
       Json(__body): Json<_>| {
        let api = api18;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let __raw = <T as crate::types::Guilds>::put_language_policy(&api, __claim, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api19 = api.clone();
    router = router.route("/guilds/get_models", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api19;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Guilds>::get_models(&api, __claim, guild_id, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api20 = api.clone();
    router = router.route("/guilds/leave_guild", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api20;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Guilds>::leave_guild(&api, __claim, guild_id, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api21 = api.clone();
    router = router.route("/guilds/remove_member", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api21;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let ban = __query.get("ban").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(ban) = ban else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Guilds>::remove_member(&api, __claim, guild_id, user_id, ban, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api22 = api.clone();
    router = router.route("/guilds/unban_member", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api22;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let user_id = __query.get("user_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(user_id) = user_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Guilds>::unban_member(&api, __claim, guild_id, user_id, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api23 = api.clone();
    router = router.route("/guilds/get_bans", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api23;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Guilds>::get_bans(&api, __claim, guild_id, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api24 = api.clone();
    router = router.route("/guilds/put_notification_settings", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, 
       Json(__body): Json<_>| {
        let api = api24;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let __raw = <T as crate::types::Guilds>::put_notification_settings(&api, __claim, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api25 = api.clone();
    router = router.route("/guilds/delete_emoji", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api25;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let emoji_id = __query.get("emoji_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(emoji_id) = emoji_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Guilds>::delete_emoji(&api, __claim, guild_id, emoji_id, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api26 = api.clone();
    router = router.route("/messages/send_message", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, 
       Json(__body): Json<_>| {
        let api = api26;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let __raw = <T as crate::types::Messages>::send_message(&api, __claim, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api27 = api.clone();
    router = router.route("/messages/edit_message", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, 
       Json(__body): Json<_>| {
        let api = api27;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let __raw = <T as crate::types::Messages>::edit_message(&api, __claim, __body, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api28 = api.clone();
    router = router.route("/messages/delete_message", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api28;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let channel_id = __query.get("channel_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(channel_id) = channel_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let message_id = __query.get("message_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(message_id) = message_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Messages>::delete_message(&api, __claim, guild_id, channel_id, message_id, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api29 = api.clone();
    router = router.route("/messages/add_reaction", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api29;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let channel_id = __query.get("channel_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(channel_id) = channel_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let message_id = __query.get("message_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(message_id) = message_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let emoji = __query.get("emoji").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(emoji) = emoji else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Messages>::add_reaction(&api, __claim, guild_id, channel_id, message_id, emoji, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api30 = api.clone();
    router = router.route("/messages/remove_reaction", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api30;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let channel_id = __query.get("channel_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(channel_id) = channel_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let message_id = __query.get("message_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(message_id) = message_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let emoji = __query.get("emoji").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(emoji) = emoji else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Messages>::remove_reaction(&api, __claim, guild_id, channel_id, message_id, emoji, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api31 = api.clone();
    router = router.route("/messages/typing", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api31;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let channel_id = __query.get("channel_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(channel_id) = channel_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Messages>::typing(&api, __claim, guild_id, channel_id, ).await;
        let result = __raw;
        match result {
            Ok(_) => (StatusCode::OK, ()).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api32 = api.clone();
    router = router.route("/messages/get_messages", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api32;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let channel_id = __query.get("channel_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(channel_id) = channel_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let before = __query.get("before").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(before) = before else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let limit = __query.get("limit").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(limit) = limit else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Messages>::get_messages(&api, __claim, guild_id, channel_id, before, limit, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api33 = api.clone();
    router = router.route("/messages/ack", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api33;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let guild_id = __query.get("guild_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(guild_id) = guild_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let channel_id = __query.get("channel_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(channel_id) = channel_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let message_id = __query.get("message_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(message_id) = message_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Messages>::ack(&api, __claim, guild_id, channel_id, message_id, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api34 = api.clone();
    router = router.route("/gateway/connect", axum::routing::post(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api34;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let __raw = <T as crate::types::Gateway>::connect(&api, __claim, ).await;
        let result = __raw;
        match result {
            Ok(value) => (StatusCode::OK, Json(value)).into_response(),
            Err(error) => (StatusCode::BAD_REQUEST, Json(error)).into_response(),
        }
    }));
    let api35 = api.clone();
    router = router.route("/gateway/poll", axum::routing::get(async move |Query(__query): Query<HashMap<String, String>>, 
       __header: axum::http::HeaderMap, | {
        let api = api35;

        let Some(__claim) = crate::types::__bearer(&__header)
                                .map(|token| <T as crate::types::JwtValidator>::validate(&api, token).ok())
                                .flatten() else {
            return Response::builder().status(StatusCode::UNAUTHORIZED).body(Body::empty()).unwrap();
        };
        let session_id = __query.get("session_id").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(session_id) = session_id else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let after = __query.get("after").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(after) = after else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let wait_ms = __query.get("wait_ms").map(|str| serde_json::from_str(str).ok()).flatten();
        let Some(wait_ms) = wait_ms else { return Response::builder().status(StatusCode::BAD_REQUEST).body(Body::empty()).unwrap(); };
        let __raw = <T as crate::types::Gateway>::poll(&api, __claim, session_id, after, wait_ms, ).await;
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
pub async fn serve<T: crate::types::Auth + crate::types::Me + crate::types::Guilds + crate::types::Messages + crate::types::Gateway + crate::types::JwtValidator>(api: T, address: &str) -> Result<(), std::io::Error> {
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, router(api)).await?;
    Ok(())
}