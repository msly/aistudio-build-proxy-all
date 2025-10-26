//! AI Studio Proxy - Rust Implementation
//! 
//! A high-performance WebSocket proxy server for AI Studio Build services

mod connection;
mod message;
mod proxy;
mod auth;

use axum::{
    extract::{
        ws::{Message, WebSocketUpgrade},
        Query, State,
    },
    http::HeaderMap,
    response::Response,
    routing::{get, post},
    Router,
};
use futures_util::{sink::SinkExt, stream::StreamExt};
use std::{
    collections::HashMap,
    sync::Arc,
    time::Duration,
};
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;
use tracing::{info, warn, error};
use uuid::Uuid;

use crate::{
    connection::ConnectionPool,
    message::WSMessage,
    proxy::{AppState, handle_proxy_request, handle_streaming_response},
    auth::authenticate_websocket,
};

// 常量定义
const WS_PATH: &str = "/v1/ws";
const PROXY_LISTEN_ADDR: &str = "0.0.0.0:5345";
const WS_READ_TIMEOUT: Duration = Duration::from_secs(60);
const PROXY_REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

/// WebSocket处理器
async fn websocket_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<HashMap<String, String>>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let auth_token = params.get("auth_token").cloned().unwrap_or_default();
    
    // 简化的认证逻辑
    let user_id = if auth_token == "valid-token-user-1" {
        "user-1".to_string()
    } else {
        return (axum::http::StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    };

    ws.on_upgrade(move |socket| handle_socket(socket, user_id, state))
}

async fn handle_socket(socket: axum::extract::ws::WebSocket, user_id: String, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<WSMessage>();
    
    // 添加到连接池
    let connection = state.connection_pool.add_connection(user_id.clone(), tx).await;
    
    // 启动发送任务
    let connection_clone = connection.clone();
    let state_clone = state.clone();
    let user_id_clone = user_id.clone();
    tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if let Err(e) = sender.send(Message::Text(serde_json::to_string(&message).unwrap())).await {
                error!("Failed to send WebSocket message: {}", e);
                break;
            }
        }
        
        // 清理连接
        state_clone.connection_pool.remove_connection(&user_id_clone, &connection_clone.user_id).await;
    });

    // 处理接收的消息
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Ok(ws_message) = serde_json::from_str::<WSMessage>(&text) {
                    match ws_message.r#type.as_str() {
                        "ping" => {
                            let pong = WSMessage {
                                id: ws_message.id,
                                r#type: "pong".to_string(),
                                payload: serde_json::Value::Null,
                            };
                            let _ = connection.sender.send(pong).await;
                        }
                        "http_response" | "stream_start" | "stream_chunk" | "stream_end" | "error" => {
                            if let Some(sender) = state.connection_pool.pending_requests.get(&ws_message.id) {
                                let _ = sender.send(ws_message);
                            }
                        }
                        _ => {
                            warn!("Received unknown message type: {}", ws_message.r#type);
                        }
                    }
                }
            }
            Ok(Message::Close(_)) => {
                info!("WebSocket connection closed");
                break;
            }
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }
}

/// HTTP代理处理器
async fn proxy_handler(
    headers: HeaderMap,
    body: String,
    State(state): State<Arc<AppState>>,
) -> Result<String, axum::http::StatusCode> {
    // 认证检查
    let api_key = headers
        .get("x-goog-api-key")
        .and_then(|h| h.to_str().ok());

    if api_key != Some(&state.auth_api_key) {
        return Err(axum::http::StatusCode::UNAUTHORIZED);
    }

    let user_id = "user-1".to_string();
    let req_id = Uuid::new_v4().to_string();

    // 获取连接
    let connection = match state.connection_pool.get_connection(&user_id).await {
        Some(conn) => conn,
        None => return Err(axum::http::StatusCode::SERVICE_UNAVAILABLE),
    };

    // 创建响应通道
    let (tx, mut rx) = mpsc::unbounded_channel::<WSMessage>();
    state.connection_pool.register_pending_request(req_id.clone(), tx);

    // 构建请求消息
    let request_message = WSMessage {
        id: req_id.clone(),
        r#type: "http_request".to_string(),
        payload: serde_json::json!({
            "method": "POST",
            "url": "https://generativelanguage.googleapis.com/v1/models/gemini-pro:generate",
            "headers": headers_to_json(&headers),
            "body": body
        }),
    };

    // 发送请求
    if connection.send_message(request_message).await.is_err() {
        return Err(axum::http::StatusCode::BAD_GATEWAY);
    }

    // 等待响应
    let timeout = tokio::time::sleep(PROXY_REQUEST_TIMEOUT);
    tokio::select! {
        response = rx.recv() => {
            if let Some(msg) = response {
                match msg.r#type.as_str() {
                    "http_response" => {
                        if let Some(body) = msg.payload.get("body").and_then(|v| v.as_str()) {
                            Ok(body.to_string())
                        } else {
                            Err(axum::http::StatusCode::BAD_GATEWAY)
                        }
                    }
                    "error" => Err(axum::http::StatusCode::BAD_GATEWAY),
                    _ => Err(axum::http::StatusCode::BAD_GATEWAY)
                }
            } else {
                Err(axum::http::StatusCode::GATEWAY_TIMEOUT)
            }
        }
        _ = timeout => {
            Err(axum::http::StatusCode::GATEWAY_TIMEOUT)
        }
    }
}

// 辅助函数
fn headers_to_json(headers: &HeaderMap) -> serde_json::Value {
    let mut header_map = serde_json::Map::new();
    for (key, value) in headers.iter() {
        if let Ok(value_str) = value.to_str() {
            header_map.insert(
                key.to_string(),
                serde_json::Value::String(value_str.to_string())
            );
        }
    }
    serde_json::Value::Object(header_map)
}

#[tokio::main]
async fn main() {
    // 初始化日志
    tracing_subscriber::fmt::init();

    // 从环境变量获取API密钥
    let auth_api_key = std::env::var("AUTH_API_KEY")
        .unwrap_or_else(|_| "your_set_api_key_here".to_string());

    let state = Arc::new(AppState::new(auth_api_key));

    // 构建路由
    let app = Router::new()
        .route("/v1/ws", get(websocket_handler))
        .route("/v1/models/:model/generate", post(proxy_handler))
        .route("/", post(proxy_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    info!("Starting server on {}", PROXY_LISTEN_ADDR);
    info!("WebSocket endpoint available at ws://{}{}", PROXY_LISTEN_ADDR, WS_PATH);
    info!("HTTP proxy available at http://{}/", PROXY_LISTEN_ADDR);

    let listener = tokio::net::TcpListener::bind(PROXY_LISTEN_ADDR).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}