//! Config module for parsing and evaluating Vagrantfiles.
//!
//! This module defines the data structures used to represent the state and
//! configuration parsed from a `Vagrantfile`, evaluated via the pure-Rust
//! in-process parser (`in_process`) or a system Ruby subprocess (`parser`).

use crate::error::MigratoryError;
use std::collections::HashMap;

/// Strongly-typed provider and provisioner option configurations.
pub mod options;

/// Pure-Rust in-process Vagrantfile parser and evaluator.
pub mod in_process;
/// Module defining the embedded ruby parser logic.
pub mod parser;

/// Machine processor architecture.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Architecture {
    /// 64-bit x86 architecture (x86_64 / amd64).
    Amd64,
    /// 64-bit ARM architecture (aarch64 / arm64).
    Arm64,
    /// Unknown or custom architecture.
    Unknown(String),
}

impl Architecture {
    /// Parses an architecture string into an [`Architecture`] variant.
    ///
    /// # Arguments
    ///
    /// * `s` - The architecture name string.
    ///
    /// # Returns
    ///
    /// Returns the parsed [`Architecture`].
    pub fn from_str_lenient(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "x86_64" | "amd64" | "x64" => Self::Amd64,
            "aarch64" | "arm64" => Self::Arm64,
            other => Self::Unknown(other.to_string()),
        }
    }

    /// Returns the canonical string slice representation.
    ///
    /// # Returns
    ///
    /// String slice of the architecture name.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Amd64 => "amd64",
            Self::Arm64 => "arm64",
            Self::Unknown(s) => s.as_str(),
        }
    }
}

impl std::fmt::Display for Architecture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Supported virtual machine guest operating system types.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum GuestKind {
    /// Linux distributions (Ubuntu, Debian, RHEL, Fedora, etc.).
    Linux,
    /// macOS / Darwin.
    Darwin,
    /// Solaris and illumos-derived systems (e.g. OmniOS).
    Solaris,
    /// Microsoft Windows.
    Windows,
    /// BSD systems (FreeBSD, OpenBSD, NetBSD).
    Bsd,
    /// Custom or unrecognized guest OS identifier.
    Custom(String),
}

impl GuestKind {
    /// Parses a guest identifier string into a [`GuestKind`].
    ///
    /// # Arguments
    ///
    /// * `s` - The guest operating system name or symbol.
    ///
    /// # Returns
    ///
    /// Returns `Some(GuestKind)` if recognized as a built-in guest, or `None`.
    pub fn from_str_lenient(s: &str) -> Option<Self> {
        let clean = s.trim_start_matches(':').to_ascii_lowercase();
        match clean.as_str() {
            "linux" | "ubuntu" | "debian" | "centos" | "fedora" | "rhel" | "arch" | "alpine" => {
                Some(Self::Linux)
            }
            "darwin" | "macos" | "osx" => Some(Self::Darwin),
            "solaris" | "illumos" | "omnios" => Some(Self::Solaris),
            "windows" => Some(Self::Windows),
            "bsd" | "freebsd" | "openbsd" | "netbsd" => Some(Self::Bsd),
            _ => None,
        }
    }

    /// Parses any guest identifier string into a [`GuestKind`], falling back to [`GuestKind::Custom`].
    ///
    /// # Arguments
    ///
    /// * `s` - Guest OS identifier string.
    ///
    /// # Returns
    ///
    /// Returns the parsed [`GuestKind`].
    pub fn parse_raw(s: &str) -> Self {
        let clean = s.trim_start_matches(':');
        Self::from_str_lenient(clean).unwrap_or_else(|| Self::Custom(clean.to_string()))
    }

    /// Returns canonical guest name slice.
    ///
    /// # Returns
    ///
    /// String slice representing the guest OS name.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Linux => "linux",
            Self::Darwin => "darwin",
            Self::Solaris => "solaris",
            Self::Windows => "windows",
            Self::Bsd => "bsd",
            Self::Custom(s) => s.as_str(),
        }
    }
}

impl std::fmt::Display for GuestKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl PartialEq<&str> for GuestKind {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl std::ops::Deref for GuestKind {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

/// Supported communicator transport protocols.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum CommunicatorKind {
    /// SSH communicator.
    Ssh,
    /// WinRM communicator.
    Winrm,
    /// Docker exec communicator.
    Docker,
    /// Custom or plugin-provided communicator.
    Custom(String),
}

impl CommunicatorKind {
    /// Parses a communicator identifier string into a [`CommunicatorKind`].
    ///
    /// # Arguments
    ///
    /// * `s` - Communicator name string.
    ///
    /// # Returns
    ///
    /// Returns `Some(CommunicatorKind)` if recognized as a built-in communicator, or `None`.
    pub fn from_str_lenient(s: &str) -> Option<Self> {
        let clean = s.trim_start_matches(':').to_ascii_lowercase();
        match clean.as_str() {
            "ssh" => Some(Self::Ssh),
            "winrm" => Some(Self::Winrm),
            "docker" => Some(Self::Docker),
            _ => None,
        }
    }

    /// Parses any communicator identifier string into a [`CommunicatorKind`], falling back to [`CommunicatorKind::Custom`].
    ///
    /// # Arguments
    ///
    /// * `s` - Communicator identifier string.
    ///
    /// # Returns
    ///
    /// Returns the parsed [`CommunicatorKind`].
    pub fn parse_raw(s: &str) -> Self {
        let clean = s.trim_start_matches(':');
        Self::from_str_lenient(clean).unwrap_or_else(|| Self::Custom(clean.to_string()))
    }

    /// Returns canonical communicator name slice.
    ///
    /// # Returns
    ///
    /// String slice representing the communicator protocol.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Ssh => "ssh",
            Self::Winrm => "winrm",
            Self::Docker => "docker",
            Self::Custom(s) => s.as_str(),
        }
    }
}

impl std::fmt::Display for CommunicatorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl PartialEq<&str> for CommunicatorKind {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl std::ops::Deref for CommunicatorKind {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

/// Supported hypervisor and container virtualization providers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub enum ProviderKind {
    /// UTM provider (Apple Silicon / macOS).
    Utm,
    /// QEMU provider.
    Qemu,
    /// Oracle VirtualBox provider.
    #[default]
    VirtualBox,
    /// VMware Desktop / Fusion / Workstation provider.
    Vmware,
    /// Parallels Desktop provider.
    Parallels,
    /// Libvirt (KVM / QEMU) provider.
    Libvirt,
    /// Microsoft Hyper-V provider.
    Hyperv,
    /// Docker container provider.
    Docker,
    /// Custom or plugin-provided provider.
    Custom(String),
}

impl ProviderKind {
    /// Parses a provider identifier string into a [`ProviderKind`].
    ///
    /// # Arguments
    ///
    /// * `s` - Provider name string or symbol.
    ///
    /// # Returns
    ///
    /// Returns `Some(ProviderKind)` if recognized as a built-in provider, or `None`.
    pub fn from_str_lenient(s: &str) -> Option<Self> {
        let clean = s.trim_start_matches(':').to_ascii_lowercase();
        match clean.as_str() {
            "utm" => Some(Self::Utm),
            "qemu" => Some(Self::Qemu),
            "virtualbox" | "vbox" => Some(Self::VirtualBox),
            "vmware" | "vmware_desktop" | "vmware_fusion" | "vmware_workstation" => {
                Some(Self::Vmware)
            }
            "parallels" => Some(Self::Parallels),
            "libvirt" => Some(Self::Libvirt),
            "hyperv" | "hyper_v" => Some(Self::Hyperv),
            "docker" => Some(Self::Docker),
            _ => None,
        }
    }

    /// Parses any provider identifier string into a [`ProviderKind`], falling back to [`ProviderKind::Custom`].
    ///
    /// # Arguments
    ///
    /// * `s` - Provider identifier string.
    ///
    /// # Returns
    ///
    /// Returns the parsed [`ProviderKind`].
    pub fn parse_raw(s: &str) -> Self {
        let clean = s.trim_start_matches(':');
        Self::from_str_lenient(clean).unwrap_or_else(|| Self::Custom(clean.to_string()))
    }

    /// Returns canonical provider name slice.
    ///
    /// # Returns
    ///
    /// String slice representing the canonical provider name.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Utm => "utm",
            Self::Qemu => "qemu",
            Self::VirtualBox => "virtualbox",
            Self::Vmware => "vmware_desktop",
            Self::Parallels => "parallels",
            Self::Libvirt => "libvirt",
            Self::Hyperv => "hyperv",
            Self::Docker => "docker",
            Self::Custom(s) => s.as_str(),
        }
    }
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl PartialEq<&str> for ProviderKind {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for ProviderKind {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other.as_str()
    }
}

impl AsRef<str> for ProviderKind {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<std::path::Path> for ProviderKind {
    fn as_ref(&self) -> &std::path::Path {
        std::path::Path::new(self.as_str())
    }
}

impl From<&str> for ProviderKind {
    fn from(s: &str) -> Self {
        Self::parse_raw(s)
    }
}

impl From<String> for ProviderKind {
    fn from(s: String) -> Self {
        Self::parse_raw(&s)
    }
}

impl std::ops::Deref for ProviderKind {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

/// Synced folder backend mechanism.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SyncedFolderType {
    /// Disabled synced folder.
    Disabled,
    /// Rsync one-way synchronization.
    Rsync,
    /// VirtioFS / VirtFS shared filesystem.
    Virtiofs,
    /// 9P (Plan 9) VirtFS shared filesystem (`9p`).
    Plan9,
    /// SMB / CIFS network share.
    Smb,
    /// VirtualBox Shared Folders (vboxsf).
    VirtualBox,
    /// Network File System (NFS).
    Nfs,
    /// Custom or unrecognized synced folder type.
    Custom(String),
}

impl SyncedFolderType {
    /// Parses a synced folder type string into a [`SyncedFolderType`].
    ///
    /// # Arguments
    ///
    /// * `s` - Synced folder type identifier.
    ///
    /// # Returns
    ///
    /// Returns `Some(SyncedFolderType)` if recognized as a built-in type, or `None`.
    pub fn from_str_lenient(s: &str) -> Option<Self> {
        let clean = s.trim_start_matches(':').to_ascii_lowercase();
        match clean.as_str() {
            "disabled" => Some(Self::Disabled),
            "rsync" => Some(Self::Rsync),
            "virtiofs" | "virtfs" => Some(Self::Virtiofs),
            "9p" | "plan9" => Some(Self::Plan9),
            "smb" => Some(Self::Smb),
            "virtualbox" | "vbox" | "vboxsf" => Some(Self::VirtualBox),
            "nfs" => Some(Self::Nfs),
            _ => None,
        }
    }

    /// Parses any synced folder type string into a [`SyncedFolderType`], falling back to [`SyncedFolderType::Custom`].
    ///
    /// # Arguments
    ///
    /// * `s` - Synced folder type identifier.
    ///
    /// # Returns
    ///
    /// Returns the parsed [`SyncedFolderType`].
    pub fn parse_raw(s: &str) -> Self {
        let clean = s.trim_start_matches(':');
        Self::from_str_lenient(clean).unwrap_or_else(|| Self::Custom(clean.to_string()))
    }

    /// Returns canonical synced folder type string slice.
    ///
    /// # Returns
    ///
    /// String slice representing the synced folder type.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Disabled => "disabled",
            Self::Rsync => "rsync",
            Self::Virtiofs => "virtiofs",
            Self::Plan9 => "9p",
            Self::Smb => "smb",
            Self::VirtualBox => "virtualbox",
            Self::Nfs => "nfs",
            Self::Custom(s) => s.as_str(),
        }
    }
}

impl std::fmt::Display for SyncedFolderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl PartialEq<&str> for SyncedFolderType {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl std::ops::Deref for SyncedFolderType {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

/// Global Vagrant configuration.
///
/// Contains settings that apply globally to the Vagrant environment, such as
/// required plugins or host-specific configurations.
#[derive(Debug, Clone, Default)]
pub struct VagrantConfig {
    /// Host configuration.
    pub host: Option<String>,
    /// Any plugins required.
    pub plugins: Vec<String>,
    /// Sensitive patterns or strings to mask from logs.
    pub sensitive: Vec<String>,
}

/// SSH configuration namespace.
///
/// Holds all settings related to connecting to the VM via SSH.
#[derive(Debug, Clone)]
pub struct SshConfig {
    /// SSH username.
    pub username: String,
    /// SSH password.
    pub password: Option<String>,
    /// SSH host.
    pub host: String,
    /// SSH port.
    pub port: u16,
    /// Guest port forwarded for SSH (defaults to 22).
    pub guest_port: Option<u16>,
    /// Path to private key.
    pub private_key_path: Option<std::path::PathBuf>,
    /// Insert key on boot.
    pub insert_key: bool,
    /// Forward agent.
    pub forward_agent: bool,
    /// Forward X11.
    pub forward_x11: bool,
    /// Proxy command.
    pub proxy_command: Option<String>,
    /// Extra arguments forwarded directly to the SSH client.
    pub extra_args: Option<Vec<String>>,
    /// Environment variables to forward over SSH.
    pub forward_env: Vec<String>,
    /// Request pseudo-terminal allocation.
    pub pty: bool,
    /// Enable TCP keepalive packets.
    pub keep_alive: bool,
    /// Custom shell to use on guest.
    pub shell: Option<String>,
    /// Template for exporting environment variables.
    pub export_command_template: Option<String>,
    /// Timeout for establishing SSH connection.
    pub connect_timeout: Option<u64>,
    /// Timeout for executing SSH commands.
    pub timeout: Option<u64>,
    /// Verify guest host key.
    pub verify_host_key: bool,
    /// Authenticate using only specified keys.
    pub keys_only: bool,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            username: "vagrant".to_string(),
            password: None,
            host: "127.0.0.1".to_string(),
            port: 2222,
            guest_port: Some(22),
            private_key_path: None,
            insert_key: true,
            forward_agent: false,
            forward_x11: false,
            proxy_command: None,
            extra_args: None,
            forward_env: Vec::new(),
            pty: false,
            keep_alive: true,
            shell: None,
            export_command_template: None,
            connect_timeout: None,
            timeout: None,
            verify_host_key: false,
            keys_only: true,
        }
    }
}

