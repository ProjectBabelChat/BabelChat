//! The WebSocket gateway: the session queues of [`crate::gateway`], pushed
//! instead of long-polled.
//!
//! ```text
//! POST /gateway/connect                    -> Ready (session, cursor)
//! GET  /gateway/ws?session_id=..&after=..  -> EventBatch frames, as events arrive
//! ```
//!
//! The socket only carries what `poll` would return, so a client can switch
//! between the two at any time and resume from the last `next` either way.
//! Close code 4404 means the session is gone, like `poll`'s `not_found`.

use std::time::Duration;

use axum::{
    Router,
    extract::{
        Query, State,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header::AUTHORIZATION},
    response::{IntoResponse, Response},
    routing::get,
};
use babel_api_edge::types::{Claim, EventBatch, JwtValidator};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;

use crate::Edge;

/// How long the server waits for events before pinging an idle socket.
const IDLE_PING: Duration = Duration::from_secs(25);
pub const SESSION_GONE: u16 = 4404;

#[derive(Deserialize)]
struct Resume {
    session_id: String,
    after: i64,
}

pub fn router(edge: Edge) -> Router {
    Router::new()
        .route("/gateway/ws", get(upgrade))
        .with_state(edge)
}

async fn upgrade(
    State(edge): State<Edge>,
    headers: HeaderMap,
    Query(resume): Query<Resume>,
    ws: WebSocketUpgrade,
) -> Response {
    let claim = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .and_then(|token| edge.validate::<Claim>(token).ok());
    let Some(claim) = claim else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    // Same answer as a poll for someone else's session.
    if !edge.inner.sessions.owns(&resume.session_id, &claim.sub) {
        return StatusCode::NOT_FOUND.into_response();
    }
    ws.on_upgrade(move |socket| serve(edge, claim, resume, socket))
}

/// Counts the open sockets (`babel_edge_websockets`) while it lives.
struct Open(metrics::Gauge);

impl Open {
    fn new() -> Self {
        let sockets = metrics::gauge!("babel_edge_websockets");
        sockets.increment(1.0);
        Self(sockets)
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        self.0.decrement(1.0);
    }
}

async fn serve(edge: Edge, claim: Claim, resume: Resume, socket: WebSocket) {
    let _open = Open::new();
    let (mut sender, mut receiver) = socket.split();
    let mut after = resume.after;
    loop {
        let polled = tokio::select! {
            polled = edge.inner.sessions.poll(&resume.session_id, &claim.sub, after, IDLE_PING) => polled,
            // Clients send nothing; anything else here is a close or an error.
            incoming = receiver.next() => match incoming {
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                _ => return,
            },
        };
        let message = match polled {
            None => {
                let _ = sender
                    .send(Message::Close(Some(CloseFrame {
                        code: SESSION_GONE,
                        reason: "session gone; connect again".into(),
                    })))
                    .await;
                return;
            }
            Some((events, _)) if events.is_empty() => Message::Ping(Default::default()),
            Some((events, next)) => {
                after = next;
                match serde_json::to_string(&EventBatch { events, next }) {
                    Ok(json) => Message::Text(json.into()),
                    Err(_) => return,
                }
            }
        };
        if sender.send(message).await.is_err() {
            return;
        }
    }
}
