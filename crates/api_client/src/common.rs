use std::{sync::Arc, time::Duration};

use lazy_static::lazy_static;
use ratelimit::Ratelimiter;
use thiserror::Error;
use ureq::{Agent, AgentBuilder, Middleware};

pub const USER_AGENT: &str = const_format::formatcp!("modestly-modular-modpack-modifier/{} ureq", env!("CARGO_PKG_VERSION"));

lazy_static! {
    static ref AGENT: Agent = AgentBuilder::new().user_agent(USER_AGENT).build();
}

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("Failed to read response to bytes. Error: {0}")]
    Read(std::io::Error),
    #[error("Failed to download file from URL {0}. Error: {1}")]
    Download(String, Box<ureq::Error>),
}

#[derive(Error, Debug)]
pub enum ApiError {
    #[error("Failed to deserialize JSON response. Error: {0}")]
    JsonDeserialize(#[from] std::io::Error),
    #[error("Request failed. Error: {0}")]
    Request(#[from] Box<ureq::Error>),
    #[error("Expected pagination info in response was missing.")]
    Pagination,
    #[error("No data returned from request.")]
    Empty,
}

pub fn download_file(url: &str) -> Result<Vec<u8>, DownloadError> {
    let mut response = Vec::new();
    AGENT
        .get(url)
        .call()
        .map_err(|e| DownloadError::Download(url.to_owned(), Box::new(e)))?
        .into_reader()
        .read_to_end(&mut response)
        .map_err(DownloadError::Read)?;
    Ok(response)
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
        self.agent_builder = std::mem::replace(&mut self.agent_builder, AgentBuilder::new()).middleware(middleware);
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
        let client_builder = self.agent_builder.timeout(Duration::from_secs(60));
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

    /// Wait for ratelimiter to allow an API call.
    ///
    /// Must be called before every use of the [`Agent`]
    fn wait_for_token(&self) {
        while let Err(duration) = self.inner.ratelimit.try_wait() {
            std::thread::sleep(duration);
        }
    }

    pub fn get<'a, P>(&self, path: &str, params: P) -> Result<ureq::Response, Box<ureq::Error>>
    where
        P: IntoIterator<Item = (&'a str, &'a str)> + Clone,
    {
        let mut retries = 2;
        loop {
            self.wait_for_token();
            match self.inner.client.get(&self.build_url(path)).query_pairs(params.clone()).call().map_err(Box::new) {
                Ok(response) => return Ok(response),
                Err(err) => {
                    if retries > 0 {
                        retries -= 1;
                    } else {
                        return Err(err);
                    }
                },
            }
        }
    }

    pub fn post_json<T>(&self, path: &str, body: T) -> Result<ureq::Response, Box<ureq::Error>>
    where
        T: serde::ser::Serialize,
    {
        self.wait_for_token();
        self.inner.client.post(&self.build_url(path)).send_json(body).map_err(Box::new)
    }
}