/// WinRM configuration namespace.
///
/// Holds settings for connecting to a Windows VM using WinRM.
#[derive(Debug, Clone)]
pub struct WinrmConfig {
    /// WinRM username.
    pub username: String,
    /// WinRM password.
    pub password: Option<String>,
    /// WinRM host.
    pub host: String,
    /// WinRM port.
    pub port: u16,
    /// Guest port forwarded for WinRM (defaults to 5985).
    pub guest_port: Option<u16>,
    /// Use HTTPS?
    pub ssl: bool,
    /// Transport (e.g. "negotiate", "ssl", "plaintext").
    pub transport: Option<String>,
    /// Only allow HTTP Basic authentication.
    pub basic_auth_only: bool,
    /// Verify SSL peer certificates.
    pub ssl_peer_verification: bool,
    /// Timeout in seconds.
    pub timeout: Option<u64>,
    /// Retry limit for failed commands.
    pub retry_limit: Option<u32>,
    /// Delay between retries in seconds.
    pub retry_delay: Option<u64>,
    /// Execution time limit for commands (e.g. "PT2H").
    pub execution_time_limit: Option<String>,
}

impl Default for WinrmConfig {
    fn default() -> Self {
        Self {
            username: "vagrant".to_string(),
            password: None,
            host: "127.0.0.1".to_string(),
            port: 5985,
            guest_port: Some(5985),
            ssl: false,
            transport: None,
            basic_auth_only: false,
            ssl_peer_verification: true,
            timeout: None,
            retry_limit: None,
            retry_delay: None,
            execution_time_limit: None,
        }
    }
}

/// Provider specific configuration.
///
/// Stores configuration overrides specific to a particular provider (e.g., virtualbox).
#[derive(Debug, Clone, Default)]
pub struct ProviderConfig {
    /// Provider type (e.g. `ProviderKind::VirtualBox`)
    pub name: ProviderKind,
    /// Provider specific options
    pub options: HashMap<String, String>,
}

impl ProviderConfig {
    /// Parses internal map into strong Docker type.
    pub fn as_docker(
        &self,
    ) -> Result<crate::config::options::TypedProviderOptions, crate::error::MigratoryError> {
        let cmd = self.options.get("cmd").cloned();
        let build_dir = self.options.get("build_dir").cloned();
        let init = self.options.get("init").map(String::as_str) == Some("true");
        let has_init = self.options.get("has_init").map(String::as_str) == Some("true");
        let privileged = self.options.get("privileged").map(String::as_str) == Some("true");

        Ok(crate::config::options::TypedProviderOptions::Docker {
            cmd,
            build_dir,
            init,
            has_init,
            privileged,
        })
    }

    /// Parses internal map into strong VirtualBox type.
    pub fn as_virtualbox(
        &self,
    ) -> Result<crate::config::options::TypedProviderOptions, crate::error::MigratoryError> {
        let boot_mode = self.options.get("boot_mode").cloned();
        let gui = self
            .options
            .get("gui")
            .map(|s| s == "true")
            .unwrap_or(false);
        let separate = self
            .options
            .get("separate")
            .map(|s| s == "true")
            .unwrap_or(false);
        let vrde = self
            .options
            .get("vrde")
            .map(|s| s == "true")
            .unwrap_or(false);
        let vrdeport = self.options.get("vrdeport").cloned();
        let memory = self.options.get("memory").cloned();
        let cpus = self.options.get("cpus").cloned();

        Ok(crate::config::options::TypedProviderOptions::VirtualBox {
            boot_mode,
            gui,
            separate,
            vrde,
            vrdeport,
            memory,
            cpus,
        })
    }
}

/// Provisioner specific configuration.
///
/// Stores configuration details for provisioners (e.g., shell, ansible).
#[derive(Debug, Clone, Default)]
pub struct ProvisionerConfig {
    /// Provisioner type (e.g. "shell")
    pub name: String,
    /// The provisioner configuration block or string.
    pub config: HashMap<String, String>,
    /// Optional identifier for named provisioners.
    pub id: Option<String>,
    /// Execution timing: "once", "always", or "never".
    pub run: Option<String>,
}

impl ProvisionerConfig {
    /// Parses internal map into strong Docker type.
    pub fn as_docker(
        &self,
    ) -> Result<crate::config::options::TypedProvisionerConfig, crate::error::MigratoryError> {
        let build_image = self.config.get("build_image").cloned();
        let build_path = self.config.get("build_path").cloned();
        let run = self.config.get("run").cloned();
        let compose = self.config.get("compose").cloned();
        let images = if let Some(s) = self.config.get("images") {
            s.split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect()
        } else {
            vec![]
        };
        let install = self
            .config
            .get("install")
            .map(|s| s == "true" || s == "1")
            .unwrap_or(true);
        Ok(crate::config::options::TypedProvisionerConfig::Docker {
            build_image,
            build_path,
            run,
            compose,
            images,
            install,
        })
    }

    /// Parses internal map into strong Shell type.
    pub fn as_shell(
        &self,
    ) -> Result<crate::config::options::TypedProvisionerConfig, crate::error::MigratoryError> {
        let inline = self.config.get("inline").cloned();
        let path = self.config.get("path").cloned();
        let args = self.config.get("args").cloned();
        let upload_path = self.config.get("upload_path").cloned();
        let powershell = self
            .config
            .get("powershell")
            .map(|s| s == "true")
            .unwrap_or(false);
        let powershell_args = self.config.get("powershell_args").cloned();
        let sensitive = self
            .config
            .get("sensitive")
            .map(|s| s == "true")
            .unwrap_or(false);
        let privileged = self
            .config
            .get("privileged")
            .map(|s| s == "true")
            .unwrap_or(false);
        let reboot = self
            .config
            .get("reboot")
            .map(|s| s == "true")
            .unwrap_or(false);
        let env = self.config.get("env").cloned();
        Ok(crate::config::options::TypedProvisionerConfig::Shell {
            inline,
            path,
            args,
            upload_path,
            powershell,
            powershell_args,
            sensitive,
            privileged,
            reboot,
            env,
        })
    }
}

/// Network configuration.
///
/// Represents different types of networking available for a VM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkConfig {
    /// Forwarded port.
    ForwardedPort {
        /// Guest port.
        guest: u16,
        /// Host port.
        host: u16,
        /// Auto-correct collisions.
        auto_correct: bool,
        /// Protocol (e.g. "tcp", "udp").
        protocol: Option<String>,
        /// Host IP to bind to.
        host_ip: Option<String>,
    },
    /// Private network (host-only).
    PrivateNetwork {
        /// IP Address.
        ip: Option<String>,
        /// Subnet mask.
        netmask: Option<String>,
        /// DHCP.
        dhcp: bool,
        /// VirtualBox internal network name.
        virtualbox_intnet: Option<String>,
    },
    /// Public network (bridged).
    PublicNetwork {
        /// IP Address.
        ip: Option<String>,
        /// Bridge interface name.
        bridge: Option<String>,
        /// Use DHCP assigned default route.
        use_dhcp_assigned_default_route: bool,
    },
}

impl NetworkConfig {
    /// Returns `(guest_port, host_port, protocol)` if this is a [`NetworkConfig::ForwardedPort`].
    ///
    /// # Returns
    ///
    /// Returns `Some((guest, host, protocol))` or `None`.
    pub fn as_forwarded_port(&self) -> Option<(u16, u16, &str)> {
        match self {
            Self::ForwardedPort {
                guest,
                host,
                protocol,
                ..
            } => Some((*guest, *host, protocol.as_deref().unwrap_or("tcp"))),
            _ => None,
        }
    }

    /// Returns `(ip, netmask)` if this is a [`NetworkConfig::PrivateNetwork`] with an IP address.
    ///
    /// # Returns
    ///
    /// Returns `Some((ip, netmask))` or `None`.
    pub fn as_private_network(&self) -> Option<(&str, &str)> {
        match self {
            Self::PrivateNetwork {
                ip: Some(ip),
                netmask,
                ..
            } => Some((ip.as_str(), netmask.as_deref().unwrap_or("255.255.255.0"))),
            _ => None,
        }
    }
}

/// Trigger configuration.
///
/// Holds settings for lifecycle triggers attached to commands or actions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TriggerConfig {
    /// Stage: `before`, `after`, or `on_error`.
    pub stage: String,
    /// Actions: e.g. `["up", "halt", "destroy", "provision", "reload", "package", "snapshot"]`.
    pub actions: Vec<String>,
    /// Inline command to run on host.
    pub run_inline: Option<String>,
    /// Path to script to run on host.
    pub run_path: Option<String>,
    /// Inline command to run on guest.
    pub run_remote_inline: Option<String>,
    /// Path to script to run on guest.
    pub run_remote_path: Option<String>,
    /// Informational message to print to UI console.
    pub info: Option<String>,
    /// Warning message to print to UI console.
    pub warn: Option<String>,
    /// Ignore errors.
    pub ignore_errors: bool,
    /// Force execution even if previous actions failed.
    pub force: bool,
    /// Abort remaining execution pipeline if trigger exits non-zero.
    pub abort: bool,
    /// Target filtering by machine name or regex patterns.
    pub only_on: Vec<String>,
    /// On error behavior: e.g. "halt" or "continue".
    pub on_error: Option<String>,
    /// Environment variables to pass to the trigger.
    pub env: HashMap<String, String>,
}

/// Synced folder configuration.
///
/// Holds settings for syncing folders between the host and the guest.
#[derive(Debug, Clone, Default)]
pub struct SyncedFolderConfig {
    /// Host path.
    pub host_path: std::path::PathBuf,
    /// Guest path.
    pub guest_path: std::path::PathBuf,
    /// Folder type (e.g. `SyncedFolderType::VirtualBox`, `SyncedFolderType::Nfs`, `SyncedFolderType::Rsync`).
    pub folder_type: Option<SyncedFolderType>,
    /// Disabled flag.
    pub disabled: bool,
    /// Owner of the synced folder.
    pub owner: Option<String>,
    /// Group of the synced folder.
    pub group: Option<String>,
    /// Mount options.
    pub mount_options: Option<Vec<String>>,
    /// Additional generic arguments (e.g., rsync args).
    pub args: Option<Vec<String>>,
}

/// Disk configuration for a virtual machine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiskConfig {
    /// Type of disk (e.g., "disk", "dvd").
    pub disk_type: String,
    /// Size of the disk (e.g., "20GB").
    pub size: Option<String>,
    /// Name or identifier for the disk.
    pub name: Option<String>,
    /// Optional backing disk file path (e.g., ISO or VMDK path).
    pub file: Option<std::path::PathBuf>,
    /// Primary master or controller options.
    pub primary: bool,
}

/// VM configuration namespace.
///
/// The core settings block defining the characteristics of the VM.
#[derive(Debug, Clone)]
pub struct VmConfig {
    /// The box to use.
    pub box_name: Option<String>,
    /// Box version.
    pub box_version: Option<String>,
    /// Box URL.
    pub box_url: Option<String>,
    /// Check for box updates.
    pub box_check_update: Option<bool>,
    /// Box download checksum.
    pub box_download_checksum: Option<String>,
    /// Box download checksum type.
    pub box_download_checksum_type: Option<String>,
    /// Box download client certificate path.
    pub box_download_client_cert: Option<std::path::PathBuf>,
    /// Box download CA certificate path.
    pub box_download_ca_cert: Option<std::path::PathBuf>,
    /// Allow insecure downloads without verifying SSL.
    pub box_download_insecure: Option<bool>,
    /// Hostname of the VM.
    pub hostname: Option<String>,
    /// Explicit guest OS override (e.g. `GuestKind::Linux`, `GuestKind::Windows`, `GuestKind::Bsd`).
    pub guest: Option<GuestKind>,
    /// Post-up message printed after machine is booted.
    pub post_up_message: Option<String>,
    /// Boot timeout.
    pub boot_timeout: Option<u64>,
    /// Communicator.
    pub communicator: Option<CommunicatorKind>,
    /// Graceful halt timeout.
    pub graceful_halt_timeout: Option<u64>,
    /// Networks defined for this VM.
    pub networks: Vec<NetworkConfig>,
    /// Provider configurations.
    pub providers: Vec<ProviderConfig>,
    /// Provisioners for this VM.
    pub provisioners: Vec<ProvisionerConfig>,
    /// Synced folders for this VM.
    pub synced_folders: Vec<SyncedFolderConfig>,
    /// Usable port range for automatic port collision resolution (default 2200..2250).
    pub usable_port_range: (u16, u16),
    /// Allowed synced folder types (e.g., `["rsync", "nfs"]`).
    pub allowed_synced_folder_types: Option<Vec<String>>,
    /// Additional disks configured for the VM.
    pub disks: Vec<DiskConfig>,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            box_name: None,
            box_version: None,
            box_url: None,
            box_check_update: None,
            box_download_checksum: None,
            box_download_checksum_type: None,
            box_download_client_cert: None,
            box_download_ca_cert: None,
            box_download_insecure: None,
            hostname: None,
            guest: None,
            post_up_message: None,
            boot_timeout: None,
            communicator: None,
            graceful_halt_timeout: None,
            networks: Vec::new(),
            providers: Vec::new(),
            provisioners: Vec::new(),
            synced_folders: Vec::new(),
            usable_port_range: (2200, 2250),
            allowed_synced_folder_types: None,
            disks: Vec::new(),
        }
    }
}

/// Configuration for application push deployments (`config.push.define`).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct PushConfig {
    /// Strategy name or identifier (e.g. "local-exec", "ftp", "sftp", "atlas", "vagrant-cloud").
    pub name: String,
    /// Strategy type to execute.
    pub strategy: String,
    /// Strategy options key-value mapping.
    pub options: HashMap<String, String>,
}

