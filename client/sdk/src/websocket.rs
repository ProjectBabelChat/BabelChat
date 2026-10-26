//! Receiving a session's events over the Edge's WebSocket (`/gateway/ws`).
//!
//! The socket carries the same `EventBatch`es as long polling, so the
//! session can switch between the two and resume from its cursor either
//! way. See types/edge.tyml (`Gateway`).

use std::time::Duration;

use babel_api_edge_client::types::{ApiError, EventBatch};
use futures::StreamExt;
use tokio_tungstenite::tungstenite::{
    self, Message as Frame, client::IntoClientRequest, http::StatusCode,
};

use crate::{Client, Error};

/// Close code for "the session is gone; connect again".
const SESSION_GONE: u16 = 4404;

pub(crate) enum Ended {
    /// The server has no WebSocket gateway (or would not upgrade): poll.
    Unsupported,
    Failed(Error),
}

/// Streams batches into `on_batch` until the socket ends. `on_open` runs
/// once the socket is up.
pub(crate) async fn stream(
    client: &Client,
    session_id: &str,
    after: i64,
    idle: Duration,
    on_open: impl FnOnce(),
    mut on_batch: impl FnMut(EventBatch),
) -> Ended {
    let base = client.server();
    let base = match base.split_once("://") {
        Some(("https", rest)) => format!("wss://{rest}"),
        Some((_, rest)) => format!("ws://{rest}"),
        None => format!("ws://{base}"),
    };
    let url = format!("{base}/gateway/ws?session_id={session_id}&after={after}");
    let mut request = match url.into_client_request() {
        Ok(request) => request,
        Err(error) => return Ended::Failed(Error::Transport(error.to_string())),
    };
    let bearer = format!("Bearer {}", client.jwt());
    match bearer.parse() {
        Ok(value) => {
            request.headers_mut().insert("authorization", value);
        }
        Err(_) => return Ended::Failed(Error::Unauthorized),
    }

    let mut socket = match tokio_tungstenite::connect_async(request).await {
        Ok((socket, _)) => socket,
        Err(tungstenite::Error::Http(response)) => {
            return match response.status() {
                StatusCode::UNAUTHORIZED => Ended::Failed(Error::Unauthorized),
                // No such route, or no such session: a poll tells which.
                _ => Ended::Unsupported,
            };
        }
        Err(error) => return Ended::Failed(Error::Transport(error.to_string())),
    };
    on_open();

    loop {
        let frame = match tokio::time::timeout(idle, socket.next()).await {
            Err(_) => return Ended::Failed(Error::Transport("the gateway went quiet".into())),
            Ok(None) => return Ended::Failed(Error::Transport("the gateway closed".into())),
            Ok(Some(Err(error))) => return Ended::Failed(Error::Transport(error.to_string())),
            Ok(Some(Ok(frame))) => frame,
        };
        match frame {
            Frame::Text(text) => match serde_json::from_str::<EventBatch>(&text) {
                Ok(batch) => on_batch(batch),
                Err(error) => return Ended::Failed(Error::Transport(error.to_string())),
            },
            Frame::Close(Some(close)) if u16::from(close.code) == SESSION_GONE => {
                return Ended::Failed(Error::Api(ApiError::NotFound));
            }
            Frame::Close(_) => {
                return Ended::Failed(Error::Transport("the gateway closed".into()));
            }
            // Pings are answered by the library while reading.
            _ => {}
        }
    }
}
