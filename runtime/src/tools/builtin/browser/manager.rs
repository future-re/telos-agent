use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::Mutex;

use super::session::BrowserSession;
use super::util::{browser_session_key, optional_string_array};
use crate::error::AgentError;
use crate::tools::api::ToolContext;

pub(super) type SharedSession = Arc<Mutex<BrowserSession>>;

#[derive(Clone, Default)]
pub struct BrowserManager {
    sessions: Arc<Mutex<HashMap<String, SharedSession>>>,
}

impl BrowserManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub(super) async fn get(&self, key: &str) -> Option<SharedSession> {
        self.sessions.lock().await.get(key).cloned()
    }

    pub(super) async fn start_or_update(
        &self,
        arguments: &Value,
        context: &ToolContext,
    ) -> Result<(String, SharedSession, bool), AgentError> {
        let key = browser_session_key(arguments, context);
        let allowed_domains = optional_string_array(arguments, "allowed_domains")?;
        let prohibited_domains = optional_string_array(arguments, "prohibited_domains")?;

        if let Some(session) = self.get(&key).await {
            {
                let mut session = session.lock().await;
                if let Some(domains) = allowed_domains {
                    session.allowed_domains = domains;
                }
                if let Some(domains) = prohibited_domains {
                    session.prohibited_domains = domains;
                }
            }
            return Ok((key, session, false));
        }

        let session = BrowserSession::start(&key, arguments, context).await?;
        let shared = Arc::new(Mutex::new(session));
        self.sessions.lock().await.insert(key.clone(), shared.clone());
        Ok((key, shared, true))
    }

    pub(super) async fn remove(&self, key: &str) -> Option<SharedSession> {
        self.sessions.lock().await.remove(key)
    }
}
