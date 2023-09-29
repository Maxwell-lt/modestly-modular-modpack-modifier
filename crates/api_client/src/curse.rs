use crate::common::ApiError;

use self::model::{File, Mod, Wrapper};

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

    /// Find a mod by its slug.
    /// The Curseforge API guarantees a unique result when searching a combination of game ID,
    /// class ID, and slug, so this function unpacks the API response to a single [`Mod`].
    ///
    /// Endpoint: /mods/search
    pub fn find_mod_by_slug(&self, slug: &str) -> Result<Mod, ApiError> {
        let params = Vec::from([
            ("gameId", "432"),  // Minecraft
            ("classesId", "6"), // Mods
            ("slug", slug),
        ]);
        self.client
            .get("/mods/search", params)
            .map_err(ApiError::Request)?
            .into_json::<Wrapper<Vec<Mod>>>()
            .map_err(ApiError::JsonDeserialize)?
            .data
            .pop()
            .ok_or(ApiError::Empty)
    }

    /// Get list of files for a mod.
    ///
    /// Endpoint: /mods/{id}/files
    pub fn get_mod_files(&self, id: u32) -> Result<Vec<File>, ApiError> {
        let mut files: Vec<File> = Vec::new();
        let mut index = 0;
        loop {
            let mut response = self
                .client
                .get(&format!("/mods/{id}/files"), [("index", index.to_string().as_str())])
                .map_err(ApiError::Request)?
                .into_json::<Wrapper<Vec<File>>>()
                .map_err(ApiError::JsonDeserialize)?;

            files.append(&mut response.data);
            let pagination = response.pagination.ok_or(ApiError::Pagination)?;
            // Check if this page has less than page_size elements, indicating the
            // final page.
            // Otherwise, increment the index to view the next page.
            // NOTE: If the page size evenly divides the number of elements, this check
            // will make an extra call to read the first page with 0 elements.
            if pagination.result_count < pagination.page_size {
                break;
            } else {
                index += pagination.page_size;
            }
        }
        Ok(files)
    }
}

pub mod model {
    use serde::{Deserialize, Serialize};
    use serde_repr::{Deserialize_repr, Serialize_repr};
    #[derive(Serialize, Deserialize, Debug)]
    pub struct Wrapper<T> {
        pub data: T,
        // Sometimes the pagination element is missing
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

    #[derive(Serialize, Deserialize, Debug)]
    pub struct Mod {
        pub id: u32,
        pub name: String,
        pub slug: String,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct File {
        pub id: u32,
        #[serde(rename = "modId")]
        pub mod_id: u32,
        #[serde(rename = "displayName")]
        pub display_name: String,
        #[serde(rename = "fileName")]
        pub file_name: String,
        #[serde(rename = "releaseType")]
        pub release_type: FileReleaseType,
        #[serde(rename = "fileStatus")]
        pub file_status: FileStatus,
        #[serde(rename = "downloadUrl")]
        pub download_url: String,
        #[serde(rename = "gameVersions")]
        pub game_versions: Vec<String>,
        pub dependencies: Vec<FileDependency>,
    }

    #[derive(Serialize_repr, Deserialize_repr, Debug, PartialEq, Eq)]
    #[repr(u8)]
    pub enum FileReleaseType {
        Release = 1,
        Beta = 2,
        Alpha = 3,
    }

    #[derive(Serialize_repr, Deserialize_repr, Debug, PartialEq, Eq)]
    #[repr(u8)]
    pub enum FileStatus {
        Processing = 1,
        ChangesRequired = 2,
        UnderReview = 3,
        Approved = 4,
        Rejected = 5,
        MalwareDetected = 6,
        Deleted = 7,
        Archived = 8,
        Testing = 9,
        Released = 10,
        ReadyForReview = 11,
        Deprecated = 12,
        Baking = 13,
        AwaitingPublishing = 14,
        FailedPublishing = 15,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct FileDependency {
        #[serde(rename = "modId")]
        pub mod_id: u32,
        #[serde(rename = "relationType")]
        pub relation_type: FileRelationType,
    }

    #[derive(Serialize_repr, Deserialize_repr, Debug)]
    #[repr(u8)]
    pub enum FileRelationType {
        EmbeddedLibrary = 1,
        OptionalDependency = 2,
        RequiredDependency = 3,
        Tool = 4,
        Incompatible = 5,
        Include = 6,
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;
    use std::io::Read;
    use toml;

    static APPLESKIN_ID: u32 = 248787;

    use crate::curse::model::FileReleaseType;

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
        let mut file = std::fs::File::open("mmmm.toml").ok()?;
        let mut data = String::new();
        file.read_to_string(&mut data).ok()?;
        toml::from_str::<Config>(&data).ok()
    }

    #[test]
    fn search_mods() {
        let client = get_client();
        let result = client.find_mod_by_slug("appleskin").unwrap();
        assert_eq!(result.id, APPLESKIN_ID);
        assert_eq!(result.name, "AppleSkin");
    }

    #[test]
    fn list_mod_files() {
        let client = get_client();
        let result = client.get_mod_files(APPLESKIN_ID).unwrap();
        // AppleSkin has 102 files as of 2023-09-29, assuming this will not decrease
        assert!(result.len() > 100);
        let file = result.iter().filter(|f| f.id == 2322922).next().unwrap();
        assert_eq!(file.file_name, "AppleSkin-mc1.10.2-1.0.1.jar");
        assert_eq!(file.dependencies.len(), 0);
        assert_eq!(file.release_type, FileReleaseType::Release);
    }
}