/// Complete machine configuration.
///
/// Brings together all configuration namespaces for a single machine definition.
#[derive(Debug, Clone)]
pub struct MachineConfig {
    /// The name of the machine (e.g. "default", "web").
    pub name: String,
    /// Whether this machine is the primary machine in a multi-machine setup.
    pub primary: bool,
    /// Whether this machine should automatically start on `vagrant up`.
    pub autostart: bool,
    /// Machine names this machine depends on for multi-machine ordering.
    pub depends_on: Vec<String>,
    /// The Vagrant global config.
    pub vagrant: VagrantConfig,
    /// The SSH config.
    pub ssh: SshConfig,
    /// The WinRM config.
    pub winrm: WinrmConfig,
    /// The VM config.
    pub vm: VmConfig,
    /// Configured triggers for this machine.
    pub triggers: Vec<TriggerConfig>,
    /// Configured push strategies for this machine.
    pub pushes: Vec<PushConfig>,
}

impl Default for MachineConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            primary: false,
            autostart: true,
            depends_on: Vec::new(),
            vagrant: VagrantConfig::default(),
            ssh: SshConfig::default(),
            winrm: WinrmConfig::default(),
            vm: VmConfig::default(),
            triggers: Vec::new(),
            pushes: Vec::new(),
        }
    }
}

/// A parsed environment (Vagrantfile output).
///
/// Represents the entirely loaded state of a Vagrant environment, which may
/// contain multiple machine definitions.
#[derive(Debug, Clone, Default)]
pub struct EnvironmentConfig {
    /// Default machine, or multiple if defined.
    pub machines: HashMap<String, MachineConfig>,
    /// Push configurations defined in the environment.
    pub pushes: Vec<PushConfig>,
}

/// Evaluates a Vagrantfile and builds the environment configuration.
///
/// Embeds Ruby via `rutie` to parse the `Vagrantfile`.
///
/// # Arguments
///
/// * `path` - The path to the Vagrantfile.
///
/// # Returns
///
/// Returns an `EnvironmentConfig` loaded from the file.
///
/// # Errors
///
/// Returns a `MigratoryError` if evaluation fails.
pub fn evaluate_vagrantfile(path: &str) -> Result<EnvironmentConfig, crate::error::MigratoryError> {
    parser::parse_vagrantfile(path)
}

/// Loads and evaluates the `Vagrantfile` in `cwd`, returning the resulting [`EnvironmentConfig`].
///
/// Resolves the `Vagrantfile` path via [`get_vagrantfile_path`], verifies that it exists,
/// and evaluates it via [`evaluate_vagrantfile`].
///
/// # Arguments
///
/// * `cwd` - The project root directory containing the `Vagrantfile`.
///
/// # Returns
///
/// Returns the evaluated [`EnvironmentConfig`].
///
/// # Errors
///
/// Returns [`crate::error::MigratoryError::NotFound`] if the `Vagrantfile` does not exist,
/// or a [`crate::error::MigratoryError`] if `Vagrantfile` evaluation fails.
pub fn load_environment_config(
    cwd: &std::path::Path,
) -> Result<EnvironmentConfig, crate::error::MigratoryError> {
    let path = get_vagrantfile_path(cwd);
    if !path.exists() {
        return Err(crate::error::MigratoryError::NotFound(
            "Vagrantfile".to_string(),
        ));
    }
    evaluate_vagrantfile(&path.to_string_lossy())
}

