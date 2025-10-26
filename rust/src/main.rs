//! AI Studio Proxy - Rust Implementation
//!
//! A high-performance WebSocket proxy server for AI Studio Build services

mod auth;
mod connection;
mod message;
mod proxy;

use axum::{
    extract::{
        ws::{Message, WebSocketUpgrade},
        Query, State,
    },
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use futures_util::{sink::SinkExt, stream::StreamExt};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::{message::WSMessage, proxy::AppState};

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

async fn handle_socket(
    socket: axum::extract::ws::WebSocket,
    user_id: String,
    state: Arc<AppState>,
) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<WSMessage>();

    // 添加到连接池
    let connection = state
        .connection_pool
        .add_connection(user_id.clone(), tx)
        .await;

    // 启动发送任务
    let connection_clone = connection.clone();
    let state_clone = state.clone();
    let user_id_clone = user_id.clone();
    tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if let Err(e) = sender
                .send(Message::Text(serde_json::to_string(&message).unwrap()))
                .await
            {
                error!("Failed to send WebSocket message: {}", e);
                break;
            }
        }

        // 清理连接
        state_clone
            .connection_pool
            .remove_connection(&user_id_clone, &connection_clone.user_id)
            .await;
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
                            let _ = connection.sender.send(pong);
                        }
                        "http_response" | "stream_start" | "stream_chunk" | "stream_end"
                        | "error" => {
                            if let Some(sender) =
                                state.connection_pool.pending_requests.get(&ws_message.id)
                            {
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

/// HTTP代理处理器 - 支持流式和非流式响应
async fn proxy_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(path): axum::extract::Path<String>,
    headers: HeaderMap,
    body: String,
) -> Response {
    info!(
        "Received HTTP request: path={}, body_len={}",
        path,
        body.len()
    );

    // 认证检查
    let api_key = headers
        .get("x-goog-api-key")
        .and_then(|h| h.to_str().ok())
        .or_else(|| {
            // 也检查 query 参数
            headers.get("x-original-uri").and_then(|uri| {
                uri.to_str()
                    .ok()
                    .and_then(|s| s.split("key=").nth(1))
                    .and_then(|s| s.split('&').next())
            })
        });

    if api_key != Some(&state.auth_api_key) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            "Unauthorized: Invalid or missing API key",
        )
            .into_response();
    }

    let user_id = "user-1".to_string();
    let req_id = Uuid::new_v4().to_string();

    info!("Authenticated as user_id={}, req_id={}", user_id, req_id);

    // 获取连接
    let connection = match state.connection_pool.get_connection(&user_id).await {
        Some(conn) => conn,
        None => {
            return (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "Service Unavailable: No active client connected",
            )
                .into_response()
        }
    };

    // 创建响应通道
    let (tx, rx) = mpsc::unbounded_channel::<WSMessage>();
    state
        .connection_pool
        .register_pending_request(req_id.clone(), tx);

    // 构建完整的目标 URL
    // 确保 path 以斜杠开头
    let target_url = if path.starts_with('/') {
        format!("https://generativelanguage.googleapis.com{}", path)
    } else {
        format!("https://generativelanguage.googleapis.com/{}", path)
    };

    // 转换 headers，过滤代理特有的头
    let forwarded_headers = filter_headers(&headers);

    // 构建请求消息
    let request_message = WSMessage {
        id: req_id.clone(),
        r#type: "http_request".to_string(),
        payload: serde_json::json!({
            "method": "POST",
            "url": target_url,
            "headers": forwarded_headers,
            "body": body
        }),
    };

    // 发送请求
    info!(
        "Sending request to WebSocket client: req_id={}, url={}",
        req_id, target_url
    );
    info!(
        "Request payload: {:?}",
        serde_json::to_string(&request_message).unwrap()
    );
    if connection.send_message(request_message).await.is_err() {
        state.connection_pool.remove_pending_request(&req_id);
        return (
            axum::http::StatusCode::BAD_GATEWAY,
            "Bad Gateway: Failed to send request to client",
        )
            .into_response();
    }

    // 处理响应（支持流式和非流式）
    process_websocket_response(rx, req_id, state).await
}

