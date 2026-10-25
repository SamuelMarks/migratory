//! Vagrant Cloud API client.
//!
//! This module provides a client for interacting with the Vagrant Cloud API
//! to fetch box metadata, authenticate users, and manage boxes, versions, and providers.

use crate::error::MigratoryError;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

/// Creates the default blocking HTTP client for Vagrant Cloud API requests.
///
/// # Returns
///
/// Returns a configured [`Client`] instance.
fn create_default_client() -> Client {
    Client::new()
}

/// Vagrant Cloud API client.
///
/// Holds the underlying HTTP client, API base URL, and optional bearer token.
#[derive(Clone)]
pub struct CloudClient {
    /// Blocking HTTP client used to issue API requests.
    client: Client,
    /// Base URL for the Vagrant Cloud API (e.g., `https://app.vagrantup.com/api/v2`).
    base_url: String,
    /// Optional authentication bearer token for authorized API endpoints.
    token: Option<String>,
}

/// A box catalog metadata response.
///
/// Represents the high-level metadata of a box in the catalog.
#[derive(Deserialize, Debug)]
pub struct BoxMetadata {
    /// Full Markdown description of the box.
    pub description_markdown: String,
    /// Short summary description of the box.
    pub short_description: String,
    /// Fully qualified box name (e.g., `hashicorp/bionic64`).
    pub name: String,
    /// List of available versions for this box.
    pub versions: Vec<BoxVersion>,
}

/// A version of a box.
///
/// Represents a specific version string and its associated providers.
#[derive(Deserialize, Debug)]
pub struct BoxVersion {
    /// Semantic version string (e.g., `1.0.0`).
    pub version: String,
    /// List of providers available for this version.
    pub providers: Vec<BoxProvider>,
}

/// A provider for a box version.
///
/// Represents an available provider (like `virtualbox` or `qemu`) and its download URL.
#[derive(Deserialize, Debug)]
pub struct BoxProvider {
    /// Provider identifier (e.g., `virtualbox`).
    pub name: String,
    /// Download URL for the `.box` artifact.
    pub url: String,
    /// Optional cryptographic checksum of the `.box` artifact.
    pub checksum: Option<String>,
    /// Optional checksum algorithm name (e.g., `sha256`).
    pub checksum_type: Option<String>,
}

/// Configuration for constructing a [`CloudClient`].
#[derive(Debug, Clone)]
pub struct CloudClientConfig {
    /// Base URL for the Vagrant Cloud API.
    pub base_url: String,
    /// Optional authentication bearer token.
    pub token: Option<String>,
}

impl CloudClientConfig {
    /// Resolves configuration from environment variables (`VAGRANT_CLOUD_URL`, `VAGRANT_CLOUD_TOKEN`, `VAGRANT_HOME`).
    ///
    /// # Returns
    ///
    /// Returns a [`CloudClientConfig`] populated from the environment.
    pub fn from_env() -> Self {
        Self::from_parts(
            std::env::var("VAGRANT_CLOUD_URL").ok().as_deref(),
            std::env::var("VAGRANT_CLOUD_TOKEN").ok().as_deref(),
            std::env::var("VAGRANT_HOME").ok().as_deref(),
        )
    }

    /// Resolves configuration from explicit optional environment variable strings.
    ///
    /// # Arguments
    ///
    /// * `cloud_url` - Optional `VAGRANT_CLOUD_URL` value.
    /// * `cloud_token` - Optional `VAGRANT_CLOUD_TOKEN` value.
    /// * `vagrant_home` - Optional `VAGRANT_HOME` directory path.
    ///
    /// # Returns
    ///
    /// Returns a [`CloudClientConfig`] populated from the provided values or defaults.
    pub fn from_parts(
        cloud_url: Option<&str>,
        cloud_token: Option<&str>,
        vagrant_home: Option<&str>,
    ) -> Self {
        let base_url = cloud_url
            .unwrap_or("https://app.vagrantup.com/api/v2")
            .to_string();

        let token = cloud_token.map(String::from).or_else(|| {
            let home_dir = vagrant_home.unwrap_or(".vagrant.d");
            let token_path = std::path::Path::new(home_dir).join("data").join("token");
            std::fs::read_to_string(&token_path)
                .ok()
                .map(|s| s.trim().to_string())
        });

        Self { base_url, token }
    }
}

