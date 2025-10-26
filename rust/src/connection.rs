//! Connection pool management for WebSocket connections

use crate::message::WSMessage;
use dashmap::DashMap;
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::sync::{mpsc, RwLock};

/// Represents a single WebSocket connection with metadata
#[derive(Debug)]
pub struct UserConnection {
    pub user_id: String,
    pub last_active: SystemTime,
    pub sender: mpsc::UnboundedSender<WSMessage>,
}

impl UserConnection {
    pub fn new(user_id: String, sender: mpsc::UnboundedSender<WSMessage>) -> Self {
        Self {
            user_id,
            last_active: SystemTime::now(),
            sender,
        }
    }

    /// Send a message to this connection
    pub async fn send_message(
        &self,
        message: WSMessage,
    ) -> Result<(), mpsc::error::SendError<WSMessage>> {
        self.sender.send(message).map_err(|_| {
            mpsc::error::SendError(WSMessage {
                id: "".to_string(),
                r#type: "error".to_string(),
                payload: serde_json::Value::Null,
            })
        })
    }

    /// Check if connection is still active (within timeout)
    pub fn is_active(&self, timeout: Duration) -> bool {
        SystemTime::now()
            .duration_since(self.last_active)
            .map(|d| d < timeout)
            .unwrap_or(false)
    }

    /// Update last active time
    pub fn update_activity(&mut self) {
        self.last_active = SystemTime::now();
    }
}

/// Manages connections for a single user
#[derive(Debug)]
pub struct UserConnections {
    pub connections: Vec<Arc<UserConnection>>,
    pub next_index: usize,
}

impl UserConnections {
    pub fn new() -> Self {
        Self {
            connections: Vec::new(),
            next_index: 0,
        }
    }

    pub fn add_connection(&mut self, connection: Arc<UserConnection>) {
        self.connections.push(connection);
    }

    pub fn remove_connection(&mut self, user_id: &str) {
        self.connections.retain(|conn| conn.user_id != user_id);
    }

    /// Get next connection using round-robin load balancing
    pub fn get_next_connection(&mut self) -> Option<Arc<UserConnection>> {
        if self.connections.is_empty() {
            return None;
        }
        let index = self.next_index % self.connections.len();
        self.next_index = (self.next_index + 1) % self.connections.len();
        Some(self.connections[index].clone())
    }

    /// Clean up inactive connections
    pub fn cleanup_inactive(&mut self, timeout: Duration) {
        self.connections.retain(|conn| conn.is_active(timeout));
    }

    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }

    pub fn len(&self) -> usize {
        self.connections.len()
    }
}

/// Global connection pool for managing all WebSocket connections
#[derive(Debug, Clone)]
pub struct ConnectionPool {
    pub users: Arc<DashMap<String, Arc<RwLock<UserConnections>>>>,
    pub pending_requests: Arc<DashMap<String, mpsc::UnboundedSender<WSMessage>>>,
}

impl ConnectionPool {
    pub fn new() -> Self {
        Self {
            users: Arc::new(DashMap::new()),
            pending_requests: Arc::new(DashMap::new()),
        }
    }

    /// Add a new connection to the pool
    pub async fn add_connection(
        &self,
        user_id: String,
        sender: mpsc::UnboundedSender<WSMessage>,
    ) -> Arc<UserConnection> {
        let connection = Arc::new(UserConnection::new(user_id.clone(), sender));

        let user_connections = self
            .users
            .entry(user_id.clone())
            .or_insert_with(|| Arc::new(RwLock::new(UserConnections::new())));

        {
            let mut connections = user_connections.write().await;
            connections.add_connection(connection.clone());
        }

        tracing::info!(
            "WebSocket connected: UserID={}, Total connections: {}",
            user_id,
            user_connections.read().await.connections.len()
        );

        connection
    }

    /// Remove a connection from the pool
    pub async fn remove_connection(&self, user_id: &str, connection_id: &str) {
        if let Some(user_connections) = self.users.get(user_id) {
            let mut connections = user_connections.write().await;
            connections.remove_connection(connection_id);

            if connections.is_empty() {
                self.users.remove(user_id);
            }
        }
    }

    /// Get a connection for a user using load balancing
    pub async fn get_connection(&self, user_id: &str) -> Option<Arc<UserConnection>> {
        if let Some(user_connections) = self.users.get(user_id) {
            let mut connections = user_connections.write().await;
            connections.get_next_connection()
        } else {
            None
        }
    }

    /// Register a pending request
    pub fn register_pending_request(
        &self,
        request_id: String,
        sender: mpsc::UnboundedSender<WSMessage>,
    ) {
        self.pending_requests.insert(request_id, sender);
    }

    /// Remove a pending request
    pub fn remove_pending_request(&self, request_id: &str) {
        self.pending_requests.remove(request_id);
    }

    /// Send response to pending request
    pub fn send_response(&self, request_id: &str, message: WSMessage) -> bool {
        if let Some(sender) = self.pending_requests.get(request_id) {
            sender.value().send(message).is_ok()
        } else {
            false
        }
    }

    /// Clean up inactive connections
    pub async fn cleanup_inactive(&self, timeout: Duration) {
        let mut to_remove = Vec::new();

        for entry in self.users.iter() {
            let mut connections = entry.value().write().await;
            connections.cleanup_inactive(timeout);

            if connections.is_empty() {
                to_remove.push(entry.key().clone());
            }
        }

        for user_id in to_remove {
            self.users.remove(&user_id);
        }
    }

    /// Get connection statistics
    pub async fn get_stats(&self) -> ConnectionStats {
        let mut total_connections = 0;
        let mut total_users = 0;

        for entry in self.users.iter() {
            let connections = entry.value().read().await;
            total_connections += connections.len();
            total_users += 1;
        }

        ConnectionStats {
            total_connections,
            total_users,
            pending_requests: self.pending_requests.len(),
        }
    }
}

/// Connection pool statistics
#[derive(Debug, Clone)]
pub struct ConnectionStats {
    pub total_connections: usize,
    pub total_users: usize,
    pub pending_requests: usize,
}
