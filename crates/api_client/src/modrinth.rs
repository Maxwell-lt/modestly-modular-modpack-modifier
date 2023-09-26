use super::common::{ApiClient, ApiClientBuilder};

static MODRINTH_BASE_URL: &str = "https://api.modrinth.com/v2";

#[derive(Clone)]
pub struct ModrinthClient {
    client: ApiClient,
}

impl ModrinthClient {
    /// Get a [`ModrinthClient`] that uses the official Modrinth API.
    pub fn new() -> Self {
        // Modrinth has a documented rate limit of 300 requests per minute.
        // Using a slightly lower limit of 295 to avoid having to deal with rate limit headers.
        ModrinthClient {
            client: ApiClientBuilder::new(295, MODRINTH_BASE_URL.to_owned()).build(),
        }
    }
}
