//! Strongly typed provider and provisioner configurations.

use std::collections::HashMap;

/// Strongly-typed provider options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypedProviderOptions {
    /// Docker provider options.
    Docker {
        /// Cmd to run.
        cmd: Option<String>,
        /// Directory to build.
        build_dir: Option<String>,
        /// Requires init.
        init: bool,
        /// Has init process.
        has_init: bool,
        /// Run privileged.
        privileged: bool,
    },
    /// VirtualBox provider options.
    VirtualBox {
        /// Boot mode.
        boot_mode: Option<String>,
        /// Gui mode.
        gui: bool,
        /// Separate mode.
        separate: bool,
        /// Vrde mode.
        vrde: bool,
        /// VRDE port.
        vrdeport: Option<String>,
        /// Memory size.
        memory: Option<String>,
        /// CPU count.
        cpus: Option<String>,
    },
    /// Generic or fallback provider options.
    Generic(HashMap<String, String>),
}

/// Strongly-typed provisioner options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypedProvisionerConfig {
    /// Docker provisioner options.
    Docker {
        /// Build image name.
        build_image: Option<String>,
        /// Build path.
        build_path: Option<String>,
        /// Run cmd.
        run: Option<String>,
        /// Compose file.
        compose: Option<String>,
        /// Images to pull.
        images: Vec<String>,
        /// Install docker.
        install: bool,
    },
    /// Shell provisioner options.
    Shell {
        /// Inline script.
        inline: Option<String>,
        /// Script path.
        path: Option<String>,
        /// Arguments.
        args: Option<String>,
        /// Upload path.
        upload_path: Option<String>,
        /// Use powershell.
        powershell: bool,
        /// Powershell arguments.
        powershell_args: Option<String>,
        /// Sensitive execution.
        sensitive: bool,
        /// Run privileged.
        privileged: bool,
        /// Reboot after.
        reboot: bool,
        /// Environment variables.
        env: Option<String>,
    },
    /// Generic or fallback provisioner options.
    Generic(HashMap<String, String>),
}
