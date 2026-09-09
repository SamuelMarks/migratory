//! Vagrant Cloud API client.
//!
//! This module provides a simple client for interacting with the Vagrant Cloud API,
//! primarily to fetch metadata about boxes.

use crate::error::MigratoryError;
use reqwest::blocking::Client;
use serde::Deserialize;

#[coverage(off)]
fn create_default_client() -> Client {
    Client::builder().build().unwrap_or_else(|_| Client::new())
}

/// Vagrant Cloud client.
///
/// This struct holds the underlying HTTP client and the base URL for the API.
pub struct CloudClient {
    client: Client,
    base_url: String,
    token: Option<String>,
}

/// A box catalog metadata response.
///
/// Represents the high-level metadata of a box in the catalog.
#[derive(Deserialize, Debug)]
pub struct BoxMetadata {
    /// Description of the box.
    pub description_markdown: String,
    /// Short description.
    pub short_description: String,
    /// Box name.
    pub name: String,
    /// List of versions.
    pub versions: Vec<BoxVersion>,
}

/// A version of a box.
///
/// Represents a specific version string and its associated providers.
#[derive(Deserialize, Debug)]
pub struct BoxVersion {
    /// Version string.
    pub version: String,
    /// List of providers.
    pub providers: Vec<BoxProvider>,
}

/// A provider for a box version.
///
/// Represents an available provider (like virtualbox, qemu) and its download URL.
#[derive(Deserialize, Debug)]
pub struct BoxProvider {
    /// Provider name.
    pub name: String,
    /// Download URL.
    pub url: String,
    /// Optional checksum.
    pub checksum: Option<String>,
    /// Optional checksum type (e.g. "sha256").
    pub checksum_type: Option<String>,
}

impl CloudClient {
    /// Creates a new client.
    ///
    /// Initializes an HTTP client configured for Vagrant Cloud API interactions.
    ///
    /// # Returns
    ///
    /// Returns a new instance of `CloudClient` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the underlying HTTP client cannot be built.
    pub fn new() -> Result<Self, MigratoryError> {
        if std::env::var("MIGRATORY_TEST_MOCK_CLIENT_ERROR").is_ok() {
            return Err(MigratoryError::Generic("mock client error".to_string()));
        }
        let client = create_default_client();
        let base_url = std::env::var("VAGRANT_CLOUD_URL")
            .unwrap_or_else(|_| "https://app.vagrantup.com/api/v2".to_string());

        let token = std::env::var("VAGRANT_CLOUD_TOKEN").ok().or_else(|| {
            let home_dir =
                std::env::var("VAGRANT_HOME").unwrap_or_else(|_| ".vagrant.d".to_string());
            let token_path = std::path::Path::new(&home_dir).join("data").join("token");
            std::fs::read_to_string(&token_path)
                .ok()
                .map(|s| s.trim().to_string())
        });

        Ok(Self {
            client,
            base_url,
            token,
        })
    }