/// 处理 WebSocket 响应，支持流式和非流式
async fn process_websocket_response(
    mut rx: mpsc::UnboundedReceiver<WSMessage>,
    req_id: String,
    state: Arc<AppState>,
) -> Response {
    use axum::response::sse::{Event, Sse};
    use futures_util::stream;

    let timeout = tokio::time::sleep(PROXY_REQUEST_TIMEOUT);
    tokio::pin!(timeout);

    // 首先等待第一条消息以确定是流式还是非流式
    info!("Waiting for response for req_id={}", req_id);
    tokio::select! {
        first_msg = rx.recv() => {
            match first_msg {
                Some(msg) => {
                    info!("Received WebSocket message: type={}, req_id={}", msg.r#type, req_id);
                    match msg.r#type.as_str() {
                        "http_response" => {
                            // 非流式响应
                            state.connection_pool.remove_pending_request(&req_id);
                            build_http_response(msg)
                        }
                        "stream_start" => {
                            // 流式响应 - 创建 SSE stream
                            info!("Starting SSE stream for request {}", req_id);

                            let stream = stream::unfold(
                                (rx, req_id.clone(), state.clone(), false),
                                |(mut rx, req_id, state, mut ended)| async move {
                                    if ended {
                                        return None;
                                    }

                                    match rx.recv().await {
                                        Some(msg) => match msg.r#type.as_str() {
                                            "stream_chunk" => {
                                                if let Some(data) = msg.payload.get("data").and_then(|v| v.as_str()) {
                                                    // JavaScript 客户端发送的数据可能已经包含 "data: " 前缀
                                                    // 我们需要剥离它，因为 Axum 的 SSE 会自动添加
                                                    let clean_data = if data.starts_with("data: ") {
                                                        &data[6..] // 移除 "data: " 前缀
                                                    } else {
                                                        data
                                                    };

                                                    // SSE 不允许 data 字段包含换行符
                                                    // 将换行符替换为空格，或者压缩 JSON 为单行
                                                    let sanitized_data = clean_data.replace('\n', " ").replace('\r', "");
                                                    Some((
                                                        Ok::<_, std::convert::Infallible>(Event::default().data(sanitized_data)),
                                                        (rx, req_id, state, ended),
                                                    ))
                                                } else {
                                                    Some((
                                                        Ok(Event::default().data("")),
                                                        (rx, req_id, state, ended),
                                                    ))
                                                }
                                            }
                                            "stream_end" => {
                                                info!("Stream ended for request {}", req_id);
                                                state.connection_pool.remove_pending_request(&req_id);
                                                ended = true;
                                                None
                                            }
                                            "error" => {
                                                error!("Stream error for request {}: {:?}", req_id, msg.payload);
                                                state.connection_pool.remove_pending_request(&req_id);
                                                None
                                            }
                                            _ => {
                                                warn!("Unexpected message type in stream: {}", msg.r#type);
                                                Some((
                                                    Ok(Event::default().data("")),
                                                    (rx, req_id, state, ended),
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

                            Sse::new(stream).into_response()
                        }
                        "error" => {
                            error!("Client returned error: {:?}", msg.payload);
                            state.connection_pool.remove_pending_request(&req_id);
                            build_error_response(msg)
                        }
                        _ => {
                            state.connection_pool.remove_pending_request(&req_id);
                            (
                                axum::http::StatusCode::BAD_GATEWAY,
                                format!("Unexpected message type: {}", msg.r#type),
                            )
                                .into_response()
                        }
                    }
                }
                None => {
                    state.connection_pool.remove_pending_request(&req_id);
                    (
                        axum::http::StatusCode::GATEWAY_TIMEOUT,
                        "Gateway Timeout: No response from client",
                    )
                        .into_response()
                }
            }
        }
        _ = &mut timeout => {
            state.connection_pool.remove_pending_request(&req_id);
            (
                axum::http::StatusCode::GATEWAY_TIMEOUT,
                "Gateway Timeout: Request timed out",
            )
                .into_response()
        }
    }
}

/// 构建 HTTP 响应
fn build_http_response(msg: WSMessage) -> Response {
    use axum::http::Response as HttpResponse;

    let status = msg
        .payload
        .get("status")
        .and_then(|v| v.as_u64())
        .map(|s| s as u16)
        .unwrap_or(200);

    let body = msg
        .payload
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut response = HttpResponse::builder().status(status);

    // 设置响应头
    if let Some(headers) = msg.payload.get("headers") {
        if let Some(headers_obj) = headers.as_object() {
            for (key, value) in headers_obj {
                if let Some(value_str) = value.as_str() {
                    response = response.header(key, value_str);
                }
            }
        }
    }

    response.body(body).unwrap().into_response()
}

/// 构建错误响应
fn build_error_response(msg: WSMessage) -> Response {
    let error_msg = msg
        .payload
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown error from client")
        .to_string();

    let status = msg
        .payload
        .get("status")
        .and_then(|v| v.as_u64())
        .map(|s| {
            axum::http::StatusCode::from_u16(s as u16)
                .unwrap_or(axum::http::StatusCode::BAD_GATEWAY)
        })
        .unwrap_or(axum::http::StatusCode::BAD_GATEWAY);

    (status, error_msg).into_response()
}

/// 过滤不应转发的 HTTP headers
fn filter_headers(headers: &HeaderMap) -> serde_json::Value {
    let mut header_map = serde_json::Map::new();

    for (key, value) in headers.iter() {
        let key_str = key.as_str();

        // 过滤掉代理特有的和 HTTP/1.1 逃逸的头
        if matches!(
            key_str.to_lowercase().as_str(),
            "connection"
                | "keep-alive"
                | "proxy-authenticate"
                | "proxy-authorization"
                | "te"
                | "trailers"
                | "transfer-encoding"
                | "upgrade"
                | "host"
        ) {
            continue;
        }

        if let Ok(value_str) = value.to_str() {
            // Go 格式：headers 的值必须是数组，匹配 Go 的 map[string][]string
            header_map.insert(
                key_str.to_string(),
                serde_json::json!([value_str]), // 包装成数组
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
    let auth_api_key =
        std::env::var("AUTH_API_KEY").unwrap_or_else(|_| "your_set_api_key_here".to_string());

    let state = Arc::new(AppState::new(auth_api_key));

    // 构建路由
    let app = Router::new()
        .route("/v1/ws", get(websocket_handler))
        .route("/v1/*path", post(proxy_handler))
        .route("/*path", post(proxy_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    info!("Starting server on {}", PROXY_LISTEN_ADDR);
    info!(
        "WebSocket endpoint available at ws://{}{}",
        PROXY_LISTEN_ADDR, WS_PATH
    );
    info!("HTTP proxy available at http://{}/", PROXY_LISTEN_ADDR);

    let listener = tokio::net::TcpListener::bind(PROXY_LISTEN_ADDR)
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}
