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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_typed_options_traits() {
        let docker_prov = TypedProviderOptions::Docker {
            cmd: Some("/bin/sh".to_string()),
            build_dir: Some(".".to_string()),
            init: true,
            has_init: false,
            privileged: true,
        };
        let vbox_prov = TypedProviderOptions::VirtualBox {
            boot_mode: Some("gui".to_string()),
            gui: true,
            separate: false,
            vrde: true,
            vrdeport: Some("3389".to_string()),
            memory: Some("2048".to_string()),
            cpus: Some("2".to_string()),
        };
        let generic_prov = TypedProviderOptions::Generic(HashMap::new());

        assert_eq!(docker_prov.clone(), docker_prov);
        assert_eq!(vbox_prov.clone(), vbox_prov);
        assert_eq!(generic_prov.clone(), generic_prov);
        assert_ne!(docker_prov, vbox_prov);
        assert_ne!(vbox_prov, generic_prov);
        assert!(!format!("{docker_prov:?}{vbox_prov:?}{generic_prov:?}").is_empty());

        let docker_provisioner = TypedProvisionerConfig::Docker {
            build_image: Some("app".to_string()),
            build_path: Some(".".to_string()),
            run: Some("echo".to_string()),
            compose: None,
            images: vec!["alpine".to_string()],
            install: true,
        };
        let shell_provisioner = TypedProvisionerConfig::Shell {
            inline: Some("echo hi".to_string()),
            path: None,
            args: None,
            upload_path: None,
            powershell: false,
            powershell_args: None,
            sensitive: false,
            privileged: true,
            reboot: false,
            env: None,
        };
        let generic_provisioner = TypedProvisionerConfig::Generic(HashMap::new());

        assert_eq!(docker_provisioner.clone(), docker_provisioner);
        assert_eq!(shell_provisioner.clone(), shell_provisioner);
        assert_eq!(generic_provisioner.clone(), generic_provisioner);
        assert_ne!(docker_provisioner, shell_provisioner);
        assert_ne!(shell_provisioner, generic_provisioner);
        assert!(
            !format!("{docker_provisioner:?}{shell_provisioner:?}{generic_provisioner:?}")
                .is_empty()
        );
    }
}
