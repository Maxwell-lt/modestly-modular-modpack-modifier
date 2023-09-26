use std::{sync::Arc, time::Duration};

use ratelimit::Ratelimiter;
use thiserror::Error;
use ureq::{Agent, AgentBuilder, Middleware};

pub const USER_AGENT: &str = const_format::formatcp!("modestly-modular-modpack-modifier/{} ureq", env!("CARGO_PKG_VERSION"));

#[derive(Error, Debug)]
pub enum ArchiveDownloadError {
    #[error("Failed to download archive from URL {0}. Error: {1}")]
    Download(String, ureq::Error),
    #[error("Failed to read downloaded archive to bytes. Error: {0}")]
    Read(std::io::Error),
}

pub fn download_archive(url: &str) -> Result<Vec<u8>, ArchiveDownloadError> {
    let response = ureq::get(&url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| ArchiveDownloadError::Download(url.into(), e))?;
    let mut archive = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut archive)
        .map_err(|e| ArchiveDownloadError::Read(e))?;
    Ok(archive)
}

#[derive(Clone)]
pub struct ApiClient {
    inner: Arc<Inner>,
}

struct Inner {
    ratelimit: Ratelimiter,
    client: Agent,
    base_url: String,
}

pub struct ApiClientBuilder {
    requests_per_minute: u64,
    base_url: String,
    agent_builder: AgentBuilder,
}

impl ApiClientBuilder {
    pub fn new(requests_per_minute: u64, base_url: String) -> ApiClientBuilder {
        ApiClientBuilder {
            requests_per_minute,
            base_url,
            agent_builder: AgentBuilder::new().user_agent(USER_AGENT).timeout(Duration::from_secs(60)),
        }
    }

    pub fn add_middleware(mut self, middleware: impl Middleware) -> Self {
        // Is this seriously the best way to handle a builder that takes mut self?
        let mut dummy = AgentBuilder::new();
        std::mem::swap(&mut self.agent_builder, &mut dummy);
        let mut dummy = dummy.middleware(middleware);
        std::mem::swap(&mut self.agent_builder, &mut dummy);
        self
    }

    pub fn build(self) -> ApiClient {
        // Unwrap: The two scenarios in which .build() returns Err are:
        // 1. max_tokens < refill_amount
        // Not possible: both values are equal (set to requests_per_minute)
        // 2. refill_interval > u64::MAX nanoseconds
        // Not possible: refill_interval is hardcoded to 60 seconds (6e10ns < 1.8e19ns)
        // Therefore, this is effectively infallible.
        let ratelimit = Ratelimiter::builder(self.requests_per_minute, Duration::from_secs(60))
            .max_tokens(self.requests_per_minute)
            .initial_available(self.requests_per_minute)
            .build()
            .unwrap();
        let client_builder = AgentBuilder::new().user_agent(USER_AGENT).timeout(Duration::from_secs(60));
        let client = client_builder.build();
        ApiClient {
            inner: Arc::new(Inner {
                ratelimit,
                client,
                base_url: self.base_url,
            }),
        }
    }
}

impl ApiClient {
    fn build_url(&self, path: &str) -> String {
        format!("{}{}", self.inner.base_url, path)
    }

    pub fn get<'a, P>(&self, path: &str, params: P) -> Result<ureq::Response, ureq::Error>
    where
        P: IntoIterator<Item = (&'a str, &'a str)>,
    {
        self.inner.client.get(&self.build_url(path)).query_pairs(params).call()
    }

    pub fn post_json<T>(&self, path: &str, body: T) -> Result<ureq::Response, ureq::Error>
    where
        T: serde::ser::Serialize,
    {
        self.inner.client.post(&format!("{}{}", &self.inner.base_url, path)).send_json(body)
    }
}
