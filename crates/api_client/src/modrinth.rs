use self::model::Project;

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

    pub fn get_mod_info(&self, id_or_slug: &str) -> Option<Project> {
        self.client
            .get(&format!("/project/{id_or_slug}"), vec![])
            .ok()
            .and_then(|r| r.into_json().ok())
    }
}

impl Default for ModrinthClient {
    fn default() -> Self {
        Self::new()
    }
}

mod model {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Project {
        pub slug: String,
        pub title: String,
        pub client_side: Sided,
        pub server_side: Sided,
    }

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum Sided {
        Required,
        Optional,
        Unsupported,
    }
}

#[cfg(test)]
mod tests {
    static APPLESKIN_ID: &str = "EsAfCjCV";
    use crate::modrinth::model::Sided;

    use super::*;

    #[test]
    fn get_mod_info() {
        let client = ModrinthClient::new();
        let project = client.get_mod_info(APPLESKIN_ID).unwrap();
        assert_eq!(project.slug, "appleskin");
        assert_eq!(project.title, "AppleSkin");
        assert_eq!(project.client_side, Sided::Required);
        assert_eq!(project.server_side, Sided::Required);
    }
}
