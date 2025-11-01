//! HTTP proxy functionality for handling requests and responses

use crate::connection::ConnectionPool;
use crate::message::{HttpResponse, StreamChunk, WSMessage};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Response, Sse},
};
use futures_util::stream::{self, Stream};
use std::{collections::HashMap, convert::Infallible, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Handle HTTP proxy requests
pub async fn handle_proxy_request(
    method: String,
    path: String,
    query_string: Option<String>,
    headers: HeaderMap,
    body: String,
    state: State<Arc<AppState>>,
) -> Result<Response<String>, StatusCode> {
    // Authenticate request
    let user_id = authenticate_request(&headers, &query_string)?;

    // Get connection from pool
    let connection = state
        .connection_pool
        .get_connection(&user_id)
        .await
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // Generate request ID
    let req_id = Uuid::new_v4().to_string();

    // Create response channel
    let (tx, mut rx) = mpsc::unbounded_channel::<WSMessage>();
    state
        .connection_pool
        .register_pending_request(req_id.clone(), tx);

    // Build target URL
    let target_url = format!("https://generativelanguage.googleapis.com{}", path);

    // Convert headers
    let header_map = headers_to_map(&headers);

    // Create request message
    let request_message =
        WSMessage::http_request(req_id.clone(), method, target_url, header_map, body);

    // Send request to WebSocket client
    if connection.send_message(request_message).await.is_err() {
        state.connection_pool.remove_pending_request(&req_id);
        return Err(StatusCode::BAD_GATEWAY);
    }

    // Wait for response with timeout
    let timeout = tokio::time::sleep(Duration::from_secs(600));

    tokio::select! {
        response = rx.recv() => {
            state.connection_pool.remove_pending_request(&req_id);

            match response {
                Some(msg) => handle_websocket_response(msg),
                None => Err(StatusCode::GATEWAY_TIMEOUT),
            }
        }
        _ = timeout => {
            state.connection_pool.remove_pending_request(&req_id);
            Err(StatusCode::GATEWAY_TIMEOUT)
        }
    }
}

/// Handle WebSocket response and convert to HTTP response
fn handle_websocket_response(msg: WSMessage) -> Result<Response<String>, StatusCode> {
    match msg.r#type.as_str() {
        "http_response" => {
            if let Some(response) = Option::<HttpResponse>::from(&msg) {
                let mut response_builder = Response::builder().status(response.status);

                // Add headers
                for (key, value) in response.headers {
                    response_builder = response_builder.header(key, value);
                }

                response_builder
                    .body(response.body)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
            } else {
                Err(StatusCode::BAD_GATEWAY)
            }
        }
        "error" => {
            let _error_msg = msg
                .payload
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");

            let status = msg
                .payload
                .get("status")
                .and_then(|v| v.as_u64())
                .map(|s| s as u16)
                .unwrap_or(500);

            Err(StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY))
        }
        _ => {
            warn!("Received unexpected message type: {}", msg.r#type);
            Err(StatusCode::BAD_GATEWAY)
        }
    }
}

