use self::model::{Mod, Wrapper};

use super::common::{ApiClient, ApiClientBuilder};

static CURSEFORGE_BASE_URL: &str = "https://api.curseforge.com/v1";

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

    /// Find a mod by its slug. Will not return more than 1 result. Only searches for results under
    /// the Minecraft game and Mod category.
    pub fn find_mod_by_slug(&self, slug: &str) -> Option<Wrapper<Vec<Mod>>> {
        let params = Vec::from([
            ("gameId", "432"),  // Minecraft
            ("classesId", "6"), // Mods
            ("slug", slug),
        ]);
        self.client.get("/mods/search", params).ok().and_then(|r| r.into_json().ok())
    }
}

pub mod model {
    use serde::{Deserialize, Serialize};
    #[derive(Serialize, Deserialize, Debug)]
    pub struct Wrapper<T> {
        pub data: T,
        pub pagination: Pagination,
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

    #[derive(Serialize, Deserialize, Debug)]
    pub struct Mod {
        pub id: u32,
        pub name: String,
        pub slug: String,
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use std::{fs::File, io::Read};
    use toml;

    use super::*;

    #[derive(Deserialize)]
    struct Config {
        curse: CurseConf,
    }

    #[derive(Deserialize)]
    struct CurseConf {
        api_key: String,
    }

    /// Tries to load a Curse key from file `mmmm.toml`, falls back to using questionable CF proxy.
    fn get_client() -> CurseClient {
        match get_toml() {
            Some(config) => CurseClient::from_key(config.curse.api_key),
            None => CurseClient::from_proxy("https://api.curse.tools/v1/cf".to_string()),
        }
    }

    fn get_toml() -> Option<Config> {
        let mut file = File::open("mmmm.toml").ok()?;
        let mut data = String::new();
        file.read_to_string(&mut data).ok()?;
        toml::from_str::<Config>(&data).ok()
    }

    #[test]
    fn search_mods() {
        let client = get_client();
        let result = client.find_mod_by_slug("appleskin").unwrap();
        assert_eq!(result.pagination.total_count, 1);
        assert_eq!(result.data.len(), 1);
        assert_eq!(result.data[0].id, 248787);
    }
}
