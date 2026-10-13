//! Private Centrifugo proxy request and reply contracts.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::types::{SessionId, UserId};

#[derive(Deserialize)]
pub struct ProxyConnectRequest {
    pub client: String,
    #[serde(skip)]
    pub headers: HashMap<String, String>,
}

#[derive(Deserialize)]
pub struct SessionMeta {
    pub session_id: SessionId,
}

#[derive(Deserialize)]
pub struct ProxyRefreshRequest {
    pub client: String,
    pub user: UserId,
    pub meta: Option<SessionMeta>,
}

#[derive(Serialize)]
pub struct ConnectResult {
    pub user: UserId,
    pub expire_at: i64,
    pub meta: ConnectMeta,
}

#[derive(Serialize)]
pub struct ConnectMeta {
    pub session_id: SessionId,
}

#[derive(Serialize)]
pub struct Disconnect {
    pub code: u32,
    pub reason: &'static str,
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum ConnectReply {
    Accepted { result: ConnectResult },
    Denied { disconnect: Disconnect },
}

#[derive(Serialize)]
pub struct RefreshReply {
    pub result: RefreshResult,
}

#[derive(Serialize)]
pub struct RefreshResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expire_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expired: Option<bool>,
}
