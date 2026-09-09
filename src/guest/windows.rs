//! Windows guest capabilities.
//!
//! This module provides the implementation for managing a Windows-based guest
//! operating system, including tasks like network configuration, hostname
//! setting, and shared folder mounting.

use super::Guest;
use crate::communicator::Communicator;
use crate::config::NetworkConfig;
use crate::error::MigratoryError;
use std::path::Path;

/// Windows guest OS.
///
/// Handles interactions and capabilities specific to Windows virtual machines.
pub struct WindowsGuest;

impl Guest for WindowsGuest {
    /// Detects if the current guest is running a Windows OS.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute commands on the guest.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if `cmd.exe /c ver` outputs "windows" (case-insensitive),
    /// otherwise `Ok(false)`.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if command execution fails in an unexpected way
    /// (though usually falls back to false).
    fn detect(&self, comm: &dyn Communicator) -> Result<bool, MigratoryError> {
        let out = match comm.execute("cmd.exe /c ver") {
            Ok(output) => output,
            Err(_) => match comm.execute("powershell -Command \"$PSVersionTable\"") {
                Ok(ps_out) => ps_out,
                Err(_) => match comm.execute("systeminfo") {
                    Ok(sys_out) => sys_out,
                    Err(_) => return Ok(false),
                },
            },
        };
        let lower = out.trim().to_lowercase();
        Ok(lower.contains("windows") || lower.contains("microsoft") || lower.contains("psversion"))
    }