    /// Fetches metadata for a given box tag (e.g. "ubuntu/jammy64").
    ///
    /// # Arguments
    ///
    /// * `tag` - The box identifier (e.g., `hashicorp/bionic64`).
    ///
    /// # Returns
    ///
    /// Returns the parsed `BoxMetadata` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if the HTTP request fails, returns a non-success status,
    /// or if the JSON response cannot be parsed.
    pub fn fetch_metadata(&self, tag: &str) -> Result<BoxMetadata, MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/boxes/{}", base, tag);
        let resp = self
            .client
            .get(&url)
            .send()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(MigratoryError::Generic(format!(
                "Failed to fetch: {}",
                resp.status()
            )));
        }
        let metadata: BoxMetadata = resp
            .json()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        Ok(metadata)
    }

    /// Authenticates with Vagrant Cloud using basic auth and returns an access token.
    pub fn authenticate(
        &self,
        username: &str,
        password: &str,
        description: Option<&str>,
    ) -> Result<String, MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/authenticate", base);

        let mut token_obj = serde_json::Map::new();
        let desc = description.unwrap_or("Migratory login");
        token_obj.insert(
            "description".to_string(),
            serde_json::Value::String(desc.to_string()),
        );

        let payload = serde_json::json!({ "token": token_obj });

        #[derive(Deserialize)]
        struct AuthResponse {
            token: String,
        }

        println!("URL: {}", url);
        let resp = self
            .client
            .post(&url)
            .basic_auth(username, Some(password))
            .json(&payload)
            .send()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(MigratoryError::Generic(format!(
                "Authentication failed: {}",
                resp.status()
            )));
        }

        let auth_resp: AuthResponse = resp
            .json()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        Ok(auth_resp.token)
    }

    /// Fetches the currently authenticated user's username.
    pub fn whoami(&self) -> Result<String, MigratoryError> {
        let token = match &self.token {
            Some(t) if !t.is_empty() => t,
            _ => {
                return Err(MigratoryError::Generic(
                    "No authentication token found. Please login first.".to_string(),
                ));
            }
        };

        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/authenticate", base);

        #[derive(Deserialize)]
        struct UserObj {
            username: String,
        }
        #[derive(Deserialize)]
        struct AuthResponse {
            user: UserObj,
        }

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(MigratoryError::Generic(format!(
                "Failed to fetch user info: {} - {}",
                status, text
            )));
        }

        let parsed: AuthResponse = resp
            .json()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        Ok(parsed.user.username)
    }

    /// Revokes the current authentication token.
    pub fn delete_token(&self) -> Result<(), MigratoryError> {
        let token = match &self.token {
            Some(t) if !t.is_empty() => t,
            _ => return Ok(()),
        };

        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/authenticate", base);

        let resp = self
            .client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;

        if !resp.status().is_success() && resp.status() != reqwest::StatusCode::NOT_FOUND {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            return Err(MigratoryError::Generic(format!(
                "Failed to logout (revoke token): {} - {}",
                status, text
            )));
        }

        Ok(())
    }

    /// Creates a box on Vagrant Cloud.
    pub fn create_box(
        &self,
        name: &str,
        description: Option<&str>,
        short_description: Option<&str>,
    ) -> Result<(), MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/boxes", base);

        let mut body = serde_json::Map::new();
        body.insert(
            "name".to_string(),
            serde_json::Value::String(name.to_string()),
        );
        if let Some(desc) = description {
            body.insert(
                "description".to_string(),
                serde_json::Value::String(desc.to_string()),
            );
        }
        if let Some(s_desc) = short_description {
            body.insert(
                "short_description".to_string(),
                serde_json::Value::String(s_desc.to_string()),
            );
        }

        let payload = serde_json::json!({ "box": body });

        println!("URL: {}", url);
        let resp = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(MigratoryError::Generic(format!(
                "Failed to create box: {}",
                resp.status()
            )));
        }
        Ok(())
    }

    /// Deletes a box on Vagrant Cloud.
    pub fn delete_box(&self, name: &str) -> Result<(), MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/boxes/{}", base, name);
        let resp = self
            .client
            .delete(&url)
            .send()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(MigratoryError::Generic(format!(
                "Failed to delete box: {}",
                resp.status()
            )));
        }
        Ok(())
    }

    /// Updates a box on Vagrant Cloud.
    pub fn update_box(
        &self,
        name: &str,
        description: Option<&str>,
        short_description: Option<&str>,
    ) -> Result<(), MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/boxes/{}", base, name);

        let mut body = serde_json::Map::new();
        if let Some(desc) = description {
            body.insert(
                "description".to_string(),
                serde_json::Value::String(desc.to_string()),
            );
        }
        if let Some(s_desc) = short_description {
            body.insert(
                "short_description".to_string(),
                serde_json::Value::String(s_desc.to_string()),
            );
        }

        let payload = serde_json::json!({ "box": body });
        let resp = self
            .client
            .put(&url)
            .json(&payload)
            .send()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(MigratoryError::Generic(format!(
                "Failed to update box: {}",
                resp.status()
            )));
        }
        Ok(())
    }

    /// Creates a version for a box on Vagrant Cloud.
    pub fn create_version(
        &self,
        box_name: &str,
        version: &str,
        description: Option<&str>,
    ) -> Result<(), MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/boxes/{}/versions", base, box_name);

        let mut body = serde_json::Map::new();
        body.insert(
            "version".to_string(),
            serde_json::Value::String(version.to_string()),
        );
        if let Some(desc) = description {
            body.insert(
                "description".to_string(),
                serde_json::Value::String(desc.to_string()),
            );
        }

        let payload = serde_json::json!({ "version": body });

        println!("URL: {}", url);
        let resp = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(MigratoryError::Generic(format!(
                "Failed to create version: {}",
                resp.status()
            )));
        }
        Ok(())
    }

    /// Deletes a version for a box on Vagrant Cloud.
    pub fn delete_version(&self, box_name: &str, version: &str) -> Result<(), MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/boxes/{}/versions/{}", base, box_name, version);

        let resp = self
            .client
            .delete(&url)
            .send()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(MigratoryError::Generic(format!(
                "Failed to delete version: {}",
                resp.status()
            )));
        }
        Ok(())
    }

    /// Revokes a version for a box on Vagrant Cloud.
    pub fn revoke_version(&self, box_name: &str, version: &str) -> Result<(), MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/boxes/{}/versions/{}/revoke", base, box_name, version);

        let resp = self
            .client
            .put(&url)
            .send()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(MigratoryError::Generic(format!(
                "Failed to revoke version: {}",
                resp.status()
            )));
        }
        Ok(())
    }

    /// Updates a version for a box on Vagrant Cloud.
    pub fn update_version(
        &self,
        box_name: &str,
        version: &str,
        description: Option<&str>,
    ) -> Result<(), MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/boxes/{}/versions/{}", base, box_name, version);

        let mut body = serde_json::Map::new();
        if let Some(desc) = description {
            body.insert(
                "description".to_string(),
                serde_json::Value::String(desc.to_string()),
            );
        }

        let payload = serde_json::json!({ "version": body });

        let resp = self
            .client
            .put(&url)
            .json(&payload)
            .send()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(MigratoryError::Generic(format!(
                "Failed to update version: {}",
                resp.status()
            )));
        }
        Ok(())
    }

    /// Creates a provider for a version on Vagrant Cloud.
    pub fn create_provider(
        &self,
        box_name: &str,
        version: &str,
        provider: &str,
    ) -> Result<(), MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/boxes/{}/versions/{}/providers", base, box_name, version);

        let payload = serde_json::json!({ "provider": { "name": provider } });

        println!("URL: {}", url);
        let resp = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(MigratoryError::Generic(format!(
                "Failed to create provider: {}",
                resp.status()
            )));
        }
        Ok(())
    }

    /// Updates a provider for a version on Vagrant Cloud.
    pub fn update_provider(
        &self,
        box_name: &str,
        version: &str,
        provider: &str,
        checksum: Option<&str>,
        checksum_type: Option<&str>,
    ) -> Result<(), MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!(
            "{}/boxes/{}/versions/{}/providers/{}",
            base, box_name, version, provider
        );

        let mut body = serde_json::Map::new();
        if let Some(cs) = checksum {
            body.insert(
                "checksum".to_string(),
                serde_json::Value::String(cs.to_string()),
            );
        }
        if let Some(cst) = checksum_type {
            body.insert(
                "checksum_type".to_string(),
                serde_json::Value::String(cst.to_string()),
            );
        }

        let payload = serde_json::json!({ "provider": body });

        let resp = self
            .client
            .put(&url)
            .json(&payload)
            .send()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(MigratoryError::Generic(format!(
                "Failed to update provider: {}",
                resp.status()
            )));
        }
        Ok(())
    }

    /// Deletes a provider from a version on Vagrant Cloud.
    pub fn delete_provider(
        &self,
        box_name: &str,
        version: &str,
        provider: &str,
    ) -> Result<(), MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!(
            "{}/boxes/{}/versions/{}/providers/{}",
            base, box_name, version, provider
        );

        let resp = self
            .client
            .delete(&url)
            .send()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(MigratoryError::Generic(format!(
                "Failed to delete provider: {}",
                resp.status()
            )));
        }
        Ok(())
    }

    /// Gets the upload URL for a provider on Vagrant Cloud.
    pub fn get_upload_url(
        &self,
        box_name: &str,
        version: &str,
        provider: &str,
    ) -> Result<String, MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!(
            "{}/boxes/{}/versions/{}/providers/{}/upload",
            base, box_name, version, provider
        );

        #[derive(Deserialize)]
        struct UploadResponse {
            upload_path: String,
        }

        let resp = self
            .client
            .get(&url)
            .send()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(MigratoryError::Generic(format!(
                "Failed to get upload URL: {}",
                resp.status()
            )));
        }

        let upload_resp: UploadResponse = resp
            .json()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        Ok(upload_resp.upload_path)
    }

    /// Uploads a file to Vagrant Cloud using the provided upload path.
    pub fn upload_file(
        &self,
        upload_path: &str,
        file_path: &std::path::Path,
    ) -> Result<(), MigratoryError> {
        let file = std::fs::File::open(file_path).map_err(MigratoryError::Io)?;
        let resp = self
            .client
            .put(upload_path)
            .body(file)
            .send()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(MigratoryError::Generic(format!(
                "Failed to upload file: {}",
                resp.status()
            )));
        }
        Ok(())
    }

    /// Releases a version for a box on Vagrant Cloud.
    pub fn release_version(&self, box_name: &str, version: &str) -> Result<(), MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/boxes/{}/versions/{}/release", base, box_name, version);

        let resp = self
            .client
            .put(&url)
            .send()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(MigratoryError::Generic(format!(
                "Failed to release version: {}",
                resp.status()
            )));
        }
        Ok(())
    }

    /// Searches for boxes on Vagrant Cloud.
    pub fn search_boxes(&self, query: &str) -> Result<Vec<BoxMetadata>, MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/search?q={}", base, query);

        #[derive(Deserialize)]
        struct SearchResponse {
            boxes: Vec<BoxMetadata>,
        }

        let resp = self
            .client
            .get(&url)
            .send()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(MigratoryError::Generic(format!(
                "Failed to search boxes: {}",
                resp.status()
            )));
        }

        let search_resp: SearchResponse = resp
            .json()
            .map_err(|e| MigratoryError::Generic(e.to_string()))?;
        Ok(search_resp.boxes)
    }

    /// Helper for testing to inject a different base URL.
    #[cfg(test)]
    fn with_base_url(base_url: String) -> Result<Self, MigratoryError> {
        let client = create_default_client();
        Ok(Self {
            client,
            base_url,
            token: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    #[test]
    fn test_whoami_json_error() {
        let server = MockServer::start();

        let mock_err = server.mock(|when, then| {
            when.method(GET).path("/api/v1/authenticate");
            then.status(200).body("not json");
        });

        let mut client = CloudClient::with_base_url(server.url("/api/v1")).unwrap();
        client.token = Some("valid_token".to_string());

        assert!(client.whoami().is_err());
        mock_err.assert_calls(1);
    }

    #[test]
    fn test_whoami() {
        let server = MockServer::start();

        let mock_ok = server.mock(|when, then| {
            when.method(GET).path("/api/v1/authenticate");
            then.status(200).json_body(serde_json::json!({
                "user": {
                    "username": "test_user"
                }
            }));
        });

        let mut client = CloudClient::with_base_url(server.url("/api/v1")).unwrap();
        client.token = Some("valid_token".to_string());

        let user = client.whoami().unwrap();
        assert_eq!(user, "test_user");
        mock_ok.assert_calls(1);

        let mock_err = server.mock(|when, then| {
            when.method(GET).path("/api/v1_err/authenticate");
            then.status(401);
        });

        let mut client_err = CloudClient::with_base_url(server.url("/api/v1_err")).unwrap();
        client_err.token = Some("invalid_token".to_string());

        assert!(client_err.whoami().is_err());
        mock_err.assert_calls(1);

        let client_no_token = CloudClient::with_base_url(server.url("/api/v1")).unwrap();
        assert!(client_no_token.whoami().is_err());
    }

    #[test]
    fn test_delete_token() {
        let server = MockServer::start();

        let mock_ok = server.mock(|when, then| {
            when.method(DELETE).path("/api/v1/authenticate");
            then.status(200);
        });

        let mut client = CloudClient::with_base_url(server.url("/api/v1")).unwrap();
        client.token = Some("valid_token".to_string());

        assert!(client.delete_token().is_ok());
        mock_ok.assert_calls(1);

        let mock_404 = server.mock(|when, then| {
            when.method(DELETE).path("/api/v1_404/authenticate");
            then.status(404);
        });

        let mut client_404 = CloudClient::with_base_url(server.url("/api/v1_404")).unwrap();
        client_404.token = Some("token_404".to_string());

        assert!(client_404.delete_token().is_ok());
        mock_404.assert_calls(1);

        let mock_err = server.mock(|when, then| {
            when.method(DELETE).path("/api/v1_err/authenticate");
            then.status(500);
        });

        let mut client_err = CloudClient::with_base_url(server.url("/api/v1_err")).unwrap();
        client_err.token = Some("error_token".to_string());

        assert!(client_err.delete_token().is_err());
        mock_err.assert_calls(1);

        let client_no_token = CloudClient::with_base_url(server.url("/api/v1")).unwrap();
        assert!(client_no_token.delete_token().is_ok());
    }

    #[test]
    fn test_client_init() {
        let client = CloudClient::new();
        assert!(client.is_ok());
    }

    #[test]
    fn test_client_init_with_env() {
        unsafe {
            std::env::set_var("VAGRANT_CLOUD_URL", "http://localhost:1234");
        }
        let client = CloudClient::new();
        unsafe {
            std::env::remove_var("VAGRANT_CLOUD_URL");
        }
        assert!(client.is_ok());
    }

    #[test]
    fn test_fetch_metadata_success() {
        // Start a lightweight mock server.
        let server = MockServer::start();

        // Create a mock on the server.
        let mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/test/box");
            then.status(200).json_body(serde_json::json!({
                "description_markdown": "Test Box",
                "short_description": "Short",
                "name": "test/box",
                "versions": [
                    {
                        "version": "1.0.0",
                        "providers": [
                            {
                                "name": "virtualbox",
                                "url": "http://example.com/box.box"
                            }
                        ]
                    }
                ]
            }));
        });

        let client = CloudClient::with_base_url(server.url("")).expect("operation should succeed");

        let metadata = client
            .fetch_metadata("test/box")
            .expect("operation should succeed");

        assert_eq!(metadata.name, "test/box");
        assert_eq!(metadata.versions.len(), 1);
        assert_eq!(metadata.versions[0].version, "1.0.0");
        assert_eq!(metadata.versions[0].providers.len(), 1);
        assert_eq!(metadata.versions[0].providers[0].name, "virtualbox");

        mock.assert();
    }

    #[test]
    fn test_fetch_metadata_not_found() {
        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/missing/box");
            then.status(404);
        });

        let client = CloudClient::with_base_url(server.url("")).expect("operation should succeed");
        let result = client.fetch_metadata("missing/box");

        assert!(result.is_err());
        mock.assert();
    }

    #[test]
    fn test_fetch_metadata_json_error() {
        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/bad/box");
            then.status(200).body("not json");
        });

        let client = CloudClient::with_base_url(server.url("")).expect("operation should succeed");
        let result = client.fetch_metadata("bad/box");

        assert!(result.is_err());
        mock.assert();
    }

    #[test]
    fn test_fetch_metadata_network_error() {
        let client = CloudClient::with_base_url("http://127.0.0.1:0".to_string())
            .expect("operation should succeed");
        println!("Delete box result: {:?}", client.delete_box("user/box"));
        let result = client.fetch_metadata("test/box");
        assert!(result.is_err());
    }

    #[test]
    fn test_cloud_structs_debug() {
        let metadata = BoxMetadata {
            description_markdown: "desc".to_string(),
            short_description: "short".to_string(),
            name: "name".to_string(),
            versions: vec![BoxVersion {
                version: "1.0".to_string(),
                providers: vec![BoxProvider {
                    name: "virtualbox".to_string(),
                    url: "url".to_string(),
                    checksum: Some("c".to_string()),
                    checksum_type: Some("sha256".to_string()),
                }],
            }],
        };
        let debug_str = format!("{:?}", metadata);
        assert!(debug_str.contains("description_markdown"));
        assert!(debug_str.contains("BoxProvider"));
    }

    #[test]
    fn test_create_box_success_with_descriptions() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/api/v1/boxes");
            then.status(200);
        });

        let client =
            CloudClient::with_base_url(server.url("/api/v1")).expect("operation should succeed");

        let res = client.create_box("user/box", Some("desc1"), Some("desc2"));
        assert!(res.is_ok());
        mock.assert();
    }

    #[test]
    fn test_delete_box() {
        let server = MockServer::start();
        let mock_ok = server.mock(|when, then| {
            when.method(DELETE).path("/api/v1/boxes/user/box");
            then.status(200);
        });
        let mock_err = server.mock(|when, then| {
            when.method(DELETE).path("/api/v1/boxes/user/box2");
            then.status(500);
        });

        let client =
            CloudClient::with_base_url(server.url("/api/v1")).expect("operation should succeed");

        assert!(client.delete_box("user/box").is_ok());
        assert!(client.delete_box("user/box2").is_err());
        mock_ok.assert();
        mock_err.assert();
    }

    #[test]
    fn test_update_box() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(PUT).path("/api/v1/boxes/user/box");
            then.status(200);
        });
        let mock_err = server.mock(|when, then| {
            when.method(PUT).path("/api/v1/boxes/user/box2");
            then.status(404);
        });

        let client =
            CloudClient::with_base_url(server.url("/api/v1")).expect("operation should succeed");

        assert!(
            client
                .update_box("user/box", Some("desc1"), Some("desc2"))
                .is_ok()
        );
        assert!(client.update_box("user/box2", None, None).is_err());
        mock.assert();
        mock_err.assert();
    }

    #[test]
    fn test_create_version() {
        let server = MockServer::start();
        let _mock_ok = server.mock(|when, then| {
            when.method(POST).path("/api/v1/boxes/user/box/versions");
            then.status(200);
        });
        let _mock_err = server.mock(|when, then| {
            when.method(POST).path("/api/v1/boxes/user/box2/versions");
            then.status(500);
        });

        let client =
            CloudClient::with_base_url(server.url("/api/v1")).expect("operation should succeed");

        assert!(
            client
                .create_version("user/box", "1.0", Some("desc"))
                .is_ok()
        );
        assert!(client.create_version("user/box2", "1.0", None).is_err());
    }

    #[test]
    fn test_update_version() {
        let server = MockServer::start();
        let _mock_ok = server.mock(|when, then| {
            when.method(PUT).path("/api/v1/boxes/user/box/versions/1.0");
            then.status(200);
        });
        let _mock_err = server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v1/boxes/user/box2/versions/1.0");
            then.status(500);
        });

        let client =
            CloudClient::with_base_url(server.url("/api/v1")).expect("operation should succeed");

        assert!(
            client
                .update_version("user/box", "1.0", Some("desc"))
                .is_ok()
        );
        assert!(client.update_version("user/box2", "1.0", None).is_err());
    }

    #[test]
    fn test_delete_version() {
        let server = MockServer::start();
        let _mock_ok = server.mock(|when, then| {
            when.method(DELETE)
                .path("/api/v1/boxes/user/box/versions/1.0");
            then.status(200);
        });
        let _mock_err = server.mock(|when, then| {
            when.method(DELETE)
                .path("/api/v1/boxes/user/box2/versions/1.0");
            then.status(500);
        });

        let client =
            CloudClient::with_base_url(server.url("/api/v1")).expect("operation should succeed");

        assert!(client.delete_version("user/box", "1.0").is_ok());
        assert!(client.delete_version("user/box2", "1.0").is_err());
    }

    #[test]
    fn test_release_version() {
        let server = MockServer::start();
        let _mock_ok = server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v1/boxes/user/box/versions/1.0/release");
            then.status(200);
        });
        let _mock_err = server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v1/boxes/user/box2/versions/1.0/release");
            then.status(500);
        });

        let client =
            CloudClient::with_base_url(server.url("/api/v1")).expect("operation should succeed");

        assert!(client.release_version("user/box", "1.0").is_ok());
        assert!(client.release_version("user/box2", "1.0").is_err());
    }

    #[test]
    fn test_revoke_version() {
        let server = MockServer::start();
        let _mock_ok = server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v1/boxes/user/box/versions/1.0/revoke");
            then.status(200);
        });
        let _mock_err = server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v1/boxes/user/box2/versions/1.0/revoke");
            then.status(500);
        });

        let client =
            CloudClient::with_base_url(server.url("/api/v1")).expect("operation should succeed");

        assert!(client.revoke_version("user/box", "1.0").is_ok());
        assert!(client.revoke_version("user/box2", "1.0").is_err());
    }

    #[test]
    fn test_create_provider() {
        let server = MockServer::start();
        let _mock_ok = server.mock(|when, then| {
            when.method(POST)
                .path("/api/v1/boxes/user/box/versions/1.0/providers");
            then.status(200);
        });
        let _mock_err = server.mock(|when, then| {
            when.method(POST)
                .path("/api/v1/boxes/user/box2/versions/1.0/providers");
            then.status(500);
        });

        let client =
            CloudClient::with_base_url(server.url("/api/v1")).expect("operation should succeed");

        assert!(
            client
                .create_provider("user/box", "1.0", "virtualbox")
                .is_ok()
        );
        assert!(
            client
                .create_provider("user/box2", "1.0", "virtualbox")
                .is_err()
        );
    }

    #[test]
    fn test_update_provider() {
        let server = MockServer::start();
        let _mock_ok = server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v1/boxes/user/box/versions/1.0/providers/virtualbox");
            then.status(200);
        });
        let _mock_err = server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v1/boxes/user/box2/versions/1.0/providers/virtualbox");
            then.status(500);
        });

        let client =
            CloudClient::with_base_url(server.url("/api/v1")).expect("operation should succeed");

        assert!(
            client
                .update_provider("user/box", "1.0", "virtualbox", Some("url"), None)
                .is_ok()
        );
        assert!(
            client
                .update_provider("user/box2", "1.0", "virtualbox", None, None)
                .is_err()
        );
    }

    #[test]
    fn test_delete_provider() {
        let server = MockServer::start();
        let _mock_ok = server.mock(|when, then| {
            when.method(DELETE)
                .path("/api/v1/boxes/user/box/versions/1.0/providers/virtualbox");
            then.status(200);
        });
        let _mock_err = server.mock(|when, then| {
            when.method(DELETE)
                .path("/api/v1/boxes/user/box2/versions/1.0/providers/virtualbox");
            then.status(500);
        });

        let client =
            CloudClient::with_base_url(server.url("/api/v1")).expect("operation should succeed");

        assert!(
            client
                .delete_provider("user/box", "1.0", "virtualbox")
                .is_ok()
        );
        assert!(
            client
                .delete_provider("user/box2", "1.0", "virtualbox")
                .is_err()
        );
    }

    #[test]
    fn test_get_upload_url() {
        let server = MockServer::start();
        let _mock_ok = server.mock(|when, then| {
            when.method(GET)
                .path("/api/v1/boxes/user/box/versions/1.0/providers/virtualbox/upload");
            then.status(200).body(r#"{"upload_path":"http://upload"}"#);
        });
        let _mock_err = server.mock(|when, then| {
            when.method(GET)
                .path("/api/v1/boxes/user/box2/versions/1.0/providers/virtualbox/upload");
            then.status(500);
        });
        let _mock_bad_json = server.mock(|when, then| {
            when.method(GET)
                .path("/api/v1/boxes/user/box3/versions/1.0/providers/virtualbox/upload");
            then.status(200)
                .body(r#"{"not_upload_path":"http://upload"}"#);
        });

        let client =
            CloudClient::with_base_url(server.url("/api/v1")).expect("operation should succeed");

        assert!(
            client
                .get_upload_url("user/box", "1.0", "virtualbox")
                .is_ok()
        );
        assert!(
            client
                .get_upload_url("user/box2", "1.0", "virtualbox")
                .is_err()
        );
        assert!(
            client
                .get_upload_url("user/box3", "1.0", "virtualbox")
                .is_err()
        );
    }

    #[test]
    fn test_upload_file() {
        use std::io::Write;
        let server = MockServer::start();
        let _mock_ok = server.mock(|when, then| {
            when.method(PUT).path("/upload");
            then.status(200);
        });
        let _mock_err = server.mock(|when, then| {
            when.method(PUT).path("/upload_err");
            then.status(500);
        });

        let client =
            CloudClient::with_base_url(server.url("/api/v1")).expect("operation should succeed");

        let mut temp_file = tempfile::NamedTempFile::new().expect("operation should succeed");
        temp_file
            .write_all(b"test")
            .expect("operation should succeed");

        assert!(
            client
                .upload_file(&server.url("/upload"), temp_file.path())
                .is_ok()
        );
        assert!(
            client
                .upload_file(&server.url("/upload_err"), temp_file.path())
                .is_err()
        );
    }

    #[test]
    fn test_update_provider_with_checksum_type() {
        let server = MockServer::start();
        let _mock_ok = server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v1/boxes/user/box/versions/1.0/providers/virtualbox");
            then.status(200);
        });

        let client =
            CloudClient::with_base_url(server.url("/api/v1")).expect("operation should succeed");
        assert!(
            client
                .update_provider("user/box", "1.0", "virtualbox", None, Some("md5"))
                .is_ok()
        );
    }

    #[test]
    fn test_search_boxes_error() {
        let server = MockServer::start();
        let _mock_err = server.mock(|when, then| {
            when.method(GET).path("/api/v1/search");
            then.status(500);
        });

        let client =
            CloudClient::with_base_url(server.url("/api/v1")).expect("operation should succeed");
        assert!(client.search_boxes("query").is_err());
    }

    #[test]
    fn test_all_network_errors() {
        let client = CloudClient::with_base_url("http://127.0.0.1:0".to_string())
            .expect("operation should succeed");

        assert!(
            client
                .create_box("user/box", Some("desc"), Some("short"))
                .is_err()
        );
        assert!(
            client
                .update_box("user/box", Some("desc"), Some("short"))
                .is_err()
        );
        assert!(client.delete_box("user/box").is_err());
        assert!(
            client
                .create_version("user/box", "1.0", Some("desc"))
                .is_err()
        );
        assert!(
            client
                .update_version("user/box", "1.0", Some("desc"))
                .is_err()
        );
        assert!(client.delete_version("user/box", "1.0").is_err());
        assert!(client.release_version("user/box", "1.0").is_err());
        assert!(client.revoke_version("user/box", "1.0").is_err());
        assert!(
            client
                .create_provider("user/box", "1.0", "virtualbox")
                .is_err()
        );
        assert!(
            client
                .update_provider("user/box", "1.0", "virtualbox", None, None)
                .is_err()
        );
        assert!(
            client
                .delete_provider("user/box", "1.0", "virtualbox")
                .is_err()
        );

        assert!(
            client
                .get_upload_url("user/box", "1.0", "virtualbox")
                .is_err()
        );
        assert!(client.search_boxes("query").is_err());
        assert!(client.authenticate("user", "pass", None).is_err());

        let temp_file = tempfile::NamedTempFile::new().expect("operation should succeed");
        assert!(
            client
                .upload_file("http://127.0.0.1:0/upload", temp_file.path())
                .is_err()
        );

        assert!(
            client
                .upload_file(
                    "http://127.0.0.1:0/upload",
                    std::path::Path::new("/non/existent/file.txt")
                )
                .is_err()
        );
    }

    #[test]
    fn test_authenticate_success() {
        let server = MockServer::start();
        let mock_ok = server.mock(|when, then| {
            when.method(POST)
                .path("/api/v1/authenticate")
                .header("Authorization", "Basic dXNlcm5hbWU6cGFzc3dvcmQ="); // base64 for username:password
            then.status(200).json_body(serde_json::json!({
                "token": "test_token"
            }));
        });

        let client =
            CloudClient::with_base_url(server.url("/api/v1")).expect("operation should succeed");
        let token = client
            .authenticate("username", "password", Some("my token"))
            .expect("operation should succeed");
        assert_eq!(token, "test_token");
        mock_ok.assert();
    }

    #[test]
    fn test_authenticate_failure() {
        let server = MockServer::start();
        let mock_err = server.mock(|when, then| {
            when.method(POST).path("/api/v1/authenticate");
            then.status(401);
        });

        let client =
            CloudClient::with_base_url(server.url("/api/v1")).expect("operation should succeed");
        assert!(client.authenticate("user", "pass", None).is_err());
        mock_err.assert();
    }

    #[test]
    fn test_authenticate_json_error() {
        let server = MockServer::start();
        let mock_bad_json = server.mock(|when, then| {
            when.method(POST).path("/api/v1/authenticate");
            then.status(200).body("not json");
        });

        let client =
            CloudClient::with_base_url(server.url("/api/v1")).expect("operation should succeed");
        assert!(client.authenticate("user", "pass", None).is_err());
        mock_bad_json.assert();
    }

    #[test]
    fn test_search_boxes_success_and_json_error() {
        let server = MockServer::start();
        let _mock_ok = server.mock(|when, then| {
            when.method(GET).path("/api/v1/search").query_param("q", "query");
            then.status(200).body(r#"{"boxes": [{"name": "user/box", "description_markdown": "", "short_description": "", "versions": []}]}"#);
        });

        let _mock_err = server.mock(|when, then| {
            when.method(GET)
                .path("/api/v1/search")
                .query_param("q", "bad");
            then.status(200).body(r#"invalid json"#);
        });

        let client =
            CloudClient::with_base_url(server.url("/api/v1")).expect("operation should succeed");

        let boxes = client
            .search_boxes("query")
            .expect("operation should succeed");
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].name, "user/box");

        assert!(client.search_boxes("bad").is_err());
    }

    #[coverage(off)]
    fn restore_env(token: Option<String>, home: Option<String>) {
        unsafe {
            if let Some(t) = token {
                std::env::set_var("VAGRANT_CLOUD_TOKEN", t);
            } else {
                std::env::remove_var("VAGRANT_CLOUD_TOKEN");
            }
            if let Some(h) = home {
                std::env::set_var("VAGRANT_HOME", h);
            } else {
                std::env::remove_var("VAGRANT_HOME");
            }
        }
    }

    /// Tests CloudClient::new when environment variables are unset.
    #[test]
    fn test_cloud_client_new_env_fallbacks() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let orig_token = std::env::var("VAGRANT_CLOUD_TOKEN").ok();
        let orig_home = std::env::var("VAGRANT_HOME").ok();

        unsafe {
            std::env::remove_var("VAGRANT_CLOUD_TOKEN");
            std::env::remove_var("VAGRANT_HOME");
        }

        let client = CloudClient::new();
        assert!(client.is_ok());

        restore_env(orig_token, orig_home);
    }

    /// Tests reading token from file when VAGRANT_CLOUD_TOKEN is unset.
    #[test]
    fn test_cloud_client_token_from_file() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempfile::tempdir().expect("tempdir failed");
        let token_dir = dir.path().join("data");
        std::fs::create_dir_all(&token_dir).expect("create_dir failed");
        std::fs::write(token_dir.join("token"), "file-token-123\n").expect("write failed");

        let orig_token = std::env::var("VAGRANT_CLOUD_TOKEN").ok();
        let orig_home = std::env::var("VAGRANT_HOME").ok();

        unsafe {
            std::env::remove_var("VAGRANT_CLOUD_TOKEN");
            std::env::set_var("VAGRANT_HOME", dir.path());
        }

        let client = CloudClient::new().expect("operation should succeed");
        assert_eq!(client.token, Some("file-token-123".to_string()));

        restore_env(orig_token, orig_home);
    }

    /// Tests whoami and delete_token with empty token.
    #[test]
    fn test_empty_token_branches() {
        let mut client = CloudClient::new().expect("operation should succeed");
        client.token = Some("".to_string());

        assert!(client.whoami().is_err());
        assert!(client.delete_token().is_ok());
    }

    /// Tests create_box with None descriptions and failure status.
    #[test]
    fn test_create_box_none_descriptions_and_failure() {
        let server = MockServer::start();
        let _create_mock = server.mock(|when, then| {
            when.method(POST).path("/api/v1/boxes");
            then.status(500);
        });

        let client =
            CloudClient::with_base_url(server.url("/api/v1")).expect("operation should succeed");
        let res = client.create_box("user/box", None, None);
        assert!(res.is_err());
    }

    /// Tests network errors in whoami and delete_token.
    #[test]
    fn test_whoami_and_delete_token_network_errors() {
        let mut client =
            CloudClient::with_base_url("http://invalid.local.domain.test:12345".to_string())
                .expect("operation should succeed");
        client.token = Some("token123".to_string());

        assert!(client.whoami().is_err());
        assert!(client.delete_token().is_err());
    }
}