impl CloudClient {
    /// Creates a new `CloudClient` using environment-resolved configuration.
    ///
    /// Initializes an HTTP client configured for Vagrant Cloud API interactions,
    /// reading `VAGRANT_CLOUD_URL` and `VAGRANT_CLOUD_TOKEN` (or `~/.vagrant.d/data/token`)
    /// when available.
    ///
    /// # Returns
    ///
    /// Returns a new instance of [`CloudClient`] on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if the underlying HTTP client cannot be initialized.
    pub fn new() -> Result<Self, MigratoryError> {
        Self::with_config(CloudClientConfig::from_env())
    }

    /// Creates a new `CloudClient` from an explicit [`CloudClientConfig`].
    ///
    /// # Arguments
    ///
    /// * `config` - Explicit client configuration.
    ///
    /// # Returns
    ///
    /// Returns a configured [`CloudClient`] on success.
    ///
    /// # Errors
    ///
    /// Returns [`MigratoryError::Validation`] if `config.base_url` is empty.
    pub fn with_config(config: CloudClientConfig) -> Result<Self, MigratoryError> {
        if config.base_url.trim().is_empty() {
            return Err(MigratoryError::Validation(
                "Cloud API base_url cannot be empty".to_string(),
            ));
        }
        let client = create_default_client();
        Ok(Self {
            client,
            base_url: config.base_url,
            token: config.token,
        })
    }

    /// Fetches metadata for a given box tag (e.g., `"ubuntu/jammy64"`).
    ///
    /// # Arguments
    ///
    /// * `tag` - The box identifier (e.g., `hashicorp/bionic64`).
    ///
    /// # Returns
    ///
    /// Returns the parsed [`BoxMetadata`] on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if the HTTP request fails, returns a non-success status,
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

    /// Authenticates with Vagrant Cloud using HTTP Basic authentication and returns an access token.
    ///
    /// # Arguments
    ///
    /// * `username` - The Vagrant Cloud username or email.
    /// * `password` - The account password.
    /// * `description` - Optional description for the generated token.
    ///
    /// # Returns
    ///
    /// Returns the newly issued authentication token string on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if the request fails, credentials are rejected, or the response is invalid.
    pub fn authenticate(
        &self,
        username: &str,
        password: &str,
        description: Option<&str>,
    ) -> Result<String, MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/authenticate", base);

