use std::{sync::Arc, time::Duration};

use ratelimit::Ratelimiter;
use ureq::{Agent, AgentBuilder};

use super::common::USER_AGENT;

static MODRINTH_BASE_URL: &str = "https://api.modrinth.com/v2";

#[derive(Clone)]
pub struct ModrinthClient {
    ratelimit: Arc<Ratelimiter>,
    client: Arc<Agent>,
    base_url: Arc<String>,
}

/// Modrinth has a documented rate limit of 300 requests per minute.
/// Using a slightly lower limit to avoid having to deal with rate limit headers.
fn get_ratelimit() -> Ratelimiter {
    Ratelimiter::builder(295, Duration::from_secs(60))
        .max_tokens(295)
        .initial_available(295)
        .build()
        .unwrap()
}

impl ModrinthClient {
    /// Get a [`ModrinthClient`] that uses the official Modrinth API.
    pub fn new() -> Self {
        ModrinthClient {
            ratelimit: Arc::new(get_ratelimit()),
            client: Arc::new(AgentBuilder::new().user_agent(USER_AGENT).build()),
            base_url: Arc::new(MODRINTH_BASE_URL.to_owned()),
        }
    }
}
