use std::{sync::Arc, time::Duration};

use ratelimit::Ratelimiter;
use ureq::{Agent, AgentBuilder, MiddlewareNext, Request};

use super::common::USER_AGENT;

static CURSEFORGE_BASE_URL: &str = "https://api.curseforge.com";

#[derive(Clone)]
pub struct CurseClient {
    ratelimit: Arc<Ratelimiter>,
    client: Arc<Agent>,
    base_url: Arc<String>,
}

/// CurseForge does not document any rate limits, so I just made something up.
fn get_ratelimit() -> Ratelimiter {
    Ratelimiter::builder(1000, Duration::from_secs(60))
        .max_tokens(1000)
        .initial_available(1000)
        .build()
        .unwrap()
}

impl CurseClient {
    /// Get a [`CurseClient`] that uses the official CurseForge API.
    pub fn from_key(key: String) -> Self {
        CurseClient {
            ratelimit: Arc::new(get_ratelimit()),
            client: Arc::new(
                AgentBuilder::new()
                    .user_agent(USER_AGENT)
                    .middleware(move |req: Request, next: MiddlewareNext| {
                        next.handle(req.set("x-api-key", &key))
                    })
                    .build(),
            ),
            base_url: Arc::new(CURSEFORGE_BASE_URL.to_owned()),
        }
    }

    /// Get a [`CurseClient`] that uses a proxy service, and does not require an API key.
    pub fn from_proxy(proxy_url: &str) -> Self {
        CurseClient {
            ratelimit: Arc::new(get_ratelimit()),
            client: Arc::new(AgentBuilder::new().user_agent(USER_AGENT).build()),
            base_url: Arc::new(proxy_url.to_owned()),
        }
    }
}

pub mod model {
    use serde::{Deserialize, Serialize};
    #[derive(Serialize, Deserialize, Debug)]
    pub struct Wrapper<T> {
        pub data: T,
        pub pagination: Option<Pagination>,
    }
    #[derive(Serialize, Deserialize, Debug)]
    pub struct Pagination {
        pub index: u32,
        #[serde(rename = "pageSize")]
        pub page_size: u32,
        #[serde(rename = "resultCount")]
        pub result_count: u32,
        #[serde(rename = "totalCount")]
        pub total_count: u32,
    }
}
