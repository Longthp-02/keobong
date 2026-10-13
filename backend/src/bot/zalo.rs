//! Adapter: sends replies through the Zalo Bot API.
//! The bot token is part of every request URL, so URLs never reach logs or errors.

use std::time::Duration;

use serde_json::{Value, json};

use super::domain::{ChatReply, SendError, SendFuture};
use crate::config::Secret;

const API_BASE: &str = "https://bot-api.zaloplatforms.com";

pub struct ZaloReply {
    http: reqwest::Client,
    token: Secret,
}

impl ZaloReply {
    pub fn new(token: Secret) -> Result<Self, reqwest::Error> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self { http, token })
    }
}

impl ChatReply for ZaloReply {
    fn send<'a>(&'a self, chat_id: &'a str, text: &'a str) -> SendFuture<'a> {
        Box::pin(async move {
            let url = format!("{API_BASE}/bot{}/sendMessage", self.token.expose());
            let response = self
                .http
                .post(url)
                .json(&json!({ "chat_id": chat_id, "text": text }))
                .send()
                .await
                .map_err(|error| SendError(error.without_url().to_string()))?;
            let status = response.status();
            let body: Value = response
                .json()
                .await
                .map_err(|error| SendError(error.without_url().to_string()))?;
            if !status.is_success() || body["ok"] != true {
                return Err(SendError(format!(
                    "status {status}, error_code {}",
                    body["error_code"]
                )));
            }
            Ok(())
        })
    }
}
