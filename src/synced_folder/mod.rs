//! Synced Folders implementation.

use crate::communicator::Communicator;
use crate::error::MigratoryError;

pub mod nfs;
pub mod rsync;
pub mod smb;
pub mod vbox;
pub mod virtiofs;

/// Common options for synced folders.
pub struct SyncedFolderOptions {
    /// Guest mount point.
    pub guest_path: String,
    /// Host source directory.
    pub host_path: String,
    /// Optional guest owner UID.
    pub owner: Option<String>,
    /// Optional guest group GID.
    pub group: Option<String>,
    /// Optional mount options (like dmask, fmask).
    pub mount_options: Vec<String>,
    /// Optional share name identifier.
    pub name: Option<String>,
    /// Transient flag (whether share is transient or persists across reboots).
    pub transient: bool,
    /// Directory mode permission bits (e.g. "0755").
    pub dmode: Option<String>,
    /// File mode permission bits (e.g. "0644").
    pub fmode: Option<String>,
}

impl Default for SyncedFolderOptions {
    fn default() -> Self {
        Self {
            guest_path: String::new(),
            host_path: String::new(),
            owner: Some("vagrant".to_string()),
            group: Some("vagrant".to_string()),
            mount_options: vec![],
            name: None,
            transient: true,
            dmode: None,
            fmode: None,
        }
    }
}

/// Interface for synced folders.
pub trait SyncedFolder {
    /// Prepares the host for syncing.
    fn prepare(&self, options: &SyncedFolderOptions) -> Result<(), MigratoryError>;

    /// Mounts the folder on the guest.
    fn mount(
        &self,
        options: &SyncedFolderOptions,
        comm: &dyn Communicator,
    ) -> Result<(), MigratoryError>;
}