/// Returns the Vagrant home directory, respecting `VAGRANT_HOME` or defaulting to `~/.vagrant.d`.
pub fn get_vagrant_home() -> std::path::PathBuf {
    get_vagrant_home_from(
        std::env::var("VAGRANT_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    )
}

/// Resolves the Vagrant home directory from explicit optional `VAGRANT_HOME` and `HOME` strings.
pub fn get_vagrant_home_from(vagrant_home: Option<&str>, home: Option<&str>) -> std::path::PathBuf {
    if let Some(vh) = vagrant_home
        && !vh.trim().is_empty()
    {
        return std::path::PathBuf::from(vh);
    }
    if let Some(user_home) = home
        && !user_home.trim().is_empty()
    {
        return std::path::PathBuf::from(user_home).join(".vagrant.d");
    }
    std::path::PathBuf::from(".vagrant.d")
}

/// Resolves all Vagrantfiles in the evaluation hierarchy in order of precedence:
/// 1. Box-level internal Vagrantfile (`include/Vagrantfile`)
/// 2. Base box Vagrantfile (`Vagrantfile`)
/// 3. Global user Vagrantfile (`~/.vagrant.d/Vagrantfile` or `$VAGRANT_HOME/Vagrantfile`)
/// 4. Project Vagrantfile (`./Vagrantfile` or `$VAGRANT_CWD/$VAGRANT_VAGRANTFILE`)
///
/// # Arguments
///
/// * `project_dir` - The directory of the current project.
/// * `box_dir` - Optional path to an installed box directory.
///
/// # Returns
///
/// A vector of existing `PathBuf`s in execution order.
pub fn resolve_vagrantfile_hierarchy(
    project_dir: &std::path::Path,
    box_dir: Option<&std::path::Path>,
) -> Vec<std::path::PathBuf> {
    resolve_vagrantfile_hierarchy_with_home(project_dir, box_dir, &get_vagrant_home())
}

/// Resolves all Vagrantfiles in the evaluation hierarchy with an explicit Vagrant home directory.
pub fn resolve_vagrantfile_hierarchy_with_home(
    project_dir: &std::path::Path,
    box_dir: Option<&std::path::Path>,
    vagrant_home: &std::path::Path,
) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();

    // 1. Box-level internal Vagrantfile
    if let Some(bdir) = box_dir {
        let include_vf = bdir.join("include").join("Vagrantfile");
        if include_vf.is_file() {
            files.push(include_vf);
        }
        // 2. Base box Vagrantfile
        let base_vf = bdir.join("Vagrantfile");
        if base_vf.is_file() {
            files.push(base_vf);
        }
    }

    // 3. User global Vagrantfile
    let global_vf = vagrant_home.join("Vagrantfile");
    if global_vf.is_file() {
        files.push(global_vf);
    }

    // 4. Project directory Vagrantfile
    let project_vf = get_vagrantfile_path(project_dir);
    if project_vf.is_file() {
        files.push(project_vf);
    }

    files
}

/// Resolves all Vagrantfiles in the evaluation hierarchy in order of precedence:
/// 1. Built-in defaults provided by Migratory core (handled in config models)
/// 2. Box-level internal Vagrantfile (`include/Vagrantfile`)
/// 3. Base box Vagrantfile (`Vagrantfile`)
/// 4. User global Vagrantfile (`~/.vagrant.d/Vagrantfile` or `$VAGRANT_HOME/Vagrantfile`)
/// 5. Project root Vagrantfile (`./Vagrantfile` or `$VAGRANT_CWD/$VAGRANT_VAGRANTFILE`)
/// 6. CLI-specified Vagrantfile override
///
/// # Arguments
///
/// * `project_dir` - The directory of the current project.
/// * `box_dir` - Optional path to an installed box directory.
/// * `cli_override` - Optional command-line Vagrantfile override path.
///
/// # Returns
///
/// A vector of existing `PathBuf`s in execution order.
pub fn resolve_vagrantfile_hierarchy_with_cli(
    project_dir: &std::path::Path,
    box_dir: Option<&std::path::Path>,
    cli_override: Option<&std::path::Path>,
) -> Vec<std::path::PathBuf> {
    let mut files = resolve_vagrantfile_hierarchy(project_dir, box_dir);
    if let Some(cli_path) = cli_override
        && cli_path.is_file()
    {
        files.push(cli_path.to_path_buf());
    }
    files
}

/// Merges two `EnvironmentConfig` structs following the Vagrant cascade order.
///
/// The `override_conf` values take precedence over `base` values according to
/// Vagrant's 6-level cascade merge rules:
/// - Machine properties are overridden if specified in `override_conf`.
/// - Synced folders are deduplicated by `guest_path` (later override wins).
/// - Provisioners are appended in execution order.
/// - Networks are appended.
/// - Provider configurations are deep-merged by provider name.
/// - Plugins and sensitive values are unioned and deduplicated.
///
/// # Arguments
///
/// * `base` - The base configuration (e.g., box or user global).
/// * `override_conf` - The higher precedence configuration (e.g., project Vagrantfile).
///
/// # Returns
///
/// A new merged `EnvironmentConfig`.
pub fn merge_environment_configs(
    base: &EnvironmentConfig,
    override_conf: &EnvironmentConfig,
) -> EnvironmentConfig {
    let mut merged = base.clone();

    for (name, o_machine) in &override_conf.machines {
        let entry = merged
            .machines
            .entry(name.clone())
            .or_insert_with(|| MachineConfig {
                name: name.clone(),
                ..Default::default()
            });

        if o_machine.primary {
            entry.primary = true;
        }
        entry.autostart = o_machine.autostart;
        if !o_machine.depends_on.is_empty() {
            entry.depends_on = o_machine.depends_on.clone();
        }

        // Merge VM config
        if let Some(b) = &o_machine.vm.box_name {
            entry.vm.box_name = Some(b.clone());
        }
        if let Some(bv) = &o_machine.vm.box_version {
            entry.vm.box_version = Some(bv.clone());
        }
        if let Some(bu) = &o_machine.vm.box_url {
            entry.vm.box_url = Some(bu.clone());
        }
        if let Some(bcu) = o_machine.vm.box_check_update {
            entry.vm.box_check_update = Some(bcu);
        }
        if let Some(bdc) = &o_machine.vm.box_download_checksum {
            entry.vm.box_download_checksum = Some(bdc.clone());
        }
        if let Some(bdct) = &o_machine.vm.box_download_checksum_type {
            entry.vm.box_download_checksum_type = Some(bdct.clone());
        }
        if let Some(bdcc) = &o_machine.vm.box_download_client_cert {
            entry.vm.box_download_client_cert = Some(bdcc.clone());
        }
        if let Some(bdca) = &o_machine.vm.box_download_ca_cert {
            entry.vm.box_download_ca_cert = Some(bdca.clone());
        }
        if let Some(bdi) = o_machine.vm.box_download_insecure {
            entry.vm.box_download_insecure = Some(bdi);
        }
        if let Some(h) = &o_machine.vm.hostname {
            entry.vm.hostname = Some(h.clone());
        }
        if let Some(g) = &o_machine.vm.guest {
            entry.vm.guest = Some(g.clone());
        }
        if let Some(pum) = &o_machine.vm.post_up_message {
            entry.vm.post_up_message = Some(pum.clone());
        }
        if let Some(bt) = o_machine.vm.boot_timeout {
            entry.vm.boot_timeout = Some(bt);
        }
        if let Some(c) = &o_machine.vm.communicator {
            entry.vm.communicator = Some(c.clone());
        }
        if let Some(ght) = o_machine.vm.graceful_halt_timeout {
            entry.vm.graceful_halt_timeout = Some(ght);
        }
        if o_machine.vm.usable_port_range != (2200, 2250) {
            entry.vm.usable_port_range = o_machine.vm.usable_port_range;
        }
        if let Some(types) = &o_machine.vm.allowed_synced_folder_types {
            entry.vm.allowed_synced_folder_types = Some(types.clone());
        }
        entry.vm.disks.extend(o_machine.vm.disks.clone());

        // Networks: append
        entry.vm.networks.extend(o_machine.vm.networks.clone());

        // Provisioners: append in order
        entry
            .vm
            .provisioners
            .extend(o_machine.vm.provisioners.clone());

        // Synced folders: deduplicate by guest_path (override replaces base)
        for sf in &o_machine.vm.synced_folders {
            if let Some(idx) = entry
                .vm
                .synced_folders
                .iter()
                .position(|existing| existing.guest_path == sf.guest_path)
            {
                entry.vm.synced_folders[idx] = sf.clone();
            } else {
                entry.vm.synced_folders.push(sf.clone());
            }
        }

        // Providers: deep merge by provider name
        for o_prov in &o_machine.vm.providers {
            if let Some(existing_prov) = entry
                .vm
                .providers
                .iter_mut()
                .find(|p| p.name == o_prov.name)
            {
                existing_prov.options.extend(o_prov.options.clone());
            } else {
                entry.vm.providers.push(o_prov.clone());
            }
        }

        // SSH: merge fields
        if o_machine.ssh.username != "vagrant" {
            entry.ssh.username = o_machine.ssh.username.clone();
        }
        if o_machine.ssh.password.is_some() {
            entry.ssh.password = o_machine.ssh.password.clone();
        }
        if o_machine.ssh.host != "127.0.0.1" {
            entry.ssh.host = o_machine.ssh.host.clone();
        }
        if o_machine.ssh.port != 2222 {
            entry.ssh.port = o_machine.ssh.port;
        }
        if o_machine.ssh.guest_port.is_some() {
            entry.ssh.guest_port = o_machine.ssh.guest_port;
        }
        if o_machine.ssh.private_key_path.is_some() {
            entry.ssh.private_key_path = o_machine.ssh.private_key_path.clone();
        }
        if !o_machine.ssh.insert_key {
            entry.ssh.insert_key = false;
        }
        if o_machine.ssh.forward_agent {
            entry.ssh.forward_agent = true;
        }
        if o_machine.ssh.forward_x11 {
            entry.ssh.forward_x11 = true;
        }
        if o_machine.ssh.proxy_command.is_some() {
            entry.ssh.proxy_command = o_machine.ssh.proxy_command.clone();
        }
        if o_machine.ssh.extra_args.is_some() {
            entry.ssh.extra_args = o_machine.ssh.extra_args.clone();
        }
        for env_var in &o_machine.ssh.forward_env {
            if !entry.ssh.forward_env.contains(env_var) {
                entry.ssh.forward_env.push(env_var.clone());
            }
        }
        if o_machine.ssh.pty {
            entry.ssh.pty = true;
        }
        if !o_machine.ssh.keep_alive {
            entry.ssh.keep_alive = false;
        }
        if o_machine.ssh.shell.is_some() {
            entry.ssh.shell = o_machine.ssh.shell.clone();
        }
        if o_machine.ssh.export_command_template.is_some() {
            entry.ssh.export_command_template = o_machine.ssh.export_command_template.clone();
        }
        if o_machine.ssh.connect_timeout.is_some() {
            entry.ssh.connect_timeout = o_machine.ssh.connect_timeout;
        }
        if o_machine.ssh.timeout.is_some() {
            entry.ssh.timeout = o_machine.ssh.timeout;
        }
        if o_machine.ssh.verify_host_key {
            entry.ssh.verify_host_key = true;
        }
        if !o_machine.ssh.keys_only {
            entry.ssh.keys_only = false;
        }

        // WinRM: merge fields
        if o_machine.winrm.username != "vagrant" {
            entry.winrm.username = o_machine.winrm.username.clone();
        }
        if o_machine.winrm.password.is_some() {
            entry.winrm.password = o_machine.winrm.password.clone();
        }
        if o_machine.winrm.host != "127.0.0.1" {
            entry.winrm.host = o_machine.winrm.host.clone();
        }
        if o_machine.winrm.port != 5985 {
            entry.winrm.port = o_machine.winrm.port;
        }
        if o_machine.winrm.guest_port.is_some() {
            entry.winrm.guest_port = o_machine.winrm.guest_port;
        }
        if o_machine.winrm.ssl {
            entry.winrm.ssl = true;
        }
        if o_machine.winrm.transport.is_some() {
            entry.winrm.transport = o_machine.winrm.transport.clone();
        }
        if o_machine.winrm.basic_auth_only {
            entry.winrm.basic_auth_only = true;
        }
        if !o_machine.winrm.ssl_peer_verification {
            entry.winrm.ssl_peer_verification = false;
        }
        if o_machine.winrm.timeout.is_some() {
            entry.winrm.timeout = o_machine.winrm.timeout;
        }
        if o_machine.winrm.retry_limit.is_some() {
            entry.winrm.retry_limit = o_machine.winrm.retry_limit;
        }
        if o_machine.winrm.retry_delay.is_some() {
            entry.winrm.retry_delay = o_machine.winrm.retry_delay;
        }
        if o_machine.winrm.execution_time_limit.is_some() {
            entry.winrm.execution_time_limit = o_machine.winrm.execution_time_limit.clone();
        }

        // Vagrant: merge fields
        if o_machine.vagrant.host.is_some() {
            entry.vagrant.host = o_machine.vagrant.host.clone();
        }
        for plugin in &o_machine.vagrant.plugins {
            if !entry.vagrant.plugins.contains(plugin) {
                entry.vagrant.plugins.push(plugin.clone());
            }
        }
        for sensitive in &o_machine.vagrant.sensitive {
            if !entry.vagrant.sensitive.contains(sensitive) {
                entry.vagrant.sensitive.push(sensitive.clone());
            }
        }

        // Triggers: append
        entry.triggers.extend(o_machine.triggers.clone());

        // Pushes: append
        entry.pushes.extend(o_machine.pushes.clone());
    }

    merged.pushes.extend(override_conf.pushes.clone());

    merged
}

/// Evaluates the complete Vagrantfile hierarchy for a project and optional box directory.
///
/// # Arguments
///
/// * `project_dir` - The root directory of the project.
/// * `box_dir` - Optional path to the box directory.
///
/// # Returns
///
/// Returns the resulting `EnvironmentConfig`.
///
/// # Errors
///
/// Returns a `MigratoryError` if evaluation fails or if no Vagrantfile is found.
pub fn evaluate_vagrantfile_hierarchy(
    project_dir: &std::path::Path,
    box_dir: Option<&std::path::Path>,
) -> Result<EnvironmentConfig, crate::error::MigratoryError> {
    evaluate_vagrantfile_hierarchy_with_home(project_dir, box_dir, &get_vagrant_home())
}

/// Evaluates the complete Vagrantfile hierarchy with an explicit Vagrant home directory.
pub fn evaluate_vagrantfile_hierarchy_with_home(
    project_dir: &std::path::Path,
    box_dir: Option<&std::path::Path>,
    vagrant_home: &std::path::Path,
) -> Result<EnvironmentConfig, crate::error::MigratoryError> {
    let files = resolve_vagrantfile_hierarchy_with_home(project_dir, box_dir, vagrant_home);
    if files.is_empty() {
        return Err(crate::error::MigratoryError::NotFound(
            "Vagrantfile".to_string(),
        ));
    }

    let file_refs: Vec<&std::path::Path> = files.iter().map(|p| p.as_path()).collect();
    parser::parse_vagrantfiles(&file_refs)
}

/// Returns the effective project working directory, respecting `VAGRANT_CWD`.
pub fn get_vagrant_cwd() -> std::path::PathBuf {
    get_vagrant_cwd_from(std::env::var("VAGRANT_CWD").ok().as_deref())
}

/// Resolves the effective project working directory from an optional `VAGRANT_CWD` string.
pub fn get_vagrant_cwd_from(vagrant_cwd: Option<&str>) -> std::path::PathBuf {
    if let Some(cwd) = vagrant_cwd
        && !cwd.trim().is_empty()
    {
        return std::path::PathBuf::from(cwd);
    }
    std::env::current_dir().unwrap_or(std::path::PathBuf::from("."))
}

/// Returns the effective Vagrantfile filename, respecting `VAGRANT_VAGRANTFILE`.
pub fn get_vagrantfile_name() -> String {
    get_vagrantfile_name_from(std::env::var("VAGRANT_VAGRANTFILE").ok().as_deref())
}

/// Resolves the effective Vagrantfile filename from an optional `VAGRANT_VAGRANTFILE` string.
pub fn get_vagrantfile_name_from(vagrant_vf: Option<&str>) -> String {
    if let Some(vf) = vagrant_vf
        && !vf.trim().is_empty()
    {
        return vf.to_string();
    }
    "Vagrantfile".to_string()
}

/// Returns the effective Vagrantfile path relative to the given directory.
pub fn get_vagrantfile_path(dir: &std::path::Path) -> std::path::PathBuf {
    get_vagrantfile_path_from(dir, std::env::var("VAGRANT_VAGRANTFILE").ok().as_deref())
}

/// Resolves the effective Vagrantfile path relative to `dir` given an optional `VAGRANT_VAGRANTFILE` string.
pub fn get_vagrantfile_path_from(
    dir: &std::path::Path,
    vagrant_vf: Option<&str>,
) -> std::path::PathBuf {
    let name = get_vagrantfile_name_from(vagrant_vf);
    dir.join(name)
}

/// Returns the dotfile path (usually `.vagrant`), respecting `VAGRANT_DOTFILE_PATH`.
pub fn get_dotfile_path(dir: &std::path::Path) -> std::path::PathBuf {
    get_dotfile_path_from(dir, std::env::var("VAGRANT_DOTFILE_PATH").ok().as_deref())
}

/// Resolves the dotfile path relative to `dir` given an optional `VAGRANT_DOTFILE_PATH` string.
pub fn get_dotfile_path_from(
    dir: &std::path::Path,
    dotfile_env: Option<&str>,
) -> std::path::PathBuf {
    if let Some(dotfile) = dotfile_env
        && !dotfile.trim().is_empty()
    {
        let path = std::path::PathBuf::from(dotfile);
        if path.is_absolute() {
            return path;
        }
        return dir.join(path);
    }
    dir.join(".vagrant")
}

/// Resolves target machine names from user input, supporting exact names, regex patterns, autostart filtering, and primary machine fallback.
///
/// # Arguments
///
/// * `machines` - The map of configured machines.
/// * `target_input` - The user target argument (e.g. `Some("web1")`, `Some("/web.*/")`, or `None`).
///
/// # Returns
///
/// Returns a `Vec<String>` of resolved machine names in deterministic sorted order.
///
/// # Errors
///
/// Returns a `MigratoryError::NotFound` if an explicit target name or regex pattern matches no configured machines, or `MigratoryError::Validation` if regex syntax is invalid.
pub fn resolve_target_machines(
    machines: &HashMap<String, MachineConfig>,
    target_input: Option<&str>,
) -> Result<Vec<String>, MigratoryError> {
    if let Some(target) = target_input {
        let trimmed = target.trim();
        if trimmed.starts_with('/') && trimmed.ends_with('/') && trimmed.len() >= 2 {
            let pattern = &trimmed[1..trimmed.len() - 1];
            let re = regex::Regex::new(pattern).map_err(|e| {
                MigratoryError::Validation(format!("Invalid regex pattern '{}': {}", pattern, e))
            })?;
            let matches: Vec<String> = machines
                .keys()
                .filter(|name| re.is_match(name))
                .cloned()
                .collect();
            if matches.is_empty() {
                return Err(MigratoryError::NotFound(format!(
                    "No machines matching regex '{}' found",
                    pattern
                )));
            }
            let mut sorted = matches;
            sorted.sort();
            return Ok(sorted);
        }

        if let Some(m) = machines.get(trimmed) {
            let _ = m;
            return Ok(vec![trimmed.to_string()]);
        }

        if trimmed.contains(',') {
            let mut resolved = Vec::new();
            for part in trimmed.split(',') {
                let part_trimmed = part.trim();
                if machines.contains_key(part_trimmed) {
                    resolved.push(part_trimmed.to_string());
                } else {
                    return Err(MigratoryError::NotFound(format!(
                        "Machine '{}' not found",
                        part_trimmed
                    )));
                }
            }
            resolved.sort();
            return Ok(resolved);
        }

        if trimmed == "default" && machines.is_empty() {
            return Ok(vec!["default".to_string()]);
        }

        return Err(MigratoryError::NotFound(format!(
            "Machine '{}' not found",
            trimmed
        )));
    }

    if machines.is_empty() {
        return Ok(vec!["default".to_string()]);
    }

    let autostart_machines: Vec<String> = machines
        .iter()
        .filter(|(_, config)| config.autostart)
        .map(|(name, _)| name.clone())
        .collect();

    let mut result = if !autostart_machines.is_empty() {
        autostart_machines
    } else {
        let primary_machines: Vec<String> = machines
            .iter()
            .filter(|(_, config)| config.primary)
            .map(|(name, _)| name.clone())
            .collect();
        if !primary_machines.is_empty() {
            primary_machines
        } else {
            machines.keys().cloned().collect()
        }
    };

    result.sort();
    Ok(result)
}

/// Sorts machines topologically based on their configured dependencies (`depends_on`).
///
/// If machine A depends on machine B, machine B will precede machine A in the returned list.
///
/// # Arguments
///
/// * `targets` - The machine names to sort.
/// * `machines` - The map of defined machine configurations.
/// * `reverse` - If true, returns reverse dependency order (useful for graceful shutdown/destruction).
///
/// # Returns
///
/// Returns a `Vec<String>` of machine names in dependency order.
///
/// # Errors
///
/// Returns `MigratoryError::NotFound` if a dependency does not exist in `machines`, or
/// `MigratoryError::Validation` if a cyclic dependency is detected.
pub fn sort_machines_by_dependencies(
    targets: &[String],
    machines: &HashMap<String, MachineConfig>,
    reverse: bool,
) -> Result<Vec<String>, MigratoryError> {
    for target in targets {
        if let Some(config) = machines.get(target) {
            for dep in &config.depends_on {
                if !machines.contains_key(dep) {
                    return Err(MigratoryError::NotFound(format!(
                        "Machine '{}' depends on '{}', which does not exist",
                        target, dep
                    )));
                }
            }
        }
    }

    let target_set: std::collections::HashSet<&String> = targets.iter().collect();
    let mut in_degree: HashMap<String, usize> = HashMap::new();
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();

    for t in targets {
        in_degree.entry(t.clone()).or_insert(0);
        adj.entry(t.clone()).or_default();
    }

    for t in targets {
        if let Some(config) = machines.get(t) {
            for dep in &config.depends_on {
                if target_set.contains(dep) && dep != t {
                    adj.entry(dep.clone()).or_default().push(t.clone());
                    *in_degree.entry(t.clone()).or_insert(0) += 1;
                }
            }
        }
    }

    let mut q_vec: Vec<String> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(node, _)| node.clone())
        .collect();
    q_vec.sort();
    let mut queue: std::collections::VecDeque<String> = q_vec.into();

    let mut result = Vec::new();
    let empty_neighbors = Vec::new();
    while let Some(u) = queue.pop_front() {
        result.push(u.clone());
        let neighbors = adj.get(&u).unwrap_or(&empty_neighbors);
        let mut sorted_neighbors = neighbors.clone();
        sorted_neighbors.sort();
        for v in sorted_neighbors {
            let deg = in_degree.entry(v.clone()).or_default();
            *deg = deg.saturating_sub(1);
            if *deg == 0 {
                queue.push_back(v);
            }
        }
    }

    if result.len() != targets.len() {
        return Err(MigratoryError::Validation(
            "Cyclic dependency detected among target machines".to_string(),
        ));
    }

    if reverse {
        result.reverse();
    }

    Ok(result)
}

