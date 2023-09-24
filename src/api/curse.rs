use std::sync::Arc;

use ratelimit::Ratelimiter;
use ureq::Agent;

pub struct CurseClient {
    ratelimit: Arc<Ratelimiter>,
    client: Arc<Agent>,
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