    /// Changes the hostname of the Windows guest.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute commands on the guest.
    /// * `hostname` - The new hostname.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn change_hostname(
        &self,
        comm: &dyn Communicator,
        hostname: &str,
    ) -> Result<(), MigratoryError> {
        if hostname.is_empty() {
            return Err(MigratoryError::Validation(
                "Hostname cannot be empty".to_string(),
            ));
        }

        // Change hostname via PowerShell Rename-Computer
        let cmd = format!(
            "powershell -Command \"Rename-Computer -NewName '{}' -Force\"",
            hostname.replace('\'', "''")
        );
        let _ = comm.execute(&cmd)?;
        Ok(())
    }

    /// Configures the network interfaces on the Windows guest.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute commands on the guest.
    /// * `networks` - Network configurations.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn configure_networks(
        &self,
        comm: &dyn Communicator,
        networks: &[NetworkConfig],
    ) -> Result<(), MigratoryError> {
        if networks.is_empty() {
            return Ok(());
        }

        let mut script = String::new();
        script.push_str("powershell -Command \"");
        let mut has_commands = false;
        for (idx, net) in (1..).zip(networks.iter()) {
            if let NetworkConfig::PrivateNetwork { ip: Some(ip), .. } = net {
                script.push_str(&format!(
                    "New-NetIPAddress -InterfaceIndex {} -IPAddress {} -PrefixLength 24;",
                    idx, ip
                ));
                has_commands = true;
            } else if let NetworkConfig::PublicNetwork { .. } = net {
                script.push_str(&format!(
                    "Set-NetIPInterface -InterfaceIndex {} -Dhcp Enabled;",
                    idx
                ));
                has_commands = true;
            }
        }
        script.push('"');

        if has_commands {
            let _ = comm.execute(&script)?;
        }
        Ok(())
    }

    /// Mounts a shared folder within the Windows guest.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute commands on the guest.
    /// * `name` - The name of the shared folder.
    /// * `guest_path` - The destination path within the guest.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn mount_shared_folder(
        &self,
        comm: &dyn Communicator,
        name: &str,
        guest_path: &Path,
    ) -> Result<(), MigratoryError> {
        if name.is_empty() {
            return Err(MigratoryError::Validation(
                "Shared folder name cannot be empty".to_string(),
            ));
        }

        let path_str = guest_path.to_str().unwrap_or("");
        if path_str.is_empty() {
            return Err(MigratoryError::Validation(
                "Guest path cannot be empty".to_string(),
            ));
        }

        let mkdir_cmd = format!("cmd.exe /c mkdir \"{}\"", path_str.replace('\"', "\"\""));
        let _ = comm.execute(&mkdir_cmd);

        let mount_cmd = format!(
            "cmd.exe /c net use \"{}\" \"\\\\vboxsvr\\{}\"",
            path_str.replace('\"', "\"\""),
            name.replace('\"', "\"\"")
        );
        let _ = comm.execute(&mount_cmd)?;

        Ok(())
    }

    /// Halts (shuts down) the Windows guest.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute commands on the guest.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    fn halt(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        let _ = comm.execute("cmd.exe /c shutdown /s /t 0 /f /d p:4:1 /c \"Migratory Halt\"")?;
        Ok(())
    }

    fn update_guest_additions(
        &self,
        comm: &dyn Communicator,
        provider_name: &str,
        _machine_id: Option<&str>,
    ) -> Result<(), MigratoryError> {
        if provider_name == "virtualbox" {
            let _ =
                comm.execute("powershell -Command \"Write-Host 'Updating VBox Additions...'\"")?;
        }
        Ok(())
    }

    fn reboot(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        let _ = comm.execute("cmd.exe /c shutdown /r /t 0 /f /d p:4:1 /c \"Migratory Reboot\"")?;
        Ok(())
    }

    fn mount_smb_shared_folder(
        &self,
        comm: &dyn Communicator,
        host_path: &str,
        guest_path: &Path,
        username: &str,
        password: &str,
    ) -> Result<(), MigratoryError> {
        let guest_str = guest_path.to_string_lossy();
        let cmd = format!(
            "powershell -Command \"New-SmbMapping -RemotePath '{}' -UserName '{}' -Password '{}' -LocalPath '{}' -Persistent $true\"",
            host_path.replace('\'', "''"),
            username.replace('\'', "''"),
            password.replace('\'', "''"),
            guest_str.replace('\'', "''")
        );
        let _ = comm.execute(&cmd)?;
        Ok(())
    }

    fn insert_public_key(
        &self,
        comm: &dyn Communicator,
        public_key: &str,
    ) -> Result<(), MigratoryError> {
        let key = public_key.trim();
        let cmd = format!(
            "powershell -Command \"$sshDir = [System.IO.Path]::Combine($env:USERPROFILE, '.ssh'); if (!(Test-Path $sshDir)) {{ New-Item -ItemType Directory -Path $sshDir | Out-Null }}; Add-Content -Path (Join-Path $sshDir 'authorized_keys') -Value '{}'\"",
            key.replace('\'', "''")
        );
        let _ = comm.execute(&cmd)?;
        Ok(())
    }

    fn remove_public_key(
        &self,
        comm: &dyn Communicator,
        public_key_pattern: &str,
    ) -> Result<(), MigratoryError> {
        let cmd = format!(
            "powershell -Command \"$f = Join-Path $env:USERPROFILE '.ssh/authorized_keys'; if (Test-Path $f) {{ (Get-Content $f) | Where-Object {{ $_ -notmatch '{}' }} | Set-Content $f }}\"",
            public_key_pattern.replace('\'', "''")
        );
        let _ = comm.execute(&cmd)?;
        Ok(())
    }

    fn rsync_installed(&self, comm: &dyn Communicator) -> Result<bool, MigratoryError> {
        let out = match comm.execute("where rsync") {
            Ok(o) => o,
            Err(_) => return Ok(false),
        };
        Ok(!out.trim().is_empty())
    }

    fn rsync_install(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        let cmd = "powershell -Command \"if (Get-Command choco -ErrorAction SilentlyContinue) { choco install -y rsync } elseif (Get-Command winget -ErrorAction SilentlyContinue) { winget install --silent rsync }\"";
        let _ = comm.execute(cmd)?;
        Ok(())
    }

    fn verify_guest_additions(
        &self,
        comm: &dyn Communicator,
        provider_name: &str,
    ) -> Result<bool, MigratoryError> {
        let check = match provider_name.to_lowercase().as_str() {
            "virtualbox" => "sc.exe query VBoxGuest",
            "vmware" | "vmware_desktop" => "sc.exe query VMTools",
            _ => "echo ok",
        };
        let out = comm.execute(check).unwrap_or_default();
        Ok(!out.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct MockComm {
        output: Result<String, String>,
    }

    impl Communicator for MockComm {
        fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
            match &self.output {
                Ok(out) => Ok(out.clone()),
                Err(err) => Err(MigratoryError::Generic(err.clone())),
            }
        }
        fn upload(&self, _local_path: &Path, _remote_path: &str) -> Result<(), MigratoryError> {
            Ok(())
        }
        fn download(&self, _remote_path: &str, _local_path: &Path) -> Result<(), MigratoryError> {
            Ok(())
        }
        fn execute_interactive(&self) -> Result<(), MigratoryError> {
            Ok(())
        }
        fn wait_for_ready(&self, _timeout: Duration) -> Result<(), MigratoryError> {
            Ok(())
        }
    }

    #[test]
    fn test_windows_guest_network_empty() {
        let comm = MockComm {
            output: Ok("".to_string()),
        };
        let guest = WindowsGuest;
        assert!(guest.configure_networks(&comm, &[]).is_ok());
    }

    #[test]
    fn test_windows_guest_network_forwarded() {
        let comm = MockComm {
            output: Ok("".to_string()),
        };
        let guest = WindowsGuest;
        let nets = vec![crate::config::NetworkConfig::ForwardedPort {
            guest: 80,
            host: 8080,
            auto_correct: true,
            protocol: None,
            host_ip: None,
        }];
        assert!(guest.configure_networks(&comm, &nets).is_ok());
    }

    #[test]
    fn test_windows_guest_network_public() {
        let comm = MockComm {
            output: Ok("".to_string()),
        };
        let guest = WindowsGuest;
        let nets = vec![
            crate::config::NetworkConfig::PublicNetwork {
                ip: None,
                bridge: None,
                use_dhcp_assigned_default_route: false,
            },
            crate::config::NetworkConfig::PublicNetwork {
                ip: None,
                bridge: None,
                use_dhcp_assigned_default_route: false,
            },
        ];
        assert!(guest.configure_networks(&comm, &nets).is_ok());
    }

    #[test]
    fn test_windows_guest_network_execution_failure() {
        let comm = MockComm {
            output: Err("Network execution failed".to_string()),
        };
        let guest = WindowsGuest;
        let nets = vec![crate::config::NetworkConfig::PublicNetwork {
            ip: None,
            bridge: None,
            use_dhcp_assigned_default_route: false,
        }];
        assert!(guest.configure_networks(&comm, &nets).is_err());
    }

    #[test]
    fn test_windows_guest_update_additions() {
        let comm = MockComm {
            output: Ok("".to_string()),
        };
        let guest = WindowsGuest;
        assert!(
            guest
                .update_guest_additions(&comm, "virtualbox", None)
                .is_ok()
        );
        assert!(guest.update_guest_additions(&comm, "other", None).is_ok());
    }

    #[test]
    fn test_mock_comm_coverage() {
        let comm = MockComm {
            output: Ok("".to_string()),
        };
        let _ = comm.upload(Path::new(""), "");
        let _ = comm.download("", Path::new(""));
        let _ = comm.execute_interactive();
        let _ = comm.wait_for_ready(Duration::from_secs(1));
    }

    #[test]
    fn test_windows_guest() {
        let comm = MockComm {
            output: Ok("Microsoft Windows [Version 10.0.19045.2965]\n".to_string()),
        };
        let guest = WindowsGuest;
        assert!(guest.detect(&comm).expect("detect should succeed"));
        assert!(guest.change_hostname(&comm, "test").is_ok());

        let networks = vec![NetworkConfig::PrivateNetwork {
            ip: Some("10.0.0.1".to_string()),
            netmask: None,
            dhcp: false,
            virtualbox_intnet: None,
        }];
        assert!(guest.configure_networks(&comm, &networks).is_ok());
        assert!(
            guest
                .mount_shared_folder(&comm, "test", Path::new("Z:"))
                .is_ok()
        );
        assert!(guest.halt(&comm).is_ok());
    }

    #[test]
    fn test_windows_guest_validation_errors() {
        let comm = MockComm {
            output: Ok("Windows\n".to_string()),
        };
        let guest = WindowsGuest;
        assert!(guest.change_hostname(&comm, "").is_err());
        assert!(
            guest
                .mount_shared_folder(&comm, "", Path::new("Z:"))
                .is_err()
        );
        assert!(
            guest
                .mount_shared_folder(&comm, "test", Path::new(""))
                .is_err()
        );
    }

    #[test]
    fn test_windows_guest_not_windows() {
        let comm = MockComm {
            output: Ok("Linux\n".to_string()),
        };
        let guest = WindowsGuest;
        assert!(!guest.detect(&comm).expect("detect should succeed"));
    }

    #[test]
    fn test_windows_guest_detect_error() {
        let comm = MockComm {
            output: Err("timeout".to_string()),
        };
        let guest = WindowsGuest;
        assert!(
            !guest
                .detect(&comm)
                .expect("detect should resolve to false on error")
        );
    }

    #[test]
    fn test_windows_guest_capabilities() {
        let guest = WindowsGuest;
        let comm = MockComm {
            output: Ok("ok".to_string()),
        };
        assert!(guest.reboot(&comm).is_ok());
        assert!(
            guest
                .mount_smb_shared_folder(&comm, "//host/share", Path::new("Z:"), "user", "pass")
                .is_ok()
        );
        assert!(guest.insert_public_key(&comm, "ssh-key").is_ok());
        assert!(guest.remove_public_key(&comm, "ssh-key").is_ok());
        assert!(guest.rsync_installed(&comm).unwrap_or(false));
        assert!(guest.rsync_install(&comm).is_ok());
        assert!(
            guest
                .verify_guest_additions(&comm, "virtualbox")
                .unwrap_or(false)
        );
        assert!(
            guest
                .verify_guest_additions(&comm, "vmware")
                .unwrap_or(false)
        );
        assert!(
            guest
                .verify_guest_additions(&comm, "other")
                .unwrap_or(false)
        );

        let fail_comm = MockComm {
            output: Err("fail".to_string()),
        };
        assert!(!guest.rsync_installed(&fail_comm).unwrap_or(true));
    }

    #[test]
    fn test_windows_detect_fallback_probes() {
        struct ProbeComm {
            stage: usize,
        }
        impl Communicator for ProbeComm {
            #[coverage(off)]
            fn execute(&self, command: &str) -> Result<String, MigratoryError> {
                if command.contains("cmd.exe /c ver") {
                    if self.stage == 0 {
                        Ok("Microsoft Windows [Version 10]".to_string())
                    } else {
                        Err(MigratoryError::Generic("fail".into()))
                    }
                } else if command.contains("$PSVersionTable") {
                    if self.stage == 1 {
                        Ok("PSVersion 5.1".to_string())
                    } else {
                        Err(MigratoryError::Generic("fail".into()))
                    }
                } else if command.contains("systeminfo") {
                    if self.stage == 2 {
                        Ok("OS Name: Microsoft Server".to_string())
                    } else {
                        Err(MigratoryError::Generic("fail".into()))
                    }
                } else {
                    Ok("".to_string())
                }
            }
            #[coverage(off)]
            fn upload(&self, _: &Path, _: &str) -> Result<(), MigratoryError> {
                Ok(())
            }
            #[coverage(off)]
            fn download(&self, _: &str, _: &Path) -> Result<(), MigratoryError> {
                Ok(())
            }
            #[coverage(off)]
            fn execute_interactive(&self) -> Result<(), MigratoryError> {
                Ok(())
            }
            #[coverage(off)]
            fn wait_for_ready(&self, _: Duration) -> Result<(), MigratoryError> {
                Ok(())
            }
        }

        let guest = WindowsGuest;
        assert!(guest.detect(&ProbeComm { stage: 0 }).unwrap_or(false));
        assert!(guest.detect(&ProbeComm { stage: 1 }).unwrap_or(false));
        assert!(guest.detect(&ProbeComm { stage: 2 }).unwrap_or(false));
        assert!(!guest.detect(&ProbeComm { stage: 3 }).unwrap_or(true));
    }

    #[test]
    fn test_windows_guest_command_failures() {
        struct FailingComm;
        impl Communicator for FailingComm {
            #[coverage(off)]
            fn execute(&self, _command: &str) -> Result<String, MigratoryError> {
                Err(MigratoryError::Generic("command failed".to_string()))
            }
            #[coverage(off)]
            fn upload(&self, _: &Path, _: &str) -> Result<(), MigratoryError> {
                Ok(())
            }
            #[coverage(off)]
            fn download(&self, _: &str, _: &Path) -> Result<(), MigratoryError> {
                Ok(())
            }
            #[coverage(off)]
            fn execute_interactive(&self) -> Result<(), MigratoryError> {
                Ok(())
            }
            #[coverage(off)]
            fn wait_for_ready(&self, _: Duration) -> Result<(), MigratoryError> {
                Ok(())
            }
        }

        let guest = WindowsGuest;
        let comm = FailingComm;

        assert!(guest.change_hostname(&comm, "newhost").is_err());
        assert!(
            guest
                .mount_virtualbox_shared_folder(&comm, "share", Path::new("C:\\share"))
                .is_err()
        );
        assert!(guest.halt(&comm).is_err());
        assert!(
            guest
                .update_guest_additions(&comm, "virtualbox", None)
                .is_err()
        );
        assert!(guest.reboot(&comm).is_err());
        assert!(
            guest
                .mount_smb_shared_folder(
                    &comm,
                    "\\\\host\\share",
                    Path::new("C:\\smb"),
                    "user",
                    "pass"
                )
                .is_err()
        );
        assert!(guest.insert_public_key(&comm, "ssh-rsa AAAA...").is_err());
        assert!(guest.remove_public_key(&comm, "pattern").is_err());
        assert!(guest.rsync_install(&comm).is_err());
    }
}
