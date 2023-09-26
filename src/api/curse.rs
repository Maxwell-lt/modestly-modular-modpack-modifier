use super::common::{ApiClient, ApiClientBuilder, USER_AGENT};

static CURSEFORGE_BASE_URL: &str = "https://api.curseforge.com";

#[derive(Clone)]
pub struct CurseClient {
    client: ApiClient,
}

impl CurseClient {
    /// Get a [`CurseClient`] that uses the official CurseForge API.
    pub fn from_key(key: String) -> Self {
        CurseClient {
            // Curseforge does not document any rate limit, trying 1000/min for now
            client: ApiClientBuilder::new(1000, CURSEFORGE_BASE_URL.to_owned())
                .add_middleware(move |req: ureq::Request, next: ureq::MiddlewareNext| next.handle(req.set("x-api-key", &key)))
                .build(),
        }
    }

    /// Get a [`CurseClient`] that uses a proxy service, and does not require an API key.
    pub fn from_proxy(proxy_url: String) -> Self {
        CurseClient {
            client: ApiClientBuilder::new(1000, proxy_url).build(),
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