/// Executes triggers matching the given stage and action name, supporting both host and guest execution.
///
/// # Arguments
///
/// * `stage` - The lifecycle stage, e.g. "before" or "after".
/// * `action_name` - The action name, e.g. "up", "halt", "destroy", "provision".
/// * `triggers` - Slice of configured triggers to evaluate.
/// * `comm` - Optional communicator for running remote triggers on the guest.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if a non-ignored trigger fails.
fn execute_triggers_with_guest(
    stage: &str,
    action_name: &str,
    triggers: &[TriggerConfig],
    comm: Option<&dyn crate::communicator::Communicator>,
) -> Result<(), crate::error::MigratoryError> {
    for trigger in triggers {
        if trigger.stage.eq_ignore_ascii_case(stage)
            && (trigger.actions.is_empty()
                || trigger
                    .actions
                    .iter()
                    .any(|a| a.eq_ignore_ascii_case(action_name)))
        {
            if let Some(msg) = &trigger.info {
                println!("==> trigger: {}", msg);
            }
            if let Some(msg) = &trigger.warn {
                eprintln!("==> trigger warning: {}", msg);
            }

            if let Some(cmd_str) = &trigger.run_inline {
                let mut cmd = std::process::Command::new("sh");
                cmd.arg("-c").arg(cmd_str);
                cmd.env("VAGRANT_TRIGGER_ACTION", action_name);
                for (k, v) in &trigger.env {
                    cmd.env(k, v);
                }
                let status = cmd.status()?;
                if !status.success() && !trigger.ignore_errors {
                    return Err(crate::error::MigratoryError::Generic(format!(
                        "Trigger failed with exit status: {}",
                        status
                    )));
                }
            } else if let Some(path_str) = &trigger.run_path {
                let mut cmd = std::process::Command::new(path_str);
                cmd.env("VAGRANT_TRIGGER_ACTION", action_name);
                for (k, v) in &trigger.env {
                    cmd.env(k, v);
                }
                let status = cmd.status()?;
                if !status.success() && !trigger.ignore_errors {
                    return Err(crate::error::MigratoryError::Generic(format!(
                        "Trigger script {} failed with exit status: {}",
                        path_str, status
                    )));
                }
            } else if let Some(remote_cmd) = &trigger.run_remote_inline
                && let Some(communicator) = comm
            {
                let res = communicator.execute(remote_cmd);
                if let Err(e) = res
                    && !trigger.ignore_errors
                {
                    return Err(crate::error::MigratoryError::Generic(format!(
                        "Remote trigger command failed: {}",
                        e
                    )));
                }
            } else if let Some(remote_path) = &trigger.run_remote_path
                && let Some(communicator) = comm
            {
                let filename = std::path::Path::new(remote_path)
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or("trigger_script.sh");
                let dest = format!("/tmp/{}", filename);
                let res = communicator
                    .upload(std::path::Path::new(remote_path), &dest)
                    .and_then(|_| communicator.execute(&format!("chmod +x {} && {}", dest, dest)));
                if let Err(e) = res
                    && !trigger.ignore_errors
                {
                    return Err(crate::error::MigratoryError::Generic(format!(
                        "Remote trigger script {} failed: {}",
                        remote_path, e
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Executes triggers matching the given stage and action name.
///
/// # Arguments
///
/// * `stage` - The lifecycle stage, e.g. "before" or "after".
/// * `action_name` - The action name, e.g. "up", "halt", "destroy", "provision".
/// * `triggers` - Slice of configured triggers to evaluate.
///
/// # Returns
///
/// Returns `Ok(())` on success.
///
/// # Errors
///
/// Returns a `MigratoryError` if a non-ignored trigger fails.
pub fn execute_triggers(
    stage: &str,
    action_name: &str,
    triggers: &[TriggerConfig],
) -> Result<(), crate::error::MigratoryError> {
    execute_triggers_with_guest(stage, action_name, triggers, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::communicator::Communicator;

    #[test]
    fn test_default_machine_config() {
        let config = MachineConfig::default();
        assert_eq!(config.ssh.username, "vagrant");
        assert_eq!(config.winrm.port, 5985);
        assert!(config.vm.box_name.is_none());
        assert!(!config.primary);
        assert!(config.autostart);
        assert!(config.triggers.is_empty());
    }

    #[test]
    fn test_evaluate_vagrantfile_mock_nonempty() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("Vagrantfile");
        std::fs::write(
            &path,
            "Vagrant.configure('2') do |c|\n  c.vm.define 'node1'\n  c.vm.define 'node2'\nend",
        )?;
        let config = evaluate_vagrantfile(&path.to_string_lossy())?;
        assert_eq!(config.machines.len(), 2);
        Ok(())
    }

    #[test]
    fn test_evaluate_vagrantfile() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("Vagrantfile");
        std::fs::write(
            &path,
            "Vagrant_Result = Vagrant.configure('2') do |c|\n c.vm.box = 'foo' \nend",
        )?;

        let env = evaluate_vagrantfile(&path.to_string_lossy())?;
        assert!(env.machines.contains_key("default"));
        assert_eq!(env.machines["default"].vm.box_name, Some("foo".to_string()));
        Ok(())
    }

    #[test]
    fn test_network_config() {
        let port = NetworkConfig::ForwardedPort {
            guest: 80,
            host: 8080,
            auto_correct: true,
            protocol: None,
            host_ip: None,
        };
        let priv_net = NetworkConfig::PrivateNetwork {
            ip: Some("192.168.50.4".to_string()),
            netmask: Some("255.255.255.0".to_string()),
            dhcp: false,
            virtualbox_intnet: None,
        };
        let pub_net = NetworkConfig::PublicNetwork {
            ip: None,
            bridge: Some("eth0".to_string()),
            use_dhcp_assigned_default_route: true,
        };

        assert!(matches!(
            port,
            NetworkConfig::ForwardedPort {
                guest: 80,
                host: 8080,
                auto_correct: true,
                protocol: None,
                host_ip: None
            }
        ));

        assert!(matches!(
            priv_net,
            NetworkConfig::PrivateNetwork { ip: Some(ip), netmask: Some(nm), dhcp: false, virtualbox_intnet: None } if ip == "192.168.50.4" && nm == "255.255.255.0"
        ));

        assert!(matches!(
            pub_net,
            NetworkConfig::PublicNetwork { ip: None, bridge: Some(b), use_dhcp_assigned_default_route: true } if b == "eth0"
        ));
    }

    #[test]
    fn test_provider_provisioner_config() {
        let provider = ProviderConfig {
            name: ProviderKind::VirtualBox,
            options: HashMap::new(),
        };
        assert_eq!(provider.name, "virtualbox");

        let provisioner = ProvisionerConfig {
            name: "shell".to_string(),
            config: HashMap::new(),
            id: Some("bootstrap".to_string()),
            run: Some("always".to_string()),
        };
        assert_eq!(provisioner.name, "shell");
        assert_eq!(provisioner.id.as_deref(), Some("bootstrap"));
        assert_eq!(provisioner.run.as_deref(), Some("always"));

        let vagrant = VagrantConfig::default();
        assert!(vagrant.host.is_none());
    }

    #[test]
    fn test_environment_path_helpers() -> Result<(), MigratoryError> {
        let dir = tempfile::tempdir()?;
        let path = dir.path();

        let _ = get_vagrant_cwd();
        let _ = get_vagrantfile_name();
        let _ = get_vagrantfile_path(path);
        let _ = get_dotfile_path(path);

        assert_eq!(
            get_vagrant_cwd_from(None),
            std::env::current_dir().unwrap_or(std::path::PathBuf::from("."))
        );
        assert_eq!(get_vagrantfile_name_from(None), "Vagrantfile");
        assert_eq!(
            get_vagrantfile_path_from(path, None),
            path.join("Vagrantfile")
        );
        assert_eq!(get_dotfile_path_from(path, None), path.join(".vagrant"));

        let path_str = path.to_str();
        assert_eq!(get_vagrant_cwd_from(path_str), path);
        assert_eq!(
            get_vagrantfile_name_from(Some("CustomVagrantfile")),
            "CustomVagrantfile"
        );
        assert_eq!(
            get_vagrantfile_path_from(path, Some("CustomVagrantfile")),
            path.join("CustomVagrantfile")
        );
        assert_eq!(
            get_dotfile_path_from(path, Some(".custom_vagrant")),
            path.join(".custom_vagrant")
        );

        let abs_dotfile = path.join(".abs_vagrant");
        assert_eq!(
            get_dotfile_path_from(path, abs_dotfile.to_str()),
            abs_dotfile
        );

        // Test empty string env vars to cover !is_empty() false branches
        assert_eq!(
            get_vagrant_cwd_from(Some("   ")),
            std::env::current_dir().unwrap_or(std::path::PathBuf::from("."))
        );
        assert_eq!(get_vagrantfile_name_from(Some("   ")), "Vagrantfile");
        assert_eq!(
            get_dotfile_path_from(path, Some("   ")),
            path.join(".vagrant")
        );
        Ok(())
    }

    #[test]
    fn test_execute_triggers() {
        let mut env_map = HashMap::new();
        env_map.insert("TEST_VAR".to_string(), "migratory_test".to_string());

        let triggers = vec![
            TriggerConfig {
                stage: "before".to_string(),
                actions: vec!["up".to_string()],
                run_inline: Some("true".to_string()),
                run_path: None,
                run_remote_inline: None,
                run_remote_path: None,
                ignore_errors: false,
                on_error: None,
                env: env_map,
                ..Default::default()
            },
            TriggerConfig {
                stage: "after".to_string(),
                actions: vec!["halt".to_string()],
                run_inline: Some("false".to_string()),
                run_path: None,
                run_remote_inline: None,
                run_remote_path: None,
                ignore_errors: true,
                on_error: None,
                env: HashMap::new(),
                ..Default::default()
            },
            TriggerConfig {
                stage: "before".to_string(),
                actions: vec![],
                run_inline: None,
                run_path: Some("true".to_string()),
                run_remote_inline: None,
                run_remote_path: None,
                ignore_errors: false,
                on_error: None,
                env: HashMap::new(),
                ..Default::default()
            },
            TriggerConfig {
                stage: "after".to_string(),
                actions: vec!["destroy".to_string()],
                run_inline: None,
                run_path: Some("false".to_string()),
                run_remote_inline: None,
                run_remote_path: None,
                ignore_errors: true,
                on_error: None,
                env: HashMap::new(),
                ..Default::default()
            },
        ];

        assert!(execute_triggers("before", "up", &triggers).is_ok());
        assert!(execute_triggers("after", "halt", &triggers).is_ok());
        assert!(execute_triggers("before", "any", &triggers).is_ok());
        assert!(execute_triggers("after", "destroy", &triggers).is_ok());
        assert!(execute_triggers("after", "other", &triggers).is_ok());

        let failing_inline = vec![TriggerConfig {
            stage: "before".to_string(),
            actions: vec!["up".to_string()],
            run_inline: Some("false".to_string()),
            run_path: None,
            run_remote_inline: None,
            run_remote_path: None,
            ignore_errors: false,
            on_error: None,
            env: HashMap::new(),
            ..Default::default()
        }];
        assert!(execute_triggers("before", "up", &failing_inline).is_err());

        let failing_path = vec![TriggerConfig {
            stage: "before".to_string(),
            actions: vec!["up".to_string()],
            run_inline: None,
            run_path: Some("false".to_string()),
            run_remote_inline: None,
            run_remote_path: None,
            ignore_errors: false,
            on_error: None,
            env: HashMap::new(),
            ..Default::default()
        }];
        assert!(execute_triggers("before", "up", &failing_path).is_err());

        let invalid_cmd = vec![TriggerConfig {
            stage: "before".to_string(),
            actions: vec!["up".to_string()],
            run_inline: None,
            run_path: Some("/path/to/nonexistent/executable/xyz_123".to_string()),
            run_remote_inline: None,
            run_remote_path: None,
            ignore_errors: false,
            on_error: None,
            env: HashMap::new(),
            ..Default::default()
        }];
        assert!(execute_triggers("before", "up", &invalid_cmd).is_err());

        let default_trigger = TriggerConfig::default();
        assert_eq!(default_trigger.stage, "");
        assert!(default_trigger.actions.is_empty());
    }

    /// Mock communicator for remote trigger execution tests.
    struct MockTriggerComm {
        /// Whether operations should return an error.
        should_fail: bool,
    }

    impl crate::communicator::Communicator for MockTriggerComm {
        fn execute(&self, command: &str) -> Result<String, crate::error::MigratoryError> {
            if self.should_fail {
                Err(crate::error::MigratoryError::Generic(
                    "remote fail".to_string(),
                ))
            } else {
                Ok(format!("executed: {}", command))
            }
        }
        fn upload(
            &self,
            _source: &std::path::Path,
            _destination: &str,
        ) -> Result<(), crate::error::MigratoryError> {
            if self.should_fail {
                Err(crate::error::MigratoryError::Generic(
                    "upload fail".to_string(),
                ))
            } else {
                Ok(())
            }
        }
        fn download(
            &self,
            _source: &str,
            _destination: &std::path::Path,
        ) -> Result<(), crate::error::MigratoryError> {
            Ok(())
        }
        fn execute_interactive(&self) -> Result<(), crate::error::MigratoryError> {
            Ok(())
        }
        fn wait_for_ready(
            &self,
            _timeout: std::time::Duration,
        ) -> Result<(), crate::error::MigratoryError> {
            Ok(())
        }
    }

    #[test]
    fn test_execute_triggers_remote() {
        let comm_success = MockTriggerComm { should_fail: false };
        let comm_fail = MockTriggerComm { should_fail: true };

        let triggers = vec![
            TriggerConfig {
                stage: "before".to_string(),
                actions: vec!["up".to_string()],
                run_inline: None,
                run_path: None,
                run_remote_inline: Some("echo hello".to_string()),
                run_remote_path: None,
                ignore_errors: false,
                on_error: None,
                env: HashMap::new(),
                ..Default::default()
            },
            TriggerConfig {
                stage: "before".to_string(),
                actions: vec!["up".to_string()],
                run_inline: None,
                run_path: None,
                run_remote_inline: None,
                run_remote_path: Some("script.sh".to_string()),
                ignore_errors: false,
                on_error: None,
                env: HashMap::new(),
                ..Default::default()
            },
        ];

        let res = execute_triggers_with_guest("before", "up", &triggers, Some(&comm_success));
        assert!(res.is_ok());

        let res_fail = execute_triggers_with_guest("before", "up", &triggers, Some(&comm_fail));
        assert!(res_fail.is_err());

        let ignore_triggers = vec![
            TriggerConfig {
                stage: "before".to_string(),
                actions: vec!["up".to_string()],
                run_inline: None,
                run_path: None,
                run_remote_inline: Some("echo fail".to_string()),
                run_remote_path: None,
                ignore_errors: true,
                on_error: None,
                env: HashMap::new(),
                ..Default::default()
            },
            TriggerConfig {
                stage: "before".to_string(),
                actions: vec!["up".to_string()],
                run_inline: None,
                run_path: None,
                run_remote_inline: None,
                run_remote_path: Some("fail.sh".to_string()),
                ignore_errors: true,
                on_error: None,
                env: HashMap::new(),
                ..Default::default()
            },
        ];
        assert!(
            execute_triggers_with_guest("before", "up", &ignore_triggers, Some(&comm_fail)).is_ok()
        );

        let _ = comm_success.download("src", std::path::Path::new("dst"));
        let _ = comm_success.execute_interactive();
        let _ = comm_success.wait_for_ready(std::time::Duration::from_secs(1));
    }

    #[test]
    fn test_vagrant_home_and_hierarchy() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let home_dir = temp.path().join("vhome");
        std::fs::create_dir_all(&home_dir)?;

        let _ = get_vagrant_home();
        assert_eq!(get_vagrant_home_from(home_dir.to_str(), None), home_dir);

        // Test with empty VAGRANT_HOME and set HOME
        assert_eq!(
            get_vagrant_home_from(Some("   "), home_dir.to_str()),
            home_dir.join(".vagrant.d")
        );

        // Test with whitespace HOME
        assert_eq!(
            get_vagrant_home_from(None, Some("   ")),
            std::path::PathBuf::from(".vagrant.d")
        );

        // Test with empty VAGRANT_HOME and empty HOME
        assert_eq!(
            get_vagrant_home_from(None, None),
            std::path::PathBuf::from(".vagrant.d")
        );

        let box_dir = temp.path().join("box");
        let box_include = box_dir.join("include");
        std::fs::create_dir_all(&box_include)?;
        let box_include_vf = box_include.join("Vagrantfile");
        std::fs::write(&box_include_vf, "Vagrant.configure('2') do |c|; end")?;
        let box_vf = box_dir.join("Vagrantfile");
        std::fs::write(&box_vf, "Vagrant.configure('2') do |c|; end")?;

        let global_vf = home_dir.join("Vagrantfile");
        std::fs::write(&global_vf, "Vagrant.configure('2') do |c|; end")?;

        let project_dir = temp.path().join("project");
        std::fs::create_dir_all(&project_dir)?;
        let project_vf = project_dir.join("Vagrantfile");
        std::fs::write(
            &project_vf,
            "Vagrant.configure('2') do |c|\n c.vm.box = 'testbox'\nend",
        )?;

        let _ = resolve_vagrantfile_hierarchy(&project_dir, Some(&box_dir));
        let hierarchy =
            resolve_vagrantfile_hierarchy_with_home(&project_dir, Some(&box_dir), &home_dir);
        assert_eq!(hierarchy.len(), 4);
        assert_eq!(hierarchy[0], box_include_vf);
        assert_eq!(hierarchy[1], box_vf);
        assert_eq!(hierarchy[2], global_vf);
        assert_eq!(hierarchy[3], project_vf);

        let empty_proj = temp.path().join("empty_proj");
        std::fs::create_dir_all(&empty_proj)?;
        let hierarchy_empty = resolve_vagrantfile_hierarchy_with_home(&empty_proj, None, &home_dir);
        assert_eq!(hierarchy_empty.len(), 1); // global_vf only

        let box_empty = temp.path().join("box_empty");
        std::fs::create_dir_all(&box_empty)?;
        let hierarchy_box_empty =
            resolve_vagrantfile_hierarchy_with_home(&empty_proj, Some(&box_empty), &home_dir);
        assert_eq!(hierarchy_box_empty.len(), 1); // global_vf only, no box files

        Ok(())
    }

    #[test]
    fn test_evaluate_vagrantfile_hierarchy() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let project_dir = temp.path().join("project");
        std::fs::create_dir_all(&project_dir)?;
        let project_vf = project_dir.join("Vagrantfile");
        std::fs::write(
            &project_vf,
            "Vagrant.configure('2') do |c|\n c.vm.box = 'mybox'\nend",
        )?;

        let env = evaluate_vagrantfile_hierarchy(&project_dir, None)?;
        assert_eq!(
            env.machines["default"].vm.box_name,
            Some("mybox".to_string())
        );

        let empty_dir = temp.path().join("empty");
        std::fs::create_dir_all(&empty_dir)?;
        let res_err = evaluate_vagrantfile_hierarchy_with_home(
            &empty_dir,
            None,
            &temp.path().join("nonexistent_home"),
        );
        assert!(res_err.is_err());
        Ok(())
    }

    #[test]
    fn test_resolve_target_machines_all_cases() -> Result<(), MigratoryError> {
        let mut machines = HashMap::new();

        let web1 = MachineConfig {
            autostart: true,
            ..Default::default()
        };
        machines.insert("web1".to_string(), web1);

        let web2 = MachineConfig {
            autostart: true,
            ..Default::default()
        };
        machines.insert("web2".to_string(), web2);

        let db1 = MachineConfig {
            autostart: false,
            ..Default::default()
        };
        machines.insert("db1".to_string(), db1);

        // 1. None target -> autostart machines ("web1", "web2")
        let auto = resolve_target_machines(&machines, None)?;
        assert_eq!(auto, vec!["web1".to_string(), "web2".to_string()]);

        // 2. Exact target match
        let exact = resolve_target_machines(&machines, Some("db1"))?;
        assert_eq!(exact, vec!["db1".to_string()]);

        // 3. Regex target match
        let regex_match = resolve_target_machines(&machines, Some("/web/"))?;
        assert_eq!(regex_match, vec!["web1".to_string(), "web2".to_string()]);

        // 4. Regex target match single
        let regex_single = resolve_target_machines(&machines, Some("/^db/"))?;
        assert_eq!(regex_single, vec!["db1".to_string()]);

        // 5. Regex target no match
        let regex_none = resolve_target_machines(&machines, Some("/frontend/"));
        assert!(matches!(regex_none, Err(MigratoryError::NotFound(_))));

        // 6. Regex syntax error
        let regex_invalid = resolve_target_machines(&machines, Some("/[a-/"));
        assert!(matches!(regex_invalid, Err(MigratoryError::Validation(_))));

        // 7. Comma-separated list
        let comma_match = resolve_target_machines(&machines, Some("web1,db1"))?;
        assert_eq!(comma_match, vec!["db1".to_string(), "web1".to_string()]);

        // 8. Comma-separated list with invalid name
        let comma_err = resolve_target_machines(&machines, Some("web1,unknown"));
        assert!(matches!(comma_err, Err(MigratoryError::NotFound(_))));

        // 9. Unknown single target
        let unknown = resolve_target_machines(&machines, Some("missing"));
        assert!(matches!(unknown, Err(MigratoryError::NotFound(_))));

        // 10. Fallback when autostart is false for all machines, but one is primary
        let mut no_auto = HashMap::new();
        let m1 = MachineConfig {
            autostart: false,
            primary: true,
            ..Default::default()
        };
        no_auto.insert("primary_vm".to_string(), m1);
        let m2 = MachineConfig {
            autostart: false,
            primary: false,
            ..Default::default()
        };
        no_auto.insert("secondary_vm".to_string(), m2);
        let prim = resolve_target_machines(&no_auto, None)?;
        assert_eq!(prim, vec!["primary_vm".to_string()]);

        // 11. Fallback when neither autostart nor primary is set
        let mut plain = HashMap::new();
        let p1 = MachineConfig {
            autostart: false,
            ..Default::default()
        };
        plain.insert("p1".to_string(), p1);
        let res_plain = resolve_target_machines(&plain, None)?;
        assert_eq!(res_plain, vec!["p1".to_string()]);

        // 12. Empty machines map with "default" target or None
        let empty_map: HashMap<String, MachineConfig> = HashMap::new();
        let res_empty = resolve_target_machines(&empty_map, None)?;
        assert_eq!(res_empty, vec!["default".to_string()]);
        let res_empty_def = resolve_target_machines(&empty_map, Some("default"))?;
        assert_eq!(res_empty_def, vec!["default".to_string()]);

        // 13. Single slash (starts and ends with / but len < 2), invalid regex, and default on non-empty map
        let slash_res = resolve_target_machines(&plain, Some("/"));
        assert!(slash_res.is_err());
        let starts_with_slash_only =
            resolve_target_machines(&plain, Some("/not_ending_with_slash"));
        assert!(starts_with_slash_only.is_err());
        let bad_regex = resolve_target_machines(&plain, Some("/[unclosed/"));
        assert!(bad_regex.is_err());
        let def_non_empty = resolve_target_machines(&plain, Some("default"));
        assert!(def_non_empty.is_err());
        Ok(())
    }

    #[test]
    fn test_sort_machines_by_dependencies() -> Result<(), MigratoryError> {
        let mut machines = HashMap::new();

        let db = MachineConfig {
            name: "db".to_string(),
            ..Default::default()
        };
        machines.insert("db".to_string(), db);

        let backend = MachineConfig {
            name: "backend".to_string(),
            depends_on: vec!["db".to_string()],
            ..Default::default()
        };
        machines.insert("backend".to_string(), backend);

        let frontend = MachineConfig {
            name: "frontend".to_string(),
            depends_on: vec!["backend".to_string()],
            ..Default::default()
        };
        machines.insert("frontend".to_string(), frontend);

        // Linear order
        let targets = vec![
            "frontend".to_string(),
            "backend".to_string(),
            "db".to_string(),
        ];
        let sorted = sort_machines_by_dependencies(&targets, &machines, false)?;
        assert_eq!(
            sorted,
            vec![
                "db".to_string(),
                "backend".to_string(),
                "frontend".to_string()
            ]
        );

        // Reverse order (for halt / destroy)
        let reversed = sort_machines_by_dependencies(&targets, &machines, true)?;
        assert_eq!(
            reversed,
            vec![
                "frontend".to_string(),
                "backend".to_string(),
                "db".to_string()
            ]
        );

        // Subset of targets
        let sub_targets = vec!["frontend".to_string(), "backend".to_string()];
        let sub_sorted = sort_machines_by_dependencies(&sub_targets, &machines, false)?;
        assert_eq!(
            sub_sorted,
            vec!["backend".to_string(), "frontend".to_string()]
        );

        // Missing dependency error
        let mut broken_machines = machines.clone();
        let orphan = MachineConfig {
            depends_on: vec!["missing_node".to_string()],
            ..Default::default()
        };
        broken_machines.insert("orphan".to_string(), orphan);
        let err_missing =
            sort_machines_by_dependencies(&["orphan".to_string()], &broken_machines, false);
        assert!(matches!(err_missing, Err(MigratoryError::NotFound(_))));

        // Cyclic dependency error
        let mut cyclic_machines = HashMap::new();
        let node_a = MachineConfig {
            depends_on: vec!["node_b".to_string()],
            ..Default::default()
        };
        cyclic_machines.insert("node_a".to_string(), node_a);
        let node_b = MachineConfig {
            depends_on: vec!["node_a".to_string()],
            ..Default::default()
        };
        cyclic_machines.insert("node_b".to_string(), node_b);

        let err_cycle = sort_machines_by_dependencies(
            &["node_a".to_string(), "node_b".to_string()],
            &cyclic_machines,
            false,
        );
        assert!(matches!(err_cycle, Err(MigratoryError::Validation(_))));

        // Unknown target not present in machines map
        let ghost_sorted = sort_machines_by_dependencies(&["ghost".to_string()], &machines, false)?;
        assert_eq!(ghost_sorted, vec!["ghost".to_string()]);

        // Self-dependent machine (dep == t)
        let mut self_dep_machines = HashMap::new();
        let self_node = MachineConfig {
            depends_on: vec!["self".to_string()],
            ..Default::default()
        };
        self_dep_machines.insert("self".to_string(), self_node);
        let self_sorted =
            sort_machines_by_dependencies(&["self".to_string()], &self_dep_machines, false)?;
        assert_eq!(self_sorted, vec!["self".to_string()]);

        // Diamond dependency
        let mut diamond = HashMap::new();
        let a = MachineConfig::default();
        diamond.insert("a".to_string(), a);
        let b = MachineConfig {
            depends_on: vec!["a".to_string()],
            ..Default::default()
        };
        diamond.insert("b".to_string(), b);
        let c = MachineConfig {
            depends_on: vec!["a".to_string()],
            ..Default::default()
        };
        diamond.insert("c".to_string(), c);
        let d = MachineConfig {
            depends_on: vec!["b".to_string(), "c".to_string()],
            ..Default::default()
        };
        diamond.insert("d".to_string(), d);

        let diamond_sorted = sort_machines_by_dependencies(
            &[
                "d".to_string(),
                "c".to_string(),
                "b".to_string(),
                "a".to_string(),
            ],
            &diamond,
            false,
        )?;
        assert_eq!(diamond_sorted[0], "a");
        assert_eq!(diamond_sorted[3], "d");
        Ok(())
    }

    #[test]
    fn test_resolve_vagrantfile_hierarchy_with_cli() -> Result<(), MigratoryError> {
        let temp = tempfile::tempdir()?;
        let project_dir = temp.path().join("project");
        std::fs::create_dir_all(&project_dir)?;

        let cli_vf = temp.path().join("CustomVagrantfile");
        std::fs::write(&cli_vf, "Vagrant.configure('2') do |c|; end")?;

        let files = resolve_vagrantfile_hierarchy_with_cli(&project_dir, None, Some(&cli_vf));
        assert!(files.contains(&cli_vf));

        let non_existent = temp.path().join("NoSuchFile");
        let files_missing =
            resolve_vagrantfile_hierarchy_with_cli(&project_dir, None, Some(&non_existent));
        assert!(!files_missing.contains(&non_existent));

        let files_none = resolve_vagrantfile_hierarchy_with_cli(&project_dir, None, None);
        assert!(!files_none.contains(&cli_vf));
        Ok(())
    }

    #[test]
    fn test_merge_environment_configs() -> Result<(), MigratoryError> {
        let mut base = EnvironmentConfig::default();
        let mut m_base = MachineConfig {
            name: "web".to_string(),
            ..Default::default()
        };
        m_base.vm.box_name = Some("base_box".to_string());
        m_base.vm.hostname = Some("base_host".to_string());
        m_base.vm.usable_port_range = (2200, 2250);
        m_base.vm.synced_folders.push(SyncedFolderConfig {
            host_path: std::path::PathBuf::from("./base"),
            guest_path: std::path::PathBuf::from("/vagrant"),
            folder_type: None,
            disabled: false,
            owner: None,
            group: None,
            mount_options: None,
            args: None,
        });
        m_base.vm.synced_folders.push(SyncedFolderConfig {
            host_path: std::path::PathBuf::from("./other"),
            guest_path: std::path::PathBuf::from("/other"),
            folder_type: None,
            disabled: false,
            owner: None,
            group: None,
            mount_options: None,
            args: None,
        });
        m_base.vm.provisioners.push(ProvisionerConfig {
            name: "shell".to_string(),
            config: HashMap::new(),
            id: Some("p1".to_string()),
            run: None,
        });
        let mut prov_opts = HashMap::new();
        prov_opts.insert("cpus".to_string(), "2".to_string());
        m_base.vm.providers.push(ProviderConfig {
            name: ProviderKind::VirtualBox,
            options: prov_opts,
        });
        m_base.ssh.forward_env.push("BASE_ENV".to_string());
        m_base.vagrant.sensitive.push("base_secret".to_string());
        base.machines.insert("web".to_string(), m_base);

        let mut override_conf = EnvironmentConfig::default();
        let mut m_over = MachineConfig {
            name: "web".to_string(),
            primary: true,
            autostart: false,
            depends_on: vec!["db".to_string()],
            ..Default::default()
        };
        m_over.vm.box_name = Some("over_box".to_string());
        m_over.vm.usable_port_range = (2300, 2400);
        m_over.vm.allowed_synced_folder_types = Some(vec!["rsync".to_string()]);
        m_over.vm.disks.push(DiskConfig {
            disk_type: "disk".to_string(),
            size: Some("30GB".to_string()),
            name: Some("data".to_string()),
            file: None,
            primary: false,
        });
        // Override synced folder on same guest_path "/vagrant"
        m_over.vm.synced_folders.push(SyncedFolderConfig {
            host_path: std::path::PathBuf::from("./over"),
            guest_path: std::path::PathBuf::from("/vagrant"),
            folder_type: Some(SyncedFolderType::Rsync),
            disabled: false,
            owner: None,
            group: None,
            mount_options: None,
            args: None,
        });
        // Additional synced folder with new guest_path "/extra"
        m_over.vm.synced_folders.push(SyncedFolderConfig {
            host_path: std::path::PathBuf::from("./extra"),
            guest_path: std::path::PathBuf::from("/extra"),
            folder_type: None,
            disabled: false,
            owner: None,
            group: None,
            mount_options: None,
            args: None,
        });
        m_over.vm.provisioners.push(ProvisionerConfig {
            name: "shell".to_string(),
            config: HashMap::new(),
            id: Some("p2".to_string()),
            run: None,
        });
        let mut prov_opts_over = HashMap::new();
        prov_opts_over.insert("memory".to_string(), "2048".to_string());
        m_over.vm.providers.push(ProviderConfig {
            name: ProviderKind::VirtualBox,
            options: prov_opts_over,
        });
        m_over.vm.providers.push(ProviderConfig {
            name: ProviderKind::Docker,
            options: HashMap::new(),
        });

        // VM config fields
        m_over.vm.box_version = Some("1.2.0".to_string());
        m_over.vm.box_url = Some("http://box.url".to_string());
        m_over.vm.box_check_update = Some(false);
        m_over.vm.box_download_checksum = Some("abcd".to_string());
        m_over.vm.box_download_checksum_type = Some("sha256".to_string());
        m_over.vm.box_download_client_cert = Some(std::path::PathBuf::from("cert.pem"));
        m_over.vm.box_download_ca_cert = Some(std::path::PathBuf::from("ca.pem"));
        m_over.vm.box_download_insecure = Some(true);
        m_over.vm.guest = Some(GuestKind::Linux);
        m_over.vm.post_up_message = Some("hello".to_string());
        m_over.vm.boot_timeout = Some(300);
        m_over.vm.communicator = Some(CommunicatorKind::Ssh);
        m_over.vm.graceful_halt_timeout = Some(60);
        m_over.vm.networks.push(NetworkConfig::ForwardedPort {
            guest: 80,
            host: 8080,
            auto_correct: false,
            protocol: None,
            host_ip: None,
        });

        // SSH fields
        m_over.ssh.username = "custom_user".to_string();
        m_over.ssh.password = Some("pass".to_string());
        m_over.ssh.host = "10.0.0.1".to_string();
        m_over.ssh.port = 2200;
        m_over.ssh.guest_port = Some(22);
        m_over.ssh.private_key_path = Some(std::path::PathBuf::from("/path/to/key"));
        m_over.ssh.insert_key = false;
        m_over.ssh.forward_agent = true;
        m_over.ssh.forward_x11 = true;
        m_over.ssh.proxy_command = Some("proxy".to_string());
        m_over.ssh.extra_args = Some(vec!["-v".to_string()]);
        m_over.ssh.pty = true;
        m_over.ssh.keep_alive = false;
        m_over.ssh.shell = Some("/bin/bash".to_string());
        m_over.ssh.export_command_template = Some("export %s".to_string());
        m_over.ssh.connect_timeout = Some(10);
        m_over.ssh.timeout = Some(20);
        m_over.ssh.verify_host_key = true;
        m_over.ssh.keys_only = false;
        m_over.ssh.forward_env.push("BASE_ENV".to_string()); // Duplicate to test branch
        m_over.ssh.forward_env.push("OVER_ENV".to_string());

        // WinRM fields
        m_over.winrm.username = "admin".to_string();
        m_over.winrm.password = Some("secret".to_string());
        m_over.winrm.host = "winrm.host".to_string();
        m_over.winrm.port = 5986;
        m_over.winrm.guest_port = Some(5985);
        m_over.winrm.ssl = true;
        m_over.winrm.transport = Some("negotiate".to_string());
        m_over.winrm.basic_auth_only = true;
        m_over.winrm.ssl_peer_verification = false;
        m_over.winrm.timeout = Some(30);
        m_over.winrm.retry_limit = Some(5);
        m_over.winrm.retry_delay = Some(2);
        m_over.winrm.execution_time_limit = Some("PT30M".to_string());

        // Vagrant fields
        m_over.vagrant.host = Some("myhost".to_string());
        m_over.vagrant.plugins.push("vagrant-disksize".to_string());
        m_over.vagrant.plugins.push("vagrant-disksize".to_string()); // Duplicate to test branch
        m_over.vagrant.sensitive.push("base_secret".to_string()); // Duplicate to test branch
        m_over.vagrant.sensitive.push("over_secret".to_string());
        override_conf.machines.insert("web".to_string(), m_over);

        // Add brand new machine to test inserting new machine in override
        let m_new = MachineConfig {
            name: "db".to_string(),
            ..Default::default()
        };
        override_conf.machines.insert("db".to_string(), m_new);

        let merged = merge_environment_configs(&base, &override_conf);
        assert!(merged.machines.contains_key("web"));
        assert!(merged.machines.contains_key("db"));
        let web = &merged.machines["web"];
        assert!(web.primary);
        assert!(!web.autostart);
        assert_eq!(web.depends_on, vec!["db".to_string()]);
        assert_eq!(web.vm.box_name.as_deref(), Some("over_box"));
        assert_eq!(web.vm.box_version.as_deref(), Some("1.2.0"));
        assert_eq!(web.vm.box_url.as_deref(), Some("http://box.url"));
        assert_eq!(web.vm.box_check_update, Some(false));
        assert_eq!(web.vm.box_download_checksum.as_deref(), Some("abcd"));
        assert_eq!(web.vm.box_download_checksum_type.as_deref(), Some("sha256"));
        assert_eq!(
            web.vm.box_download_client_cert.as_deref(),
            Some(std::path::Path::new("cert.pem"))
        );
        assert_eq!(
            web.vm.box_download_ca_cert.as_deref(),
            Some(std::path::Path::new("ca.pem"))
        );
        assert_eq!(web.vm.box_download_insecure, Some(true));
        assert_eq!(web.vm.guest.as_deref(), Some("linux"));
        assert_eq!(web.vm.post_up_message.as_deref(), Some("hello"));
        assert_eq!(web.vm.boot_timeout, Some(300));
        assert_eq!(web.vm.communicator.as_deref(), Some("ssh"));
        assert_eq!(web.vm.graceful_halt_timeout, Some(60));
        assert_eq!(web.vm.hostname.as_deref(), Some("base_host"));
        assert_eq!(web.vm.usable_port_range, (2300, 2400));
        assert_eq!(
            web.vm.allowed_synced_folder_types.as_deref(),
            Some(&["rsync".to_string()][..])
        );
        assert_eq!(web.vm.disks.len(), 1);
        assert_eq!(web.vm.networks.len(), 1);
        assert_eq!(web.vm.synced_folders.len(), 3);
        assert!(
            web.vm
                .synced_folders
                .iter()
                .any(|s| s.guest_path == std::path::Path::new("/extra"))
        );
        // /vagrant should be replaced by ./over
        let vagrant_sf = web
            .vm
            .synced_folders
            .iter()
            .find(|s| s.guest_path == std::path::Path::new("/vagrant"))
            .ok_or(MigratoryError::NotFound("/vagrant".to_string()))?;
        assert_eq!(vagrant_sf.host_path, std::path::Path::new("./over"));
        assert_eq!(vagrant_sf.folder_type.as_deref(), Some("rsync"));

        // Provisioners should be appended
        assert_eq!(web.vm.provisioners.len(), 2);
        assert_eq!(web.vm.provisioners[0].id.as_deref(), Some("p1"));
        assert_eq!(web.vm.provisioners[1].id.as_deref(), Some("p2"));

        // Provider options should be merged and new provider added
        let vb = web
            .vm
            .providers
            .iter()
            .find(|p| p.name == "virtualbox")
            .ok_or(MigratoryError::NotFound("virtualbox".to_string()))?;
        assert_eq!(vb.options.get("cpus").map(|s| s.as_str()), Some("2"));
        assert_eq!(vb.options.get("memory").map(|s| s.as_str()), Some("2048"));
        assert!(web.vm.providers.iter().any(|p| p.name == "docker"));

        // SSH fields
        assert_eq!(web.ssh.username, "custom_user");
        assert_eq!(web.ssh.password.as_deref(), Some("pass"));
        assert_eq!(web.ssh.host, "10.0.0.1");
        assert_eq!(web.ssh.port, 2200);
        assert_eq!(web.ssh.guest_port, Some(22));
        assert_eq!(
            web.ssh.private_key_path.as_deref(),
            Some(std::path::Path::new("/path/to/key"))
        );
        assert!(!web.ssh.insert_key);
        assert!(web.ssh.forward_agent);
        assert!(web.ssh.forward_x11);
        assert_eq!(web.ssh.proxy_command.as_deref(), Some("proxy"));
        assert_eq!(web.ssh.extra_args.as_deref(), Some(&["-v".to_string()][..]));
        assert!(web.ssh.pty);
        assert!(!web.ssh.keep_alive);
        assert_eq!(web.ssh.shell.as_deref(), Some("/bin/bash"));
        assert_eq!(
            web.ssh.export_command_template.as_deref(),
            Some("export %s")
        );
        assert_eq!(web.ssh.connect_timeout, Some(10));
        assert_eq!(web.ssh.timeout, Some(20));
        assert!(web.ssh.verify_host_key);
        assert!(!web.ssh.keys_only);
        assert_eq!(
            web.ssh.forward_env,
            vec!["BASE_ENV".to_string(), "OVER_ENV".to_string()]
        );

        // WinRM fields
        assert_eq!(web.winrm.username, "admin");
        assert_eq!(web.winrm.password.as_deref(), Some("secret"));
        assert_eq!(web.winrm.host, "winrm.host");
        assert_eq!(web.winrm.port, 5986);
        assert_eq!(web.winrm.guest_port, Some(5985));
        assert!(web.winrm.ssl);
        assert_eq!(web.winrm.transport.as_deref(), Some("negotiate"));
        assert!(web.winrm.basic_auth_only);
        assert!(!web.winrm.ssl_peer_verification);
        assert_eq!(web.winrm.timeout, Some(30));
        assert_eq!(web.winrm.retry_limit, Some(5));
        assert_eq!(web.winrm.retry_delay, Some(2));
        assert_eq!(web.winrm.execution_time_limit.as_deref(), Some("PT30M"));

        // Vagrant fields
        assert_eq!(web.vagrant.host.as_deref(), Some("myhost"));
        assert_eq!(web.vagrant.plugins, vec!["vagrant-disksize".to_string()]);
        assert_eq!(
            web.vagrant.sensitive,
            vec!["base_secret".to_string(), "over_secret".to_string()]
        );

        // Merge with override machine having guest_port = None to test false branch of is_some()
        let mut base_for_none = EnvironmentConfig::default();
        base_for_none
            .machines
            .insert("m".to_string(), MachineConfig::default());
        let mut over_for_none = EnvironmentConfig::default();
        let mut m_over_none = MachineConfig::default();
        m_over_none.ssh.guest_port = None;
        m_over_none.winrm.guest_port = None;
        over_for_none.machines.insert("m".to_string(), m_over_none);
        let merged_none = merge_environment_configs(&base_for_none, &over_for_none);
        assert_eq!(merged_none.machines["m"].ssh.guest_port, Some(22));
        assert_eq!(merged_none.machines["m"].winrm.guest_port, Some(5985));
        Ok(())
    }

    #[test]
    fn test_strongly_typed_enums() -> Result<(), MigratoryError> {
        // Architecture
        assert_eq!(Architecture::from_str_lenient("amd64"), Architecture::Amd64);
        assert_eq!(
            Architecture::from_str_lenient("x86_64"),
            Architecture::Amd64
        );
        assert_eq!(Architecture::from_str_lenient("x64"), Architecture::Amd64);
        assert_eq!(Architecture::from_str_lenient("arm64"), Architecture::Arm64);
        assert_eq!(
            Architecture::from_str_lenient("aarch64"),
            Architecture::Arm64
        );
        assert_eq!(
            Architecture::from_str_lenient("riscv64"),
            Architecture::Unknown("riscv64".to_string())
        );
        assert_eq!(Architecture::Amd64.as_str(), "amd64");
        assert_eq!(Architecture::Arm64.as_str(), "arm64");
        assert_eq!(
            Architecture::Unknown("riscv64".to_string()).as_str(),
            "riscv64"
        );
        assert_eq!(format!("{}", Architecture::Amd64), "amd64");
        assert_eq!(format!("{}", Architecture::Arm64), "arm64");
        assert_eq!(
            format!("{}", Architecture::Unknown("mips".to_string())),
            "mips"
        );

        // GuestKind
        assert_eq!(GuestKind::from_str_lenient("linux"), Some(GuestKind::Linux));
        assert_eq!(
            GuestKind::from_str_lenient(":ubuntu"),
            Some(GuestKind::Linux)
        );
        assert_eq!(
            GuestKind::from_str_lenient("darwin"),
            Some(GuestKind::Darwin)
        );
        assert_eq!(
            GuestKind::from_str_lenient(":macos"),
            Some(GuestKind::Darwin)
        );
        assert_eq!(
            GuestKind::from_str_lenient("solaris"),
            Some(GuestKind::Solaris)
        );
        assert_eq!(
            GuestKind::from_str_lenient(":omnios"),
            Some(GuestKind::Solaris)
        );
        assert_eq!(
            GuestKind::from_str_lenient("windows"),
            Some(GuestKind::Windows)
        );
        assert_eq!(
            GuestKind::from_str_lenient(":freebsd"),
            Some(GuestKind::Bsd)
        );
        assert_eq!(GuestKind::from_str_lenient("unknown_os"), None);
        assert_eq!(GuestKind::Linux.as_str(), "linux");
        assert_eq!(GuestKind::Darwin.as_str(), "darwin");
        assert_eq!(GuestKind::Solaris.as_str(), "solaris");
        assert_eq!(GuestKind::Windows.as_str(), "windows");
        assert_eq!(GuestKind::Bsd.as_str(), "bsd");
        assert_eq!(format!("{}", GuestKind::Linux), "linux");

        // CommunicatorKind
        assert_eq!(
            CommunicatorKind::from_str_lenient("ssh"),
            Some(CommunicatorKind::Ssh)
        );
        assert_eq!(
            CommunicatorKind::from_str_lenient(":winrm"),
            Some(CommunicatorKind::Winrm)
        );
        assert_eq!(
            CommunicatorKind::from_str_lenient("docker"),
            Some(CommunicatorKind::Docker)
        );
        assert_eq!(CommunicatorKind::from_str_lenient("telnet"), None);
        assert_eq!(CommunicatorKind::Ssh.as_str(), "ssh");
        assert_eq!(CommunicatorKind::Winrm.as_str(), "winrm");
        assert_eq!(CommunicatorKind::Docker.as_str(), "docker");
        assert_eq!(format!("{}", CommunicatorKind::Ssh), "ssh");

        // ProviderKind
        assert_eq!(
            ProviderKind::from_str_lenient("utm"),
            Some(ProviderKind::Utm)
        );
        assert_eq!(
            ProviderKind::from_str_lenient(":qemu"),
            Some(ProviderKind::Qemu)
        );
        assert_eq!(
            ProviderKind::from_str_lenient("virtualbox"),
            Some(ProviderKind::VirtualBox)
        );
        assert_eq!(
            ProviderKind::from_str_lenient("vbox"),
            Some(ProviderKind::VirtualBox)
        );
        assert_eq!(
            ProviderKind::from_str_lenient(":vmware_desktop"),
            Some(ProviderKind::Vmware)
        );
        assert_eq!(
            ProviderKind::from_str_lenient("parallels"),
            Some(ProviderKind::Parallels)
        );
        assert_eq!(
            ProviderKind::from_str_lenient(":libvirt"),
            Some(ProviderKind::Libvirt)
        );
        assert_eq!(
            ProviderKind::from_str_lenient("hyperv"),
            Some(ProviderKind::Hyperv)
        );
        assert_eq!(
            ProviderKind::from_str_lenient(":docker"),
            Some(ProviderKind::Docker)
        );
        assert_eq!(ProviderKind::from_str_lenient("bhyve"), None);
        assert_eq!(ProviderKind::Utm.as_str(), "utm");
        assert_eq!(ProviderKind::Qemu.as_str(), "qemu");
        assert_eq!(ProviderKind::VirtualBox.as_str(), "virtualbox");
        assert_eq!(ProviderKind::Vmware.as_str(), "vmware_desktop");
        assert_eq!(ProviderKind::Parallels.as_str(), "parallels");
        assert_eq!(ProviderKind::Libvirt.as_str(), "libvirt");
        assert_eq!(ProviderKind::Hyperv.as_str(), "hyperv");
        assert_eq!(ProviderKind::Docker.as_str(), "docker");
        assert_eq!(format!("{}", ProviderKind::Utm), "utm");

        // SyncedFolderType
        assert_eq!(
            SyncedFolderType::from_str_lenient("disabled"),
            Some(SyncedFolderType::Disabled)
        );
        assert_eq!(
            SyncedFolderType::from_str_lenient(":rsync"),
            Some(SyncedFolderType::Rsync)
        );
        assert_eq!(
            SyncedFolderType::from_str_lenient("virtiofs"),
            Some(SyncedFolderType::Virtiofs)
        );
        assert_eq!(
            SyncedFolderType::from_str_lenient("virtfs"),
            Some(SyncedFolderType::Virtiofs)
        );
        assert_eq!(
            SyncedFolderType::from_str_lenient(":smb"),
            Some(SyncedFolderType::Smb)
        );
        assert_eq!(
            SyncedFolderType::from_str_lenient("virtualbox"),
            Some(SyncedFolderType::VirtualBox)
        );
        assert_eq!(
            SyncedFolderType::from_str_lenient(":nfs"),
            Some(SyncedFolderType::Nfs)
        );
        assert_eq!(SyncedFolderType::from_str_lenient("sshfs"), None);
        assert_eq!(SyncedFolderType::Disabled.as_str(), "disabled");
        assert_eq!(SyncedFolderType::Rsync.as_str(), "rsync");
        assert_eq!(SyncedFolderType::Virtiofs.as_str(), "virtiofs");
        assert_eq!(SyncedFolderType::Smb.as_str(), "smb");
        assert_eq!(SyncedFolderType::VirtualBox.as_str(), "virtualbox");
        assert_eq!(SyncedFolderType::Nfs.as_str(), "nfs");
        assert_eq!(format!("{}", SyncedFolderType::Rsync), "rsync");

        // Serde roundtrip checks
        let json_arch = serde_json::to_string(&Architecture::Arm64)?;
        let de_arch: Architecture = serde_json::from_str(&json_arch)?;
        assert_eq!(de_arch, Architecture::Arm64);

        let json_prov = serde_json::to_string(&ProviderKind::Utm)?;
        let de_prov: ProviderKind = serde_json::from_str(&json_prov)?;
        assert_eq!(de_prov, ProviderKind::Utm);

        // Additional coverage for GuestKind, CommunicatorKind, ProviderKind, SyncedFolderType
        assert_eq!(GuestKind::parse_raw("linux"), GuestKind::Linux);
        assert_eq!(
            GuestKind::parse_raw(":custom_guest"),
            GuestKind::Custom("custom_guest".to_string())
        );
        assert_eq!(GuestKind::Custom("x".to_string()).as_str(), "x");
        assert!(GuestKind::Linux == "linux");
        let gk_deref: &str = &GuestKind::Darwin;
        assert_eq!(gk_deref, "darwin");

        assert_eq!(
            CommunicatorKind::parse_raw(":custom_comm"),
            CommunicatorKind::Custom("custom_comm".to_string())
        );
        assert_eq!(CommunicatorKind::Custom("c".to_string()).as_str(), "c");
        assert!(CommunicatorKind::Ssh == "ssh");
        let ck_deref: &str = &CommunicatorKind::Winrm;
        assert_eq!(ck_deref, "winrm");

        assert_eq!(
            ProviderKind::parse_raw(":custom_prov"),
            ProviderKind::Custom("custom_prov".to_string())
        );
        assert_eq!(ProviderKind::Custom("p".to_string()).as_str(), "p");
        assert!(ProviderKind::Utm == "utm");
        let utm_str = "utm".to_string();
        assert!(ProviderKind::Utm == utm_str);
        let pk_str: &str = ProviderKind::Qemu.as_ref();
        assert_eq!(pk_str, "qemu");
        let pk_path: &std::path::Path = ProviderKind::Qemu.as_ref();
        assert_eq!(pk_path, std::path::Path::new("qemu"));
        assert_eq!(ProviderKind::from("docker"), ProviderKind::Docker);
        assert_eq!(
            ProviderKind::from("docker".to_string()),
            ProviderKind::Docker
        );
        let pk_deref: &str = &ProviderKind::Libvirt;
        assert_eq!(pk_deref, "libvirt");

        assert_eq!(
            SyncedFolderType::from_str_lenient("9p"),
            Some(SyncedFolderType::Plan9)
        );
        assert_eq!(
            SyncedFolderType::parse_raw(":custom_sf"),
            SyncedFolderType::Custom("custom_sf".to_string())
        );
        assert_eq!(SyncedFolderType::Plan9.as_str(), "9p");
        assert_eq!(SyncedFolderType::Custom("sf".to_string()).as_str(), "sf");
        assert!(SyncedFolderType::Nfs == "nfs");
        let sf_deref: &str = &SyncedFolderType::Smb;
        assert_eq!(sf_deref, "smb");

        // Test get_vagrant_home_from, get_vagrant_cwd_from, get_vagrantfile_name_from, get_dotfile_path_from
        let _ = get_vagrant_home();
        assert_eq!(
            get_vagrant_home_from(Some("/custom/vh"), Some("/home/u")),
            std::path::PathBuf::from("/custom/vh")
        );
        assert_eq!(
            get_vagrant_home_from(None, Some("/home/u")),
            std::path::PathBuf::from("/home/u/.vagrant.d")
        );
        assert_eq!(
            get_vagrant_home_from(None, None),
            std::path::PathBuf::from(".vagrant.d")
        );
        assert_eq!(
            get_vagrant_cwd_from(Some("/custom/cwd")),
            std::path::PathBuf::from("/custom/cwd")
        );
        assert_eq!(get_vagrantfile_name_from(Some("CustomVf")), "CustomVf");
        assert_eq!(
            get_dotfile_path_from(std::path::Path::new("/proj"), Some("/abs/dot")),
            std::path::PathBuf::from("/abs/dot")
        );
        assert_eq!(
            get_dotfile_path_from(std::path::Path::new("/proj"), Some("rel_dot")),
            std::path::PathBuf::from("/proj/rel_dot")
        );

        let dir = tempfile::tempdir()?;
        assert!(evaluate_vagrantfile_hierarchy(dir.path(), None).is_err());

        // Test execute_triggers_with_comm with remote_inline and remote_path
        struct MockTriggerComm {
            fail: bool,
        }
        impl crate::communicator::Communicator for MockTriggerComm {
            fn execute(&self, _cmd: &str) -> Result<String, crate::error::MigratoryError> {
                if self.fail {
                    Err(crate::error::MigratoryError::Generic("fail".to_string()))
                } else {
                    Ok(String::new())
                }
            }
            fn upload(
                &self,
                _local: &std::path::Path,
                _remote: &str,
            ) -> Result<(), crate::error::MigratoryError> {
                if self.fail {
                    Err(crate::error::MigratoryError::Generic("fail".to_string()))
                } else {
                    Ok(())
                }
            }
        }

        let triggers = [
            TriggerConfig {
                stage: "before".to_string(),
                actions: vec!["up".to_string()],
                info: Some("info msg".to_string()),
                warn: Some("warn msg".to_string()),
                run_inline: Some("false".to_string()),
                ignore_errors: false,
                env: std::iter::once(("K".to_string(), "V".to_string())).collect(),
                ..Default::default()
            },
            TriggerConfig {
                stage: "before".to_string(),
                actions: vec!["up".to_string()],
                run_path: Some("false".to_string()),
                ignore_errors: false,
                env: std::iter::once(("K".to_string(), "V".to_string())).collect(),
                ..Default::default()
            },
            TriggerConfig {
                stage: "before".to_string(),
                actions: vec!["up".to_string()],
                run_remote_inline: Some("echo remote".to_string()),
                ignore_errors: false,
                ..Default::default()
            },
            TriggerConfig {
                stage: "before".to_string(),
                actions: vec!["up".to_string()],
                run_remote_path: Some("/tmp/script.sh".to_string()),
                ignore_errors: false,
                ..Default::default()
            },
        ];
        assert!(execute_triggers("before", "up", &triggers[..1]).is_err());
        assert!(execute_triggers("before", "up", &triggers[1..2]).is_err());
        let comm_ok = MockTriggerComm { fail: false };
        let comm_fail = MockTriggerComm { fail: true };
        assert!(
            execute_triggers_with_guest("before", "up", &triggers[2..], Some(&comm_ok)).is_ok()
        );
        assert!(
            execute_triggers_with_guest("before", "up", &triggers[2..3], Some(&comm_fail)).is_err()
        );
        assert!(
            execute_triggers_with_guest("before", "up", &triggers[3..4], Some(&comm_fail)).is_err()
        );
        Ok(())
    }
}