/// Handle streaming responses
pub async fn handle_streaming_response(
    method: String,
    path: String,
    query_string: Option<String>,
    headers: HeaderMap,
    body: String,
    state: State<Arc<AppState>>,
) -> Result<Sse<impl Stream<Item = Result<axum::response::sse::Event, Infallible>>>, StatusCode> {
    // Authenticate request
    let user_id = authenticate_request(&headers, &query_string)?;

    // Get connection from pool
    let connection = state
        .connection_pool
        .get_connection(&user_id)
        .await
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    // Generate request ID
    let req_id = Uuid::new_v4().to_string();

    // Create response channel
    let (tx, rx) = mpsc::unbounded_channel::<WSMessage>();
    state
        .connection_pool
        .register_pending_request(req_id.clone(), tx);

    // Build target URL
    let target_url = format!("https://generativelanguage.googleapis.com{}", path);

    // Convert headers
    let header_map = headers_to_map(&headers);

    // Create request message
    let request_message =
        WSMessage::http_request(req_id.clone(), method, target_url, header_map, body);

    // Send request to WebSocket client
    if connection.send_message(request_message).await.is_err() {
        state.connection_pool.remove_pending_request(&req_id);
        return Err(StatusCode::BAD_GATEWAY);
    }

    // Create streaming response
    let stream = stream::unfold(
        (rx, req_id.clone(), state.clone()),
        |(mut rx, req_id, state)| async move {
            match rx.recv().await {
                Some(msg) => match msg.r#type.as_str() {
                    "stream_start" => {
                        info!("Stream started for request {}", req_id);
                        Some((
                            Ok(axum::response::sse::Event::default().data("stream_start")),
                            (rx, req_id, state),
                        ))
                    }
                    "stream_chunk" => {
                        if let Some(chunk) = Option::<StreamChunk>::from(&msg) {
                            let event = axum::response::sse::Event::default().data(chunk.data);
                            Some((Ok(event), (rx, req_id, state)))
                        } else {
                            Some((
                                Ok(axum::response::sse::Event::default().data("")),
                                (rx, req_id, state),
                            ))
                        }
                    }
                    "stream_end" => {
                        info!("Stream ended for request {}", req_id);
                        state.connection_pool.remove_pending_request(&req_id);
                        None
                    }
                    "error" => {
                        error!("Stream error for request {}", req_id);
                        state.connection_pool.remove_pending_request(&req_id);
                        None
                    }
                    _ => {
                        warn!("Unexpected message type in stream: {}", msg.r#type);
                        Some((
                            Ok(axum::response::sse::Event::default().data("")),
                            (rx, req_id, state),
                        ))
                    }
                },
                None => {
                    info!("Stream channel closed for request {}", req_id);
                    state.connection_pool.remove_pending_request(&req_id);
                    None
                }
            }
        },
    );

    Ok(Sse::new(stream))
}

/// Convert HTTP headers to HashMap
fn headers_to_map(headers: &HeaderMap) -> HashMap<String, String> {
    let mut map = HashMap::new();

    for (key, value) in headers.iter() {
        if let Ok(value_str) = value.to_str() {
            // Filter out proxy-specific headers
            if !matches!(
                key.as_str(),
                "connection"
                    | "keep-alive"
                    | "proxy-authenticate"
                    | "proxy-authorization"
                    | "te"
                    | "trailers"
                    | "transfer-encoding"
                    | "upgrade"
            ) {
                map.insert(key.to_string(), value_str.to_string());
            }
        }
    }

    map
}

/// Authenticate HTTP request
pub fn authenticate_request(headers: &HeaderMap, query_string: &Option<String>) -> Result<String, StatusCode> {
    // Check for API key in headers first
    if let Some(api_key) = headers.get("x-goog-api-key").and_then(|h| h.to_str().ok()) {
        let expected_key =
            std::env::var("AUTH_API_KEY").unwrap_or_else(|_| "your_set_api_key_here".to_string());
        if api_key == expected_key {
            return Ok("user-1".to_string());
        }
    }

    // Check for API key in query string
    if let Some(query) = query_string {
        // Parse query parameters to find 'key' parameter
        for param in query.split('&') {
            if let Some((key, value)) = param.split_once('=') {
                if key == "key" {
                    let expected_key =
                        std::env::var("AUTH_API_KEY").unwrap_or_else(|_| "your_set_api_key_here".to_string());
                    if value == expected_key {
                        return Ok("user-1".to_string());
                    }
                }
            }
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}

/// Application state
#[derive(Debug, Clone)]
pub struct AppState {
    pub connection_pool: ConnectionPool,
    pub auth_api_key: String,
}

impl AppState {
    pub fn new(auth_api_key: String) -> Self {
        Self {
            connection_pool: ConnectionPool::new(),
            auth_api_key,
        }
    }
}
