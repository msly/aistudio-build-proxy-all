//! WebSocket message structures and handling

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// WebSocket message structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WSMessage {
    pub id: String,
    pub r#type: String,
    pub payload: serde_json::Value,
}

impl WSMessage {
    pub fn new(id: String, r#type: String, payload: serde_json::Value) -> Self {
        Self { id, r#type, payload }
    }

    /// Create a ping message
    pub fn ping(id: String) -> Self {
        Self {
            id,
            r#type: "ping".to_string(),
            payload: serde_json::Value::Null,
        }
    }

    /// Create a pong message
    pub fn pong(id: String) -> Self {
        Self {
            id,
            r#type: "pong".to_string(),
            payload: serde_json::Value::Null,
        }
    }

    /// Create an HTTP request message
    pub fn http_request(id: String, method: String, url: String, headers: HashMap<String, String>, body: String) -> Self {
        Self {
            id,
            r#type: "http_request".to_string(),
            payload: serde_json::json!({
                "method": method,
                "url": url,
                "headers": headers,
                "body": body
            }),
        }
    }

    /// Create an HTTP response message
    pub fn http_response(id: String, status: u16, headers: HashMap<String, String>, body: String) -> Self {
        Self {
            id,
            r#type: "http_response".to_string(),
            payload: serde_json::json!({
                "status": status,
                "headers": headers,
                "body": body
            }),
        }
    }

    /// Create a stream start message
    pub fn stream_start(id: String, headers: HashMap<String, String>) -> Self {
        Self {
            id,
            r#type: "stream_start".to_string(),
            payload: serde_json::json!({
                "headers": headers
            }),
        }
    }

    /// Create a stream chunk message
    pub fn stream_chunk(id: String, data: String) -> Self {
        Self {
            id,
            r#type: "stream_chunk".to_string(),
            payload: serde_json::json!({
                "data": data
            }),
        }
    }

    /// Create a stream end message
    pub fn stream_end(id: String) -> Self {
        Self {
            id,
            r#type: "stream_end".to_string(),
            payload: serde_json::Value::Null,
        }
    }

    /// Create an error message
    pub fn error(id: String, error: String, status: Option<u16>) -> Self {
        let mut payload = serde_json::json!({
            "error": error
        });
        
        if let Some(status) = status {
            payload["status"] = serde_json::Value::Number(serde_json::Number::from(status));
        }
        
        Self {
            id,
            r#type: "error".to_string(),
            payload,
        }
    }
}

/// Message type enumeration for type safety
#[derive(Debug, Clone, PartialEq)]
pub enum MessageType {
    Ping,
    Pong,
    HttpRequest,
    HttpResponse,
    StreamStart,
    StreamChunk,
    StreamEnd,
    Error,
    Unknown(String),
}

impl From<&str> for MessageType {
    fn from(s: &str) -> Self {
        match s {
            "ping" => MessageType::Ping,
            "pong" => MessageType::Pong,
            "http_request" => MessageType::HttpRequest,
            "http_response" => MessageType::HttpResponse,
            "stream_start" => MessageType::StreamStart,
            "stream_chunk" => MessageType::StreamChunk,
            "stream_end" => MessageType::StreamEnd,
            "error" => MessageType::Error,
            _ => MessageType::Unknown(s.to_string()),
        }
    }
}

impl From<&WSMessage> for MessageType {
    fn from(msg: &WSMessage) -> Self {
        MessageType::from(msg.r#type.as_str())
    }
}

/// HTTP request structure for easier handling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl From<&WSMessage> for Option<HttpRequest> {
    fn from(msg: &WSMessage) -> Self {
        if msg.r#type == "http_request" {
            serde_json::from_value(msg.payload.clone()).ok()
        } else {
            None
        }
    }
}

/// HTTP response structure for easier handling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl From<&WSMessage> for Option<HttpResponse> {
    fn from(msg: &WSMessage) -> Self {
        if msg.r#type == "http_response" {
            serde_json::from_value(msg.payload.clone()).ok()
        } else {
            None
        }
    }
}

/// Stream chunk structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    pub data: String,
}

impl From<&WSMessage> for Option<StreamChunk> {
    fn from(msg: &WSMessage) -> Self {
        if msg.r#type == "stream_chunk" {
            serde_json::from_value(msg.payload.clone()).ok()
        } else {
            None
        }
    }
}
