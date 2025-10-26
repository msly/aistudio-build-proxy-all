//! Authentication and authorization functionality

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
};
use std::{collections::HashMap, sync::Arc};
use tracing::{info, warn};

/// Authentication result
#[derive(Debug, Clone)]
pub struct AuthResult {
    pub user_id: String,
    pub is_authenticated: bool,
}

/// WebSocket authentication
pub async fn authenticate_websocket(
    query: Query<HashMap<String, String>>,
    _state: State<Arc<AppState>>,
) -> Result<AuthResult, StatusCode> {
    let auth_token = query.get("auth_token").cloned().unwrap_or_default();

    if auth_token.is_empty() {
        warn!("WebSocket authentication failed: missing auth_token");
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Simple token validation (in production, use proper JWT validation)
    if auth_token == "valid-token-user-1" {
        info!("WebSocket authenticated for user-1");
        Ok(AuthResult {
            user_id: "user-1".to_string(),
            is_authenticated: true,
        })
    } else {
        warn!("WebSocket authentication failed: invalid token");
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// HTTP request authentication
pub async fn authenticate_http_request(
    headers: HeaderMap,
    state: State<Arc<AppState>>,
) -> Result<AuthResult, StatusCode> {
    // Check for API key in headers
    let api_key = headers.get("x-goog-api-key").and_then(|h| h.to_str().ok());

    if api_key != Some(&state.auth_api_key) {
        warn!("HTTP authentication failed: invalid API key");
        return Err(StatusCode::UNAUTHORIZED);
    }

    info!("HTTP request authenticated for user-1");
    Ok(AuthResult {
        user_id: "user-1".to_string(),
        is_authenticated: true,
    })
}

/// Validate JWT token (placeholder for future implementation)
pub fn validate_jwt(token: &str) -> Result<String, String> {
    if token.is_empty() {
        return Err("Missing auth_token".to_string());
    }

    // In production, implement proper JWT validation
    if token == "valid-token-user-1" {
        Ok("user-1".to_string())
    } else {
        Err("Invalid token".to_string())
    }
}

/// Application state for authentication
#[derive(Debug, Clone)]
pub struct AppState {
    pub auth_api_key: String,
}

impl AppState {
    pub fn new(auth_api_key: String) -> Self {
        Self { auth_api_key }
    }
}