        /// Token metadata payload nested inside an authentication request.
        #[derive(Serialize)]
        struct TokenRequest<'a> {
            /// Human-readable description of the token.
            description: &'a str,
        }
        /// Top-level JSON body for token creation requests.
        #[derive(Serialize)]
        struct AuthRequest<'a> {
            /// Nested token request parameters.
            token: TokenRequest<'a>,
        }

        let desc = description.unwrap_or("Migratory login");
        let payload = AuthRequest {
            token: TokenRequest { description: desc },
        };

        /// JSON response returned from the `/authenticate` endpoint.
        #[derive(Deserialize)]
        struct AuthResponse {
            /// Newly created bearer token string.
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
    ///
    /// # Returns
    ///
    /// Returns the authenticated username string on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if no token is configured, the request fails, or the token is invalid.
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

        /// Nested user profile object in authentication verification responses.
        #[derive(Deserialize)]
        struct UserObj {
            /// Account username.
            username: String,
        }
        /// Response payload from `GET /authenticate`.
        #[derive(Deserialize)]
        struct AuthResponse {
            /// Authenticated user object.
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

    /// Revokes the current authentication token on Vagrant Cloud.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success or when no token is present.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if the HTTP revocation request fails.
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
    ///
    /// # Arguments
    ///
    /// * `name` - Box tag (e.g., `"user/box"`).
    /// * `description` - Optional full description.
    /// * `short_description` - Optional short summary description.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if the API request fails or returns a non-success status.
    pub fn create_box(
        &self,
        name: &str,
        description: Option<&str>,
        short_description: Option<&str>,
    ) -> Result<(), MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/boxes", base);

        /// Inner box creation parameters.
        #[derive(Serialize)]
        struct BoxCreate<'a> {
            /// Name of the box to create.
            name: &'a str,
            /// Optional full description.
            #[serde(skip_serializing_if = "Option::is_none")]
            description: Option<&'a str>,
            /// Optional short description.
            #[serde(skip_serializing_if = "Option::is_none")]
            short_description: Option<&'a str>,
        }
        /// Top-level JSON body for box creation.
        #[derive(Serialize)]
        struct CreateBoxRequest<'a> {
            /// Box creation details.
            #[serde(rename = "box")]
            box_field: BoxCreate<'a>,
        }

        let payload = CreateBoxRequest {
            box_field: BoxCreate {
                name,
                description,
                short_description,
            },
        };

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
    ///
    /// # Arguments
    ///
    /// * `name` - Box tag to delete (e.g., `"user/box"`).
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if the API request fails or returns a non-success status.
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
    ///
    /// # Arguments
    ///
    /// * `name` - Box tag to update (e.g., `"user/box"`).
    /// * `description` - Optional updated full description.
    /// * `short_description` - Optional updated short summary description.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if the API request fails or returns a non-success status.
    pub fn update_box(
        &self,
        name: &str,
        description: Option<&str>,
        short_description: Option<&str>,
    ) -> Result<(), MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/boxes/{}", base, name);

        /// Inner box update parameters.
        #[derive(Serialize)]
        struct BoxUpdate<'a> {
            /// Updated full description.
            #[serde(skip_serializing_if = "Option::is_none")]
            description: Option<&'a str>,
            /// Updated short description.
            #[serde(skip_serializing_if = "Option::is_none")]
            short_description: Option<&'a str>,
        }
        /// Top-level JSON body for box update requests.
        #[derive(Serialize)]
        struct UpdateBoxRequest<'a> {
            /// Box update details.
            #[serde(rename = "box")]
            box_field: BoxUpdate<'a>,
        }

        let payload = UpdateBoxRequest {
            box_field: BoxUpdate {
                description,
                short_description,
            },
        };

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
    ///
    /// # Arguments
    ///
    /// * `box_name` - Box tag (e.g., `"user/box"`).
    /// * `version` - Version string to create (e.g., `"1.0.0"`).
    /// * `description` - Optional version release notes / description.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if the API request fails or returns a non-success status.
    pub fn create_version(
        &self,
        box_name: &str,
        version: &str,
        description: Option<&str>,
    ) -> Result<(), MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/boxes/{}/versions", base, box_name);

        /// Inner version creation parameters.
        #[derive(Serialize)]
        struct VersionCreate<'a> {
            /// Version identifier string.
            version: &'a str,
            /// Optional version description.
            #[serde(skip_serializing_if = "Option::is_none")]
            description: Option<&'a str>,
        }
        /// Top-level JSON body for version creation requests.
        #[derive(Serialize)]
        struct CreateVersionRequest<'a> {
            /// Version creation details.
            version: VersionCreate<'a>,
        }

        let payload = CreateVersionRequest {
            version: VersionCreate {
                version,
                description,
            },
        };

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
    ///
    /// # Arguments
    ///
    /// * `box_name` - Box tag (e.g., `"user/box"`).
    /// * `version` - Version string to delete (e.g., `"1.0.0"`).
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if the API request fails or returns a non-success status.
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
    ///
    /// # Arguments
    ///
    /// * `box_name` - Box tag (e.g., `"user/box"`).
    /// * `version` - Version string to revoke (e.g., `"1.0.0"`).
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if the API request fails or returns a non-success status.
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
    ///
    /// # Arguments
    ///
    /// * `box_name` - Box tag (e.g., `"user/box"`).
    /// * `version` - Version string to update (e.g., `"1.0.0"`).
    /// * `description` - Optional updated version description.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if the API request fails or returns a non-success status.
    pub fn update_version(
        &self,
        box_name: &str,
        version: &str,
        description: Option<&str>,
    ) -> Result<(), MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/boxes/{}/versions/{}", base, box_name, version);

        /// Inner version update parameters.
        #[derive(Serialize)]
        struct VersionUpdate<'a> {
            /// Updated version description.
            #[serde(skip_serializing_if = "Option::is_none")]
            description: Option<&'a str>,
        }
        /// Top-level JSON body for version update requests.
        #[derive(Serialize)]
        struct UpdateVersionRequest<'a> {
            /// Version update details.
            version: VersionUpdate<'a>,
        }

        let payload = UpdateVersionRequest {
            version: VersionUpdate { description },
        };

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
    ///
    /// # Arguments
    ///
    /// * `box_name` - Name of the box (e.g., `"hashicorp/bionic64"`).
    /// * `version` - Version of the box.
    /// * `provider` - Provider name (e.g., `"virtualbox"`).
    /// * `architecture` - Optional target architecture (e.g., `"amd64"`, `"arm64"`).
    /// * `default_architecture` - Optional flag marking this provider as default architecture.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if the provider cannot be created.
    pub fn create_provider(
        &self,
        box_name: &str,
        version: &str,
        provider: &str,
        architecture: Option<&str>,
        default_architecture: Option<bool>,
    ) -> Result<(), MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/boxes/{}/versions/{}/providers", base, box_name, version);

        /// Inner provider creation parameters.
        #[derive(Serialize)]
        struct ProviderCreate<'a> {
            /// Provider identifier.
            name: &'a str,
            /// Optional target CPU architecture.
            #[serde(skip_serializing_if = "Option::is_none")]
            architecture: Option<&'a str>,
            /// Whether this architecture is the default for the provider.
            #[serde(skip_serializing_if = "Option::is_none")]
            default_architecture: Option<bool>,
        }
        /// Top-level JSON body for provider creation requests.
        #[derive(Serialize)]
        struct CreateProviderRequest<'a> {
            /// Provider creation details.
            provider: ProviderCreate<'a>,
        }

        let payload = CreateProviderRequest {
            provider: ProviderCreate {
                name: provider,
                architecture,
                default_architecture,
            },
        };

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
    ///
    /// # Arguments
    ///
    /// * `box_name` - Box tag (e.g., `"user/box"`).
    /// * `version` - Version string (e.g., `"1.0.0"`).
    /// * `provider` - Provider name (e.g., `"virtualbox"`).
    /// * `checksum` - Optional cryptographic checksum value.
    /// * `checksum_type` - Optional checksum algorithm (e.g., `"sha256"`).
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if the API request fails or returns a non-success status.
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

        /// Inner provider update parameters.
        #[derive(Serialize)]
        struct ProviderUpdate<'a> {
            /// Updated checksum string.
            #[serde(skip_serializing_if = "Option::is_none")]
            checksum: Option<&'a str>,
            /// Updated checksum algorithm type.
            #[serde(skip_serializing_if = "Option::is_none")]
            checksum_type: Option<&'a str>,
        }
        /// Top-level JSON body for provider update requests.
        #[derive(Serialize)]
        struct UpdateProviderRequest<'a> {
            /// Provider update details.
            provider: ProviderUpdate<'a>,
        }

        let payload = UpdateProviderRequest {
            provider: ProviderUpdate {
                checksum,
                checksum_type,
            },
        };

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
    ///
    /// # Arguments
    ///
    /// * `box_name` - Box tag (e.g., `"user/box"`).
    /// * `version` - Version string (e.g., `"1.0.0"`).
    /// * `provider` - Provider name to delete (e.g., `"virtualbox"`).
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if the API request fails or returns a non-success status.
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

    /// Gets the pre-signed upload URL for a provider artifact on Vagrant Cloud.
    ///
    /// # Arguments
    ///
    /// * `box_name` - Box tag (e.g., `"user/box"`).
    /// * `version` - Version string (e.g., `"1.0.0"`).
    /// * `provider` - Provider name (e.g., `"virtualbox"`).
    ///
    /// # Returns
    ///
    /// Returns the upload target URL string on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if the API request fails or returns invalid JSON.
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

        /// JSON response containing the pre-signed upload URL.
        #[derive(Deserialize)]
        struct UploadResponse {
            /// Target URL to `PUT` the box file to.
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

    /// Uploads a `.box` file to Vagrant Cloud using the provided upload URL.
    ///
    /// # Arguments
    ///
    /// * `upload_path` - The pre-signed upload URL obtained from [`CloudClient::get_upload_url`].
    /// * `file_path` - Local filesystem path to the `.box` archive to upload.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if the local file cannot be opened or the HTTP `PUT` fails.
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
    ///
    /// # Arguments
    ///
    /// * `box_name` - Box tag (e.g., `"user/box"`).
    /// * `version` - Version string to mark as released (e.g., `"1.0.0"`).
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if the API request fails or returns a non-success status.
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

    /// Searches for boxes on Vagrant Cloud matching a query string.
    ///
    /// # Arguments
    ///
    /// * `query` - Search query term.
    ///
    /// # Returns
    ///
    /// Returns a vector of matching [`BoxMetadata`] entries on success.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if the HTTP request fails or the JSON response is invalid.
    pub fn search_boxes(&self, query: &str) -> Result<Vec<BoxMetadata>, MigratoryError> {
        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/search?q={}", base, query);

        /// JSON response wrapper for box search results.
        #[derive(Deserialize)]
        struct SearchResponse {
            /// List of matching boxes.
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

    /// Helper for testing to inject a custom base URL.
    ///
    /// # Arguments
    ///
    /// * `base_url` - Custom mock server base URL.
    ///
    /// # Returns
    ///
    /// Returns a [`CloudClient`] configured with `base_url`.
    ///
    /// # Errors
    ///
    /// Returns a [`MigratoryError`] if initialization fails.
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
    fn test_whoami_json_error() -> Result<(), MigratoryError> {
        let server = MockServer::start();

        let mock_err = server.mock(|when, then| {
            when.method(GET).path("/api/v1/authenticate");
            then.status(200).body("not json");
        });

        let mut client = CloudClient::with_base_url(server.url("/api/v1"))?;
        client.token = Some("valid_token".to_string());

        assert!(client.whoami().is_err());
        mock_err.assert_calls(1);
        Ok(())
    }

    #[test]
    fn test_whoami() -> Result<(), MigratoryError> {
        let server = MockServer::start();

        let mock_ok = server.mock(|when, then| {
            when.method(GET).path("/api/v1/authenticate");
            then.status(200).json_body(serde_json::json!({
                "user": {
                    "username": "test_user"
                }
            }));
        });

        let mut client = CloudClient::with_base_url(server.url("/api/v1"))?;
        client.token = Some("valid_token".to_string());

        let user = client.whoami()?;
        assert_eq!(user, "test_user");
        mock_ok.assert_calls(1);

        let mock_err = server.mock(|when, then| {
            when.method(GET).path("/api/v1_err/authenticate");
            then.status(401);
        });

        let mut client_err = CloudClient::with_base_url(server.url("/api/v1_err"))?;
        client_err.token = Some("invalid_token".to_string());

        assert!(client_err.whoami().is_err());
        mock_err.assert_calls(1);

        let client_no_token = CloudClient::with_base_url(server.url("/api/v1"))?;
        assert!(client_no_token.whoami().is_err());
        Ok(())
    }

    #[test]
    fn test_delete_token() -> Result<(), MigratoryError> {
        let server = MockServer::start();

        let mock_ok = server.mock(|when, then| {
            when.method(DELETE).path("/api/v1/authenticate");
            then.status(200);
        });

        let mut client = CloudClient::with_base_url(server.url("/api/v1"))?;
        client.token = Some("valid_token".to_string());

        assert!(client.delete_token().is_ok());
        mock_ok.assert_calls(1);

        let mock_404 = server.mock(|when, then| {
            when.method(DELETE).path("/api/v1_404/authenticate");
            then.status(404);
        });

        let mut client_404 = CloudClient::with_base_url(server.url("/api/v1_404"))?;
        client_404.token = Some("token_404".to_string());

        assert!(client_404.delete_token().is_ok());
        mock_404.assert_calls(1);

        let mock_err = server.mock(|when, then| {
            when.method(DELETE).path("/api/v1_err/authenticate");
            then.status(500);
        });

        let mut client_err = CloudClient::with_base_url(server.url("/api/v1_err"))?;
        client_err.token = Some("error_token".to_string());

        assert!(client_err.delete_token().is_err());
        mock_err.assert_calls(1);

        let client_no_token = CloudClient::with_base_url(server.url("/api/v1"))?;
        assert!(client_no_token.delete_token().is_ok());
        Ok(())
    }

    #[test]
    fn test_client_init() {
        let client = CloudClient::with_config(CloudClientConfig {
            base_url: "https://app.vagrantup.com/api/v2".to_string(),
            token: None,
        });
        assert!(client.is_ok());
        assert!(
            CloudClient::with_config(CloudClientConfig {
                base_url: String::new(),
                token: None,
            })
            .is_err()
        );
    }

    #[test]
    fn test_client_init_with_env() {
        let cfg = CloudClientConfig::from_parts(Some("http://localhost:1234"), None, None);
        let client = CloudClient::with_config(cfg);
        assert!(client.is_ok());
    }

    #[test]
    fn test_fetch_metadata_success() -> Result<(), MigratoryError> {
        let server = MockServer::start();

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

        let client = CloudClient::with_base_url(server.url(""))?;

        let metadata = client.fetch_metadata("test/box")?;

        assert_eq!(metadata.name, "test/box");
        assert_eq!(metadata.versions.len(), 1);
        assert_eq!(metadata.versions[0].version, "1.0.0");
        assert_eq!(metadata.versions[0].providers.len(), 1);
        assert_eq!(metadata.versions[0].providers[0].name, "virtualbox");

        mock.assert();
        Ok(())
    }

    #[test]
    fn test_fetch_metadata_not_found() -> Result<(), MigratoryError> {
        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/missing/box");
            then.status(404);
        });

        let client = CloudClient::with_base_url(server.url(""))?;
        let result = client.fetch_metadata("missing/box");

        assert!(result.is_err());
        mock.assert();
        Ok(())
    }

    #[test]
    fn test_fetch_metadata_json_error() -> Result<(), MigratoryError> {
        let server = MockServer::start();

        let mock = server.mock(|when, then| {
            when.method(GET).path("/boxes/bad/box");
            then.status(200).body("not json");
        });

        let client = CloudClient::with_base_url(server.url(""))?;
        let result = client.fetch_metadata("bad/box");

        assert!(result.is_err());
        mock.assert();
        Ok(())
    }

    #[test]
    fn test_fetch_metadata_network_error() -> Result<(), MigratoryError> {
        let client = CloudClient::with_base_url("http://127.0.0.1:0".to_string())?;
        let _ = client.delete_box("user/box");
        let result = client.fetch_metadata("test/box");
        assert!(result.is_err());
        Ok(())
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
    fn test_create_box_success_with_descriptions() -> Result<(), MigratoryError> {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(POST).path("/api/v1/boxes");
            then.status(200);
        });

        let client = CloudClient::with_base_url(server.url("/api/v1"))?;

        let res = client.create_box("user/box", Some("desc1"), Some("desc2"));
        assert!(res.is_ok());
        mock.assert();
        Ok(())
    }

    #[test]
    fn test_delete_box() -> Result<(), MigratoryError> {
        let server = MockServer::start();
        let mock_ok = server.mock(|when, then| {
            when.method(DELETE).path("/api/v1/boxes/user/box");
            then.status(200);
        });
        let mock_err = server.mock(|when, then| {
            when.method(DELETE).path("/api/v1/boxes/user/box2");
            then.status(500);
        });

        let client = CloudClient::with_base_url(server.url("/api/v1"))?;

        assert!(client.delete_box("user/box").is_ok());
        assert!(client.delete_box("user/box2").is_err());
        mock_ok.assert();
        mock_err.assert();
        Ok(())
    }

    #[test]
    fn test_update_box() -> Result<(), MigratoryError> {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(PUT).path("/api/v1/boxes/user/box");
            then.status(200);
        });
        let mock_err = server.mock(|when, then| {
            when.method(PUT).path("/api/v1/boxes/user/box2");
            then.status(404);
        });

        let client = CloudClient::with_base_url(server.url("/api/v1"))?;

        assert!(
            client
                .update_box("user/box", Some("desc1"), Some("desc2"))
                .is_ok()
        );
        assert!(client.update_box("user/box2", None, None).is_err());
        mock.assert();
        mock_err.assert();
        Ok(())
    }

    #[test]
    fn test_create_version() -> Result<(), MigratoryError> {
        let server = MockServer::start();
        let _mock_ok = server.mock(|when, then| {
            when.method(POST).path("/api/v1/boxes/user/box/versions");
            then.status(200);
        });
        let _mock_err = server.mock(|when, then| {
            when.method(POST).path("/api/v1/boxes/user/box2/versions");
            then.status(500);
        });

        let client = CloudClient::with_base_url(server.url("/api/v1"))?;

        assert!(
            client
                .create_version("user/box", "1.0", Some("desc"))
                .is_ok()
        );
        assert!(client.create_version("user/box2", "1.0", None).is_err());
        Ok(())
    }

    #[test]
    fn test_update_version() -> Result<(), MigratoryError> {
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

        let client = CloudClient::with_base_url(server.url("/api/v1"))?;

        assert!(
            client
                .update_version("user/box", "1.0", Some("desc"))
                .is_ok()
        );
        assert!(client.update_version("user/box2", "1.0", None).is_err());
        Ok(())
    }

    #[test]
    fn test_delete_version() -> Result<(), MigratoryError> {
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

        let client = CloudClient::with_base_url(server.url("/api/v1"))?;

        assert!(client.delete_version("user/box", "1.0").is_ok());
        assert!(client.delete_version("user/box2", "1.0").is_err());
        Ok(())
    }

    #[test]
    fn test_release_version() -> Result<(), MigratoryError> {
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

        let client = CloudClient::with_base_url(server.url("/api/v1"))?;

        assert!(client.release_version("user/box", "1.0").is_ok());
        assert!(client.release_version("user/box2", "1.0").is_err());
        Ok(())
    }

    #[test]
    fn test_revoke_version() -> Result<(), MigratoryError> {
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

        let client = CloudClient::with_base_url(server.url("/api/v1"))?;

        assert!(client.revoke_version("user/box", "1.0").is_ok());
        assert!(client.revoke_version("user/box2", "1.0").is_err());
        Ok(())
    }

    #[test]
    fn test_create_provider() -> Result<(), MigratoryError> {
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

        let client = CloudClient::with_base_url(server.url("/api/v1"))?;

        assert!(
            client
                .create_provider("user/box", "1.0", "virtualbox", None, None)
                .is_ok()
        );
        assert!(
            client
                .create_provider("user/box2", "1.0", "virtualbox", None, None)
                .is_err()
        );
        assert!(
            client
                .create_provider("user/box", "1.0", "virtualbox", Some("arm64"), Some(true))
                .is_ok()
        );
        assert!(
            client
                .create_provider("user/box", "1.0", "virtualbox", Some("amd64"), Some(false))
                .is_ok()
        );
        Ok(())
    }

    #[test]
    fn test_update_provider() -> Result<(), MigratoryError> {
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

        let client = CloudClient::with_base_url(server.url("/api/v1"))?;

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
        Ok(())
    }

    #[test]
    fn test_delete_provider() -> Result<(), MigratoryError> {
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

        let client = CloudClient::with_base_url(server.url("/api/v1"))?;

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
        Ok(())
    }

    #[test]
    fn test_get_upload_url() -> Result<(), MigratoryError> {
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

        let client = CloudClient::with_base_url(server.url("/api/v1"))?;

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
        Ok(())
    }

    #[test]
    fn test_upload_file() -> Result<(), MigratoryError> {
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

        let client = CloudClient::with_base_url(server.url("/api/v1"))?;

        let mut temp_file = tempfile::NamedTempFile::new()?;
        temp_file.write_all(b"test")?;

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
        Ok(())
    }

    #[test]
    fn test_update_provider_with_checksum_type() -> Result<(), MigratoryError> {
        let server = MockServer::start();
        let _mock_ok = server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v1/boxes/user/box/versions/1.0/providers/virtualbox");
            then.status(200);
        });

        let client = CloudClient::with_base_url(server.url("/api/v1"))?;
        assert!(
            client
                .update_provider("user/box", "1.0", "virtualbox", None, Some("md5"))
                .is_ok()
        );
        Ok(())
    }

    #[test]
    fn test_search_boxes_error() -> Result<(), MigratoryError> {
        let server = MockServer::start();
        let _mock_err = server.mock(|when, then| {
            when.method(GET).path("/api/v1/search");
            then.status(500);
        });

        let client = CloudClient::with_base_url(server.url("/api/v1"))?;
        assert!(client.search_boxes("query").is_err());
        Ok(())
    }

    #[test]
    fn test_all_network_errors() -> Result<(), MigratoryError> {
        let client = CloudClient::with_base_url("http://127.0.0.1:0".to_string())?;

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
                .create_provider("user/box", "1.0", "virtualbox", None, None)
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

        let temp_file = tempfile::NamedTempFile::new()?;
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
        Ok(())
    }

    #[test]
    fn test_authenticate_success() -> Result<(), MigratoryError> {
        let server = MockServer::start();
        let mock_ok = server.mock(|when, then| {
            when.method(POST)
                .path("/api/v1/authenticate")
                .header("Authorization", "Basic dXNlcm5hbWU6cGFzc3dvcmQ=");
            then.status(200).json_body(serde_json::json!({
                "token": "test_token"
            }));
        });

        let client = CloudClient::with_base_url(server.url("/api/v1"))?;
        let token = client.authenticate("username", "password", Some("my token"))?;
        assert_eq!(token, "test_token");
        mock_ok.assert();
        Ok(())
    }

    #[test]
    fn test_authenticate_failure() -> Result<(), MigratoryError> {
        let server = MockServer::start();
        let mock_err = server.mock(|when, then| {
            when.method(POST).path("/api/v1/authenticate");
            then.status(401);
        });

        let client = CloudClient::with_base_url(server.url("/api/v1"))?;
        assert!(client.authenticate("user", "pass", None).is_err());
        mock_err.assert();
        Ok(())
    }

    #[test]
    fn test_authenticate_json_error() -> Result<(), MigratoryError> {
        let server = MockServer::start();
        let mock_bad_json = server.mock(|when, then| {
            when.method(POST).path("/api/v1/authenticate");
            then.status(200).body("not json");
        });

        let client = CloudClient::with_base_url(server.url("/api/v1"))?;
        assert!(client.authenticate("user", "pass", None).is_err());
        mock_bad_json.assert();
        Ok(())
    }

    #[test]
    fn test_search_boxes_success_and_json_error() -> Result<(), MigratoryError> {
        let server = MockServer::start();
        let _mock_ok = server.mock(|when, then| {
            when.method(GET).path("/api/v1/search").query_param("q", "query");
            then.status(200).body(r#"{"boxes": [{"name": "user/box", "description_markdown": "", "short_description": "", "versions": []}]}"#);
        });

        let _mock_err = server.mock(|when, then| {
            when.method(GET)
                .path("/api/v1/search")
                .query_param("q", "bad");
            then.status(200).body("invalid json");
        });

        let client = CloudClient::with_base_url(server.url("/api/v1"))?;

        let boxes = client.search_boxes("query")?;
        assert_eq!(boxes.len(), 1);
        assert_eq!(boxes[0].name, "user/box");

        assert!(client.search_boxes("bad").is_err());
        Ok(())
    }

    /// Tests `CloudClient::new` when environment variables are unset.
    #[test]
    fn test_cloud_client_new_env_fallbacks() {
        let _ = CloudClient::new();
        let cfg = CloudClientConfig::from_parts(None, None, None);
        let client = CloudClient::with_config(cfg);
        assert!(client.is_ok());
    }

    /// Tests reading token from file when `VAGRANT_CLOUD_TOKEN` is unset.
    #[test]
    fn test_cloud_client_token_from_file() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let token_dir = dir.path().join("data");
        std::fs::create_dir_all(&token_dir)?;
        std::fs::write(token_dir.join("token"), "file-token-123\n")?;

        let cfg = CloudClientConfig::from_parts(None, None, dir.path().to_str());
        let client = CloudClient::with_config(cfg)?;
        assert_eq!(client.token, Some("file-token-123".to_string()));

        Ok(())
    }

    /// Tests `whoami` and `delete_token` with empty token.
    #[test]
    fn test_empty_token_branches() -> Result<(), MigratoryError> {
        let mut client = CloudClient::new()?;
        client.token = Some(String::new());

        assert!(client.whoami().is_err());
        assert!(client.delete_token().is_ok());
        Ok(())
    }

    /// Tests `create_box` with `None` descriptions and failure status.
    #[test]
    fn test_create_box_none_descriptions_and_failure() -> Result<(), MigratoryError> {
        let server = MockServer::start();
        let _create_mock = server.mock(|when, then| {
            when.method(POST).path("/api/v1/boxes");
            then.status(500);
        });

        let client = CloudClient::with_base_url(server.url("/api/v1"))?;
        let res = client.create_box("user/box", None, None);
        assert!(res.is_err());
        Ok(())
    }

    /// Tests network errors in `whoami` and `delete_token`.
    #[test]
    fn test_whoami_and_delete_token_network_errors() -> Result<(), MigratoryError> {
        let mut client =
            CloudClient::with_base_url("http://invalid.local.domain.test:12345".to_string())?;
        client.token = Some("token123".to_string());

        assert!(client.whoami().is_err());
        assert!(client.delete_token().is_err());
        Ok(())
    }
}
