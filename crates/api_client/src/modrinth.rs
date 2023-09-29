use crate::common::ApiError;

use self::model::{Project, Version};

use super::common::{ApiClient, ApiClientBuilder};

static MODRINTH_BASE_URL: &str = "https://api.modrinth.com/v2";

#[derive(Clone)]
pub struct ModrinthClient {
    client: ApiClient,
}

/// API client for Modrinth.
///
/// This client applies a rate limit to requests, so applications should instantiate a single
/// [`ModrinthClient`] and clone copies as needed, to ensure the rate limits are not exceeded.
/// Internal fields are contained within a [`std::sync::Arc`].
///
/// # Examples
///
/// ```
/// use api_client::modrinth::*;
/// use api_client::modrinth::model::*;
/// // Basic usage
/// let client = ModrinthClient::new();
/// let mod_info: Project = client.get_mod_info("EsAfCjCV").unwrap();
/// assert_eq!(mod_info.title, "AppleSkin");
///
/// // Clone a single client to ensure rate limits are maintained
/// let client2 = client.clone();
/// std::thread::spawn(move || {
///     client2.get_version("Tsz4BT2X").unwrap();
/// });
/// ```
impl ModrinthClient {
    /// Get a [`ModrinthClient`] that uses the official Modrinth API.
    pub fn new() -> Self {
        // Modrinth has a documented rate limit of 300 requests per minute.
        // Using a slightly lower limit of 295 to avoid having to deal with rate limit headers.
        ModrinthClient {
            client: ApiClientBuilder::new(295, MODRINTH_BASE_URL.to_owned()).build(),
        }
    }

    /// Get mod info from Modrinth, given either a project slug or base-62 numeric ID.
    ///
    /// Endpoint: /project/{id|slug}
    pub fn get_mod_info(&self, id_or_slug: &str) -> Result<Project, ApiError> {
        self.client
            .get(&format!("/project/{id_or_slug}"), vec![])
            .map_err(ApiError::Request)?
            .into_json()
            .map_err(ApiError::JsonDeserialize)
    }

    /// Get version list of a mod from Modrinth, given either a project slug or base-62 numeric ID.
    /// Can optionally filter by mod loader and game version.
    ///
    /// Endpoint: /project/{id|slug}/versions
    pub fn get_mod_versions(&self, id_or_slug: &str, loaders: Option<&[&str]>, game_versions: Option<&[&str]>) -> Result<Vec<Version>, ApiError> {
        let mut params = vec![];
        if let Some(l) = format_params(loaders) {
            println!("Adding loaders");
            params.push(("loaders", l));
        }
        if let Some(g) = format_params(game_versions) {
            println!("Adding game_version");
            params.push(("game_versions", g));
        }
        self.client
            .get(
                &format!("/project/{id_or_slug}/version"),
                params
                    .iter()
                    // Convert a Vec<(&str, String)> to a Vec<(&str, &str)> to match ureq's API
                    .map(|&(x, ref y)| (x, &y[..]))
                    .collect::<Vec<_>>(),
            )
            .map_err(ApiError::Request)?
            .into_json()
            .map_err(ApiError::JsonDeserialize)
    }

    /// Get single version from Modrinth, given its base-62 numeric ID.
    ///
    /// Endpoint: /version/{id}
    pub fn get_version(&self, id: &str) -> Result<Version, ApiError> {
        self.client
            .get(&format!("/version/{id}"), vec![])
            .map_err(ApiError::Request)?
            .into_json()
            .map_err(ApiError::JsonDeserialize)
    }
}

/// Format list of items for use as an array query parameter.
/// For query parameters that are accepted as an array,
/// Modrinth requires this formatting: `["forge","fabric","quilt"].`
fn format_params(params: Option<&[&str]>) -> Option<String> {
    params
        .and_then(|a| a.iter().map(|l| format!("\"{l}\"")).reduce(|acc, loader| format!("{acc},{loader}")))
        .map(|a| format!("[{a}]"))
}

impl Default for ModrinthClient {
    fn default() -> Self {
        Self::new()
    }
}

pub mod model {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Project {
        pub slug: String,
        pub title: String,
        pub client_side: Sided,
        pub server_side: Sided,
        pub id: String,
    }

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum Sided {
        Required,
        Optional,
        Unsupported,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Version {
        pub name: String,
        pub version_number: String,
        pub game_versions: Vec<String>,
        pub version_type: VersionType,
        pub loaders: Vec<String>,
        pub id: String,
        pub project_id: String,
        pub files: Vec<VersionFile>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct VersionFile {
        pub hashes: VersionFileHashes,
        pub url: String,
        pub filename: String,
        pub primary: bool,
        pub size: u64,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct VersionFileHashes {
        pub sha512: String,
        pub sha1: String,
    }

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum VersionType {
        Release,
        Beta,
        Alpha,
    }
}

#[cfg(test)]
mod tests {
    static APPLESKIN_ID: &str = "EsAfCjCV";
    static APPLESKIN_1_12_VERSION_ID: &str = "Tsz4BT2X";
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

    #[test]
    fn mod_info_404() {
        let client = ModrinthClient::new();
        let project = client.get_mod_info("this-mod-does-not-exist-abcdefg");
        let err = project.unwrap_err();
        if let ApiError::Request(request_error) = err {
            assert_eq!(request_error.into_response().unwrap().status(), 404);
        } else {
            assert!(false, "Expected error from API request");
        }
    }

    #[test]
    fn get_mod_versions() {
        let client = ModrinthClient::new();
        let versions = client.get_mod_versions(APPLESKIN_ID, None, None).unwrap();
        assert_eq!(
            versions
                .iter()
                .filter(|v| v.game_versions.iter().any(|v| v == "1.12.2"))
                .next()
                .unwrap()
                .version_number,
            "1.0.14+mc1.12"
        );
    }

    #[test]
    fn test_format_params() {
        assert_eq!(format_params(Some(&["1.12.2"])).unwrap(), "[\"1.12.2\"]");
        assert_eq!(format_params(Some(&["fabric", "quilt"])).unwrap(), "[\"fabric\",\"quilt\"]");
    }

    #[test]
    fn filter_versions() {
        let client = ModrinthClient::new();
        let versions = client.get_mod_versions(APPLESKIN_ID, Some(&["forge"]), Some(&["1.12.2"])).unwrap();
        assert_eq!(versions.len(), 1);
    }

    #[test]
    fn get_version() {
        let client = ModrinthClient::new();
        let version = client.get_version(APPLESKIN_1_12_VERSION_ID).unwrap();
        assert_eq!(version.version_number, "1.0.14+mc1.12");
    }
}
