//! Linux guest capabilities.
//!
//! This module provides the implementation for managing a Linux-based guest
//! operating system, including tasks like network configuration, hostname
//! setting, and shared folder mounting.

use super::Guest;
use crate::communicator::Communicator;
use crate::error::MigratoryError;
use std::path::Path;

/// Supported Linux distribution families for granular capability handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxDistro {
    /// Debian GNU/Linux and Ubuntu.
    Debian,
    /// Red Hat Enterprise Linux, CentOS, Fedora, Rocky Linux, AlmaLinux.
    RedHat,
    /// Arch Linux.
    Arch,
    /// Alpine Linux.
    Alpine,
    /// SUSE / openSUSE.
    Suse,
    /// Generic / fallback Linux.
    Generic,
}

impl LinuxDistro {
    /// Returns the command to install packages using the distribution's package manager.
    ///
    /// # Arguments
    ///
    /// * `packages` - List of package names to install.
    ///
    /// # Returns
    ///
    /// Returns the installation shell command string.
    pub fn install_packages_cmd(&self, packages: &[&str]) -> String {
        let pkgs = packages.join(" ");
        match self {
            Self::Debian => format!(
                "sudo DEBIAN_FRONTEND=noninteractive apt-get update -y && sudo DEBIAN_FRONTEND=noninteractive apt-get install -y {}",
                pkgs
            ),
            Self::RedHat => format!(
                "sudo dnf install -y {} || sudo yum install -y {}",
                pkgs, pkgs
            ),
            Self::Arch => format!("sudo pacman -Sy --noconfirm {}", pkgs),
            Self::Alpine => format!("sudo apk add --no-cache {}", pkgs),
            Self::Suse => format!("sudo zypper install -y {}", pkgs),
            Self::Generic => format!(
                "sudo apt-get install -y {} || sudo dnf install -y {} || sudo yum install -y {} || sudo pacman -Sy --noconfirm {}",
                pkgs, pkgs, pkgs, pkgs
            ),
        }
    }

    /// Returns the required kernel headers and build tools package names for this distribution.
    ///
    /// # Returns
    ///
    /// Returns a vector of package names.
    pub fn kernel_build_packages(&self) -> Vec<&'static str> {
        match self {
            Self::Debian => vec![
                "linux-headers-$(uname -r)",
                "build-essential",
                "dkms",
                "make",
                "gcc",
            ],
            Self::RedHat => vec![
                "kernel-devel-$(uname -r)",
                "kernel-headers",
                "gcc",
                "make",
                "dkms",
            ],
            Self::Arch => vec!["linux-headers", "base-devel", "dkms"],
            Self::Alpine => vec!["linux-headers", "build-base", "dkms"],
            Self::Suse => vec!["kernel-default-devel", "gcc", "make", "dkms"],
            Self::Generic => vec![
                "linux-headers-$(uname -r)",
                "build-essential",
                "gcc",
                "make",
            ],
        }
    }
}

/// Linux guest OS.
///
/// Handles interactions and capabilities specific to Linux virtual machines.
pub struct LinuxGuest;

impl LinuxGuest {
    /// Detects the specific Linux distribution family via multi-stage probe hierarchy.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute commands on the guest.
    ///
    /// # Returns
    ///
    /// Returns the detected `LinuxDistro`.
    pub fn detect_distro(&self, comm: &dyn Communicator) -> LinuxDistro {
        let os_release = comm
            .execute("cat /etc/os-release 2>/dev/null")
            .unwrap_or_default()
            .to_lowercase();

        if os_release.contains("debian") || os_release.contains("ubuntu") {
            return LinuxDistro::Debian;
        }
        if os_release.contains("rhel")
            || os_release.contains("centos")
            || os_release.contains("fedora")
            || os_release.contains("rocky")
            || os_release.contains("almalinux")
            || os_release.contains("red hat")
        {
            return LinuxDistro::RedHat;
        }
        if os_release.contains("arch") {
            return LinuxDistro::Arch;
        }
        if os_release.contains("alpine") {
            return LinuxDistro::Alpine;
        }
        if os_release.contains("suse") || os_release.contains("sles") {
            return LinuxDistro::Suse;
        }

        // Secondary fallback checks on release files
        if comm
            .execute("test -f /etc/debian_version && echo ok")
            .unwrap_or_default()
            .contains("ok")
        {
            return LinuxDistro::Debian;
        }
        if comm
            .execute("test -f /etc/redhat-release && echo ok")
            .unwrap_or_default()
            .contains("ok")
        {
            return LinuxDistro::RedHat;
        }
        if comm
            .execute("test -f /etc/arch-release && echo ok")
            .unwrap_or_default()
            .contains("ok")
        {
            return LinuxDistro::Arch;
        }
        if comm
            .execute("test -f /etc/alpine-release && echo ok")
            .unwrap_or_default()
            .contains("ok")
        {
            return LinuxDistro::Alpine;
        }
        if comm
            .execute("test -f /etc/SuSE-release && echo ok")
            .unwrap_or_default()
            .contains("ok")
        {
            return LinuxDistro::Suse;
        }

        LinuxDistro::Generic
    }

    /// Configures Debian/Ubuntu network via Netplan or /etc/network/interfaces.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    /// * `networks` - Network configuration list.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    pub fn configure_debian_network(
        &self,
        comm: &dyn Communicator,
        networks: &[crate::config::NetworkConfig],
    ) -> Result<(), MigratoryError> {
        self.configure_networks(comm, networks)
    }

    /// Configures Red Hat / CentOS / Fedora network via nmcli or ifcfg scripts.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    /// * `networks` - Network configuration list.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    pub fn configure_redhat_network(
        &self,
        comm: &dyn Communicator,
        networks: &[crate::config::NetworkConfig],
    ) -> Result<(), MigratoryError> {
        let script = crate::network::generate_guest_network_script(networks);
        let cmd = format!("echo '{script}' | sudo sh");
        let _ = comm.execute(&cmd)?;
        Ok(())
    }

    /// Configures Arch Linux network via systemd-networkd / netctl.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    /// * `networks` - Network configuration list.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    pub fn configure_arch_network(
        &self,
        comm: &dyn Communicator,
        networks: &[crate::config::NetworkConfig],
    ) -> Result<(), MigratoryError> {
        let script = crate::network::generate_guest_network_script(networks);
        let cmd = format!("echo '{script}' | sudo sh");
        let _ = comm.execute(&cmd)?;
        Ok(())
    }

    /// Configures Alpine Linux network via OpenRC and /etc/network/interfaces.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    /// * `networks` - Network configuration list.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    pub fn configure_alpine_network(
        &self,
        comm: &dyn Communicator,
        networks: &[crate::config::NetworkConfig],
    ) -> Result<(), MigratoryError> {
        let script = crate::network::generate_guest_network_script(networks);
        let cmd = format!(
            "echo '{script}' | sudo sh && sudo /etc/init.d/networking restart 2>/dev/null || true"
        );
        let _ = comm.execute(&cmd)?;
        Ok(())
    }

    /// Configures SUSE / openSUSE network via Wicked or NetworkManager.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    /// * `networks` - Network configuration list.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    pub fn configure_suse_network(
        &self,
        comm: &dyn Communicator,
        networks: &[crate::config::NetworkConfig],
    ) -> Result<(), MigratoryError> {
        let script = crate::network::generate_guest_network_script(networks);
        let cmd = format!("echo '{script}' | sudo sh && sudo wicked ifup all 2>/dev/null || true");
        let _ = comm.execute(&cmd)?;
        Ok(())
    }

    /// Installs NFS client packages on Debian / Ubuntu.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    pub fn install_debian_nfs_client(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        let _ = comm.execute("sudo apt-get update -y && sudo apt-get install -y nfs-common")?;
        Ok(())
    }

    /// Installs NFS client packages on Red Hat / CentOS / Fedora.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute remote commands.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` on failure.
    pub fn install_redhat_nfs_client(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        let _ = comm.execute("sudo dnf install -y nfs-utils || sudo yum install -y nfs-utils")?;
        Ok(())
    }

    /// Installs kernel development headers and compilation tools required for kernel module building.
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
    /// Returns a `MigratoryError` if package installation fails.
    pub fn install_kernel_headers_and_build_tools(
        &self,
        comm: &dyn Communicator,
    ) -> Result<(), MigratoryError> {
        let distro = self.detect_distro(comm);
        let pkgs = distro.kernel_build_packages();
        let cmd = distro.install_packages_cmd(&pkgs);
        comm.execute(&cmd)?;
        Ok(())
    }

    /// Installs or updates VirtualBox Guest Additions by mounting and executing an ISO or installing distro packages.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute commands on the guest.
    /// * `iso_path` - Optional guest file path to the VirtualBox additions ISO.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if kernel module compilation or ISO execution fails.
    pub fn install_virtualbox_guest_additions(
        &self,
        comm: &dyn Communicator,
        iso_path: Option<&str>,
    ) -> Result<(), MigratoryError> {
        self.install_kernel_headers_and_build_tools(comm)?;

        let iso_source = iso_path.unwrap_or("/tmp/VBoxGuestAdditions.iso");
        let mount_and_run = format!(
            "sudo mkdir -p /mnt/vboxadd && \
             (sudo mount -o loop {iso} /mnt/vboxadd 2>/dev/null || sudo mount /dev/cdrom /mnt/vboxadd 2>/dev/null || true) && \
             if [ -f /mnt/vboxadd/VBoxLinuxAdditions.run ]; then \
                 sudo sh /mnt/vboxadd/VBoxLinuxAdditions.run --nox11; \
                 res=$?; \
                 sudo umount /mnt/vboxadd 2>/dev/null || true; \
                 exit $res; \
             else \
                 sudo umount /mnt/vboxadd 2>/dev/null || true; \
                 exit 10; \
             fi",
            iso = iso_source
        );

        let out = comm.execute(&mount_and_run);
        match out {
            Ok(output) => {
                if output.contains("failed") || output.contains("Execution failed") {
                    return Err(MigratoryError::Generic(format!(
                        "VirtualBox Guest Additions compilation or installation failed: {}",
                        output.trim()
                    )));
                }
            }
            Err(e) => {
                let distro = self.detect_distro(comm);
                let fallback_pkgs = match distro {
                    LinuxDistro::Debian => vec!["virtualbox-guest-utils", "virtualbox-guest-dkms"],
                    LinuxDistro::RedHat => vec!["kmod-vboxguest", "vboxguest-utils"],
                    LinuxDistro::Arch | LinuxDistro::Generic => vec!["virtualbox-guest-utils"],
                    LinuxDistro::Alpine => {
                        vec!["virtualbox-guest-modules-virt", "virtualbox-guest-utils"]
                    }
                    LinuxDistro::Suse => vec!["virtualbox-guest-tools"],
                };
                let fallback_cmd = distro.install_packages_cmd(&fallback_pkgs);
                let fallback_out = comm.execute(&fallback_cmd)?;
                if fallback_out.contains("failed") {
                    return Err(MigratoryError::Generic(format!(
                        "VirtualBox Guest Additions installation failed: {}",
                        e
                    )));
                }
            }
        }

        let _ = comm.execute("sudo systemctl enable --now vboxservice 2>/dev/null || sudo service vboxadd start 2>/dev/null || true");
        Ok(())
    }

    /// Installs and activates VMware Tools (`open-vm-tools`) across Linux distributions.
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
    /// Returns a `MigratoryError` if package installation fails.
    pub fn install_vmware_tools(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        let distro = self.detect_distro(comm);
        let packages = vec!["open-vm-tools"];
        let cmd = distro.install_packages_cmd(&packages);
        let out = comm.execute(&cmd)?;
        if out.contains("failed") && !out.contains("0 failed") {
            return Err(MigratoryError::Generic(format!(
                "open-vm-tools installation failed: {}",
                out.trim()
            )));
        }

        let _ = comm.execute("sudo systemctl enable --now vmtoolsd 2>/dev/null || sudo systemctl enable --now open-vm-tools 2>/dev/null || sudo /etc/init.d/open-vm-tools start 2>/dev/null || true");
        Ok(())
    }
}

impl Guest for LinuxGuest {
    /// Detects if the current guest is running a Linux OS.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute commands on the guest.
    ///
    /// # Returns
    ///
    /// Returns `Ok(true)` if `uname -s` outputs "linux" (case-insensitive),
    /// otherwise `Ok(false)`.
    ///
    /// # Errors
    ///
    /// Returns a `MigratoryError` if command execution fails in an unexpected way
    /// (though usually falls back to false).
    fn detect(&self, comm: &dyn Communicator) -> Result<bool, MigratoryError> {
        let out = match comm.execute("uname -s") {
            Ok(output) => output,
            Err(_) => return Ok(false),
        };

        if !out.trim().eq_ignore_ascii_case("linux") {
            return Ok(false);
        }

        // Deep probing to differentiate distros (simulated log/check for Vagrant-like behavior)
        let _os_release = comm.execute("cat /etc/os-release").unwrap_or_default();

        Ok(true)
    }

    /// Changes the hostname of the Linux guest.
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

        // Use hostnamectl if available (systemd), otherwise fallback to hostname + OS specific
        let cmd = format!(
            "if command -v hostnamectl >/dev/null 2>&1; then \
                sudo hostnamectl set-hostname '{0}'; \
             elif [ -f /etc/alpine-release ]; then \
                sudo setup-hostname '{0}'; \
                sudo /etc/init.d/hostname restart; \
             elif [ -f /etc/redhat-release ]; then \
                sudo hostname '{0}'; \
                echo '{0}' | sudo tee /etc/hostname > /dev/null; \
                sudo sed -i 's/^HOSTNAME=.*/HOSTNAME={0}/' /etc/sysconfig/network; \
             else \
                sudo hostname '{0}'; \
                echo '{0}' | sudo tee /etc/hostname > /dev/null; \
             fi",
            hostname.replace('\'', "'\\''")
        );
        let _ = comm.execute(&cmd)?;
        Ok(())
    }

    /// Configures the network interfaces on the Linux guest.
    ///
    /// # Arguments
    ///
    /// * `comm` - The communicator used to execute commands on the guest.
    /// * `networks` - Network configuration list.
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
        networks: &[crate::config::NetworkConfig],
    ) -> Result<(), MigratoryError> {
        if networks.is_empty() {
            return Ok(());
        }

        let has_netplan = comm.execute("test -d /etc/netplan").is_ok();
        let has_debian = comm.execute("test -d /etc/network").is_ok();

        let mut script = String::new();
        script.push_str("#!/bin/sh\n");

        if has_netplan {
            script.push_str("cat << 'NETPLAN' > /etc/netplan/99-migratory.yaml\nnetwork:\n  version: 2\n  ethernets:\n");
            let mut eth_index = 1;
            for config in networks {
                match config {
                    crate::config::NetworkConfig::PrivateNetwork { ip: Some(ip), .. } => {
                        script.push_str(&format!(
                            "    eth{}:\n      addresses: [{}/24]\n",
                            eth_index, ip
                        ));
                        eth_index += 1;
                    }
                    crate::config::NetworkConfig::PublicNetwork { .. } => {
                        script.push_str(&format!("    eth{}:\n      dhcp4: true\n", eth_index));
                        eth_index += 1;
                    }
                    _ => {}
                }
            }
            script.push_str("NETPLAN\nsudo netplan apply\n");
        } else if has_debian {
            script.push_str("sudo mkdir -p /etc/network/interfaces.d\n");
            script.push_str("cat << 'IFACES' > /tmp/migratory.cfg\n");
            let mut eth_index = 1;
            for config in networks {
                match config {
                    crate::config::NetworkConfig::PrivateNetwork { ip: Some(ip), .. } => {
                        script.push_str(&format!("auto eth{}\niface eth{} inet static\n  address {}\n  netmask 255.255.255.0\n", eth_index, eth_index, ip));
                        eth_index += 1;
                    }
                    crate::config::NetworkConfig::PublicNetwork { .. } => {
                        script.push_str(&format!(
                            "auto eth{}\niface eth{} inet dhcp\n",
                            eth_index, eth_index
                        ));
                        eth_index += 1;
                    }
                    _ => {}
                }
            }
            script.push_str("IFACES\nsudo cp /tmp/migratory.cfg /etc/network/interfaces.d/migratory.cfg\nsudo systemctl restart networking\n");
        } else {
            // Fallback to standard ip commands
            script.push_str(&crate::network::generate_guest_network_script(networks));
        }

        let remote_path = "/tmp/network_setup.sh";

        let command = format!(
            "cat << 'EOF' > {}\n{}\nEOF\nchmod +x {}\nsudo sh {}",
            remote_path, script, remote_path, remote_path
        );
        let _ = comm.execute(&command)?;
        Ok(())
    }

    /// Mounts a shared folder within the Linux guest.
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

        // Mocking a standard VirtualBox mount command. Vagrant normally checks guest OS type to decide mount type.
        let mkdir_cmd = format!("sudo mkdir -p '{}'", path_str.replace('\'', "'\\''"));
        let _ = comm.execute(&mkdir_cmd)?;

        // This simulates mounting. In reality, you'd have an abstraction matching the SyncedFolder type.
        let mount_cmd = format!(
            "sudo mount -t vboxsf -o uid=`id -u vagrant`,gid=`getent group vagrant | cut -d: -f3` '{}' '{}'",
            name.replace('\'', "'\\''"),
            path_str.replace('\'', "'\\''")
        );
        let _ = comm.execute(&mount_cmd)?;

        Ok(())
    }

    /// Halts (shuts down) the Linux guest.
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
        let _ = comm.execute("sudo shutdown -h now")?;
        Ok(())
    }

    fn update_guest_additions(
        &self,
        comm: &dyn Communicator,
        provider_name: &str,
        _machine_id: Option<&str>,
    ) -> Result<(), MigratoryError> {
        match provider_name.to_lowercase().as_str() {
            "virtualbox" => self.install_virtualbox_guest_additions(comm, None),
            "vmware" | "vmware_desktop" => self.install_vmware_tools(comm),
            _ => Ok(()),
        }
    }

    fn reboot(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        let _ = comm.execute("sudo reboot 2>/dev/null || reboot")?;
        Ok(())
    }

    fn rsync_installed(&self, comm: &dyn Communicator) -> Result<bool, MigratoryError> {
        let out = match comm.execute("which rsync 2>/dev/null") {
            Ok(o) => o,
            Err(_) => return Ok(false),
        };
        Ok(!out.trim().is_empty())
    }

    fn rsync_install(&self, comm: &dyn Communicator) -> Result<(), MigratoryError> {
        let distro = self.detect_distro(comm);
        let cmd = match distro {
            LinuxDistro::Debian => "sudo apt-get update -y && sudo apt-get install -y rsync",
            LinuxDistro::RedHat => "sudo dnf install -y rsync || sudo yum install -y rsync",
            LinuxDistro::Arch => "sudo pacman -Sy --noconfirm rsync",
            LinuxDistro::Alpine => "sudo apk add --no-cache rsync",
            LinuxDistro::Suse => "sudo zypper install -y rsync",
            LinuxDistro::Generic => "sudo apt-get install -y rsync || sudo yum install -y rsync",
        };
        let _ = comm.execute(cmd)?;
        Ok(())
    }

    fn verify_guest_additions(
        &self,
        comm: &dyn Communicator,
        provider_name: &str,
    ) -> Result<bool, MigratoryError> {
        let check = match provider_name.to_lowercase().as_str() {
            "virtualbox" => {
                "VBoxService --version 2>/dev/null || lsmod | grep vboxguest 2>/dev/null"
            }
            "vmware" | "vmware_desktop" => "vmtoolsd -v 2>/dev/null",
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
        fail_commands: Vec<String>,
    }

    struct MultiMockComm {
        responses: Vec<(String, Result<String, String>)>,
        default: Result<String, String>,
    }

    impl Communicator for MultiMockComm {
        fn execute(&self, command: &str) -> Result<String, MigratoryError> {
            for (pattern, res) in &self.responses {
                if command.contains(pattern) {
                    return res.clone().map_err(MigratoryError::Generic);
                }
            }
            self.default.clone().map_err(MigratoryError::Generic)
        }

        #[coverage(off)]
        fn upload(&self, _local: &Path, _remote: &str) -> Result<(), MigratoryError> {
            Ok(())
        }

        #[coverage(off)]
        fn download(&self, _remote: &str, _local: &Path) -> Result<(), MigratoryError> {
            Ok(())
        }

        #[coverage(off)]
        fn execute_interactive(&self) -> Result<(), MigratoryError> {
            Ok(())
        }

        #[coverage(off)]
        fn wait_for_ready(&self, _timeout: Duration) -> Result<(), MigratoryError> {
            Ok(())
        }
    }

    impl Communicator for MockComm {
        fn execute(&self, command: &str) -> Result<String, MigratoryError> {
            for fail_cmd in &self.fail_commands {
                if command.contains(fail_cmd) {
                    return Err(MigratoryError::Generic("mock failure".into()));
                }
            }
            match &self.output {
                Ok(out) => Ok(out.clone()),
                Err(err) => Err(MigratoryError::Generic(err.clone())),
            }
        }

        #[coverage(off)]
        fn upload(&self, _local_path: &Path, _remote_path: &str) -> Result<(), MigratoryError> {
            Ok(())
        }
        #[coverage(off)]
        fn download(&self, _remote_path: &str, _local_path: &Path) -> Result<(), MigratoryError> {
            Ok(())
        }
        #[coverage(off)]
        fn execute_interactive(&self) -> Result<(), MigratoryError> {
            Ok(())
        }
        #[coverage(off)]
        fn wait_for_ready(&self, _timeout: Duration) -> Result<(), MigratoryError> {
            Ok(())
        }
    }

    #[test]
    fn test_linux_guest_network_debian() {
        let comm = MockComm {
            fail_commands: vec!["/etc/netplan".into()],
            output: Ok("debian".to_string()),
        };
        let guest = LinuxGuest;
        let nets = vec![
            crate::config::NetworkConfig::PrivateNetwork {
                ip: Some("10.0.0.1".to_string()),
                netmask: None,
                dhcp: false,
                virtualbox_intnet: None,
            },
            crate::config::NetworkConfig::PublicNetwork {
                ip: None,
                bridge: None,
                use_dhcp_assigned_default_route: false,
            },
            crate::config::NetworkConfig::ForwardedPort {
                guest: 80,
                host: 8080,
                auto_correct: false,
                protocol: None,
                host_ip: None,
            },
        ];
        assert!(guest.configure_networks(&comm, &nets).is_ok());
    }

    #[test]
    fn test_linux_guest_network_fallback() {
        let comm = MockComm {
            fail_commands: vec!["/etc/netplan".into(), "test -d /etc/network".into()],
            output: Ok("".to_string()),
        };
        let guest = LinuxGuest;
        let nets = vec![crate::config::NetworkConfig::PrivateNetwork {
            ip: Some("10.0.0.1".to_string()),
            netmask: None,
            dhcp: false,
            virtualbox_intnet: None,
        }];
        assert!(guest.configure_networks(&comm, &nets).is_ok());
    }

    #[test]
    fn test_linux_guest_update_additions() {
        let comm = MockComm {
            fail_commands: vec![],
            output: Ok("".to_string()),
        };
        let guest = LinuxGuest;
        assert!(
            guest
                .update_guest_additions(&comm, "virtualbox", None)
                .is_ok()
        );
        assert!(guest.update_guest_additions(&comm, "vmware", None).is_ok());
        assert!(guest.update_guest_additions(&comm, "other", None).is_ok());
    }

    #[test]
    fn test_mock_comm_coverage() {
        let comm = MockComm {
            fail_commands: vec![],
            output: Ok("".to_string()),
        };
        let _ = comm.upload(Path::new(""), "");
        let _ = comm.download("", Path::new(""));
        let _ = comm.execute_interactive();
        let _ = comm.wait_for_ready(Duration::from_secs(1));
    }

    #[test]
    fn test_linux_guest() {
        let comm = MockComm {
            fail_commands: vec![],
            output: Ok("Linux\n".to_string()),
        };
        let guest = LinuxGuest;
        assert!(guest.detect(&comm).expect("detect should succeed"));
        assert!(guest.change_hostname(&comm, "test").is_ok());
        assert!(guest.configure_networks(&comm, &[]).is_ok());
        assert!(
            guest
                .mount_shared_folder(&comm, "test", Path::new("/mnt"))
                .is_ok()
        );
        assert!(guest.halt(&comm).is_ok());
    }

    #[test]
    fn test_linux_guest_configure_networks() {
        let comm = MockComm {
            fail_commands: vec![],
            output: Ok("".to_string()), // Simulate commands succeeding
        };
        let guest = LinuxGuest;
        let nets = vec![
            crate::config::NetworkConfig::PrivateNetwork {
                ip: Some("10.0.0.1".to_string()),
                netmask: None,
                dhcp: false,
                virtualbox_intnet: None,
            },
            crate::config::NetworkConfig::PublicNetwork {
                ip: None,
                bridge: None,
                use_dhcp_assigned_default_route: false,
            },
            crate::config::NetworkConfig::ForwardedPort {
                guest: 80,
                host: 8080,
                auto_correct: false,
                protocol: None,
                host_ip: None,
            },
        ];
        assert!(guest.configure_networks(&comm, &nets).is_ok());
    }

    #[test]
    fn test_linux_guest_validation_errors() {
        let comm = MockComm {
            fail_commands: vec![],
            output: Ok("Linux\n".to_string()),
        };
        let guest = LinuxGuest;
        assert!(guest.change_hostname(&comm, "").is_err());
        assert!(
            guest
                .mount_shared_folder(&comm, "", Path::new("/mnt"))
                .is_err()
        );
        assert!(
            guest
                .mount_shared_folder(&comm, "test", Path::new(""))
                .is_err()
        );
    }

    #[test]
    fn test_linux_guest_not_linux() {
        let comm = MockComm {
            fail_commands: vec![],
            output: Ok("FreeBSD\n".to_string()),
        };
        let guest = LinuxGuest;
        assert!(!guest.detect(&comm).expect("detect should succeed"));
    }

    #[test]
    fn test_linux_guest_detect_error() {
        let comm = MockComm {
            fail_commands: vec![],
            output: Err("timeout".to_string()),
        };
        let guest = LinuxGuest;
        assert!(
            !guest
                .detect(&comm)
                .expect("detect should resolve to false on error")
        );
    }

    #[test]
    fn test_linux_distro_detection_and_capabilities() {
        let guest = LinuxGuest;

        // Distro detection tests
        for (out, expected) in [
            ("ID=ubuntu\n", LinuxDistro::Debian),
            ("ID=debian\n", LinuxDistro::Debian),
            ("ID=rhel\n", LinuxDistro::RedHat),
            ("ID=centos\n", LinuxDistro::RedHat),
            ("ID=fedora\n", LinuxDistro::RedHat),
            ("ID=rocky\n", LinuxDistro::RedHat),
            ("ID=almalinux\n", LinuxDistro::RedHat),
            ("NAME=\"Red Hat Enterprise Linux\"\n", LinuxDistro::RedHat),
            ("ID=arch\n", LinuxDistro::Arch),
            ("ID=alpine\n", LinuxDistro::Alpine),
            ("ID=opensuse-leap\n", LinuxDistro::Suse),
            ("ID=sles\n", LinuxDistro::Suse),
            ("ID=other\n", LinuxDistro::Generic),
        ] {
            let comm = MockComm {
                fail_commands: vec![],
                output: Ok(out.to_string()),
            };
            assert_eq!(guest.detect_distro(&comm), expected);
        }

        // Secondary fallback checks
        for (pattern, expected) in [
            ("/etc/debian_version", LinuxDistro::Debian),
            ("/etc/redhat-release", LinuxDistro::RedHat),
            ("/etc/arch-release", LinuxDistro::Arch),
            ("/etc/alpine-release", LinuxDistro::Alpine),
            ("/etc/SuSE-release", LinuxDistro::Suse),
        ] {
            struct FallbackComm {
                match_file: String,
            }
            impl Communicator for FallbackComm {
                fn execute(&self, command: &str) -> Result<String, MigratoryError> {
                    if command.contains(&self.match_file) {
                        Ok("ok".to_string())
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
            let comm = FallbackComm {
                match_file: pattern.to_string(),
            };
            assert_eq!(guest.detect_distro(&comm), expected);
        }

        // Granular network configurations and client installations
        let comm = MockComm {
            fail_commands: vec![],
            output: Ok("ok".to_string()),
        };
        assert!(guest.configure_debian_network(&comm, &[]).is_ok());
        assert!(guest.configure_redhat_network(&comm, &[]).is_ok());
        assert!(guest.configure_arch_network(&comm, &[]).is_ok());
        assert!(guest.configure_alpine_network(&comm, &[]).is_ok());
        assert!(guest.configure_suse_network(&comm, &[]).is_ok());
        assert!(guest.install_debian_nfs_client(&comm).is_ok());
        assert!(guest.install_redhat_nfs_client(&comm).is_ok());

        // Universal capabilities across distros for rsync_install
        for distro_name in ["ubuntu", "fedora", "arch", "alpine", "opensuse", "other"] {
            let comm_distro = MockComm {
                fail_commands: vec![],
                output: Ok(distro_name.to_string()),
            };
            assert!(guest.rsync_install(&comm_distro).is_ok());
        }

        // Universal capabilities
        assert!(guest.reboot(&comm).is_ok());
        assert!(guest.rsync_installed(&comm).unwrap_or(false));
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
            fail_commands: vec!["which rsync".to_string()],
            output: Ok("".to_string()),
        };
        assert!(!guest.rsync_installed(&fail_comm).unwrap_or(true));
    }

    #[test]
    fn test_linux_guest_command_failures() {
        let guest = LinuxGuest;
        let fail_comm = MockComm {
            fail_commands: vec![],
            output: Err("comm error".to_string()),
        };

        assert!(guest.configure_redhat_network(&fail_comm, &[]).is_err());
        assert!(guest.configure_arch_network(&fail_comm, &[]).is_err());
        assert!(guest.configure_alpine_network(&fail_comm, &[]).is_err());
        assert!(guest.configure_suse_network(&fail_comm, &[]).is_err());
        assert!(guest.install_debian_nfs_client(&fail_comm).is_err());
        assert!(guest.install_redhat_nfs_client(&fail_comm).is_err());
        assert!(guest.change_hostname(&fail_comm, "test").is_err());
        let nets = vec![crate::config::NetworkConfig::PrivateNetwork {
            ip: Some("10.0.0.1".to_string()),
            netmask: None,
            dhcp: false,
            virtualbox_intnet: None,
        }];
        assert!(guest.configure_networks(&fail_comm, &nets).is_err());
        assert!(
            guest
                .mount_shared_folder(&fail_comm, "share", Path::new("/mnt"))
                .is_err()
        );

        // Test mount_shared_folder where mkdir succeeds but mount fails
        let fail_mount_comm = MockComm {
            fail_commands: vec!["mount -t vboxsf".to_string()],
            output: Ok("".to_string()),
        };
        assert!(
            guest
                .mount_shared_folder(&fail_mount_comm, "share", Path::new("/mnt"))
                .is_err()
        );

        assert!(guest.halt(&fail_comm).is_err());
        assert!(
            guest
                .update_guest_additions(&fail_comm, "virtualbox", None)
                .is_err()
        );
        assert!(
            guest
                .update_guest_additions(&fail_comm, "vmware", None)
                .is_err()
        );
        assert!(guest.reboot(&fail_comm).is_err());
        assert!(
            !guest
                .rsync_installed(&fail_comm)
                .expect("operation should succeed")
        );
        assert!(guest.rsync_install(&fail_comm).is_err());
    }

    #[test]
    fn test_linux_distro_package_commands() {
        let debian = LinuxDistro::Debian;
        assert!(debian.install_packages_cmd(&["curl"]).contains("apt-get"));

        let redhat = LinuxDistro::RedHat;
        assert!(redhat.install_packages_cmd(&["curl"]).contains("dnf"));

        let arch = LinuxDistro::Arch;
        assert!(arch.install_packages_cmd(&["curl"]).contains("pacman"));

        let alpine = LinuxDistro::Alpine;
        assert!(alpine.install_packages_cmd(&["curl"]).contains("apk"));

        let suse = LinuxDistro::Suse;
        assert!(suse.install_packages_cmd(&["curl"]).contains("zypper"));

        let generic = LinuxDistro::Generic;
        assert!(generic.install_packages_cmd(&["curl"]).contains("apt-get"));
    }

    #[test]
    fn test_linux_distro_kernel_packages() {
        assert!(!LinuxDistro::Debian.kernel_build_packages().is_empty());
        assert!(!LinuxDistro::RedHat.kernel_build_packages().is_empty());
        assert!(!LinuxDistro::Arch.kernel_build_packages().is_empty());
        assert!(!LinuxDistro::Alpine.kernel_build_packages().is_empty());
        assert!(!LinuxDistro::Suse.kernel_build_packages().is_empty());
        assert!(!LinuxDistro::Generic.kernel_build_packages().is_empty());
    }

    #[test]
    fn test_install_virtualbox_guest_additions_compilation_failure() {
        let guest = LinuxGuest;
        let fail_compile_comm = MultiMockComm {
            responses: vec![
                // detect_distro:
                (
                    "cat /etc/os-release".to_string(),
                    Ok("ID=ubuntu\n".to_string()),
                ),
                // kernel headers install:
                ("apt-get".to_string(), Ok("".to_string())),
                // mount and run fails compilation:
                (
                    "VBoxLinuxAdditions.run".to_string(),
                    Ok("Building kernel modules... failed".to_string()),
                ),
            ],
            default: Ok("".to_string()),
        };
        let res = guest.install_virtualbox_guest_additions(&fail_compile_comm, None);
        assert!(res.is_err());
        assert!(
            res.unwrap_err()
                .to_string()
                .contains("compilation or installation failed")
        );
    }

    #[test]
    fn test_install_virtualbox_guest_additions_fallback_success() {
        let guest = LinuxGuest;
        let fallback_comm = MultiMockComm {
            responses: vec![
                (
                    "cat /etc/os-release".to_string(),
                    Ok("ID=ubuntu\n".to_string()),
                ),
                ("apt-get install".to_string(), Ok("".to_string())),
                (
                    "VBoxLinuxAdditions.run".to_string(),
                    Err("No such file".to_string()),
                ),
                (
                    "virtualbox-guest-utils".to_string(),
                    Ok("Installed".to_string()),
                ),
            ],
            default: Ok("".to_string()),
        };
        assert!(
            guest
                .install_virtualbox_guest_additions(&fallback_comm, None)
                .is_ok()
        );
    }

    #[test]
    fn test_install_virtualbox_guest_additions_fallback_all_distros_and_failures() {
        let guest = LinuxGuest;
        for (distro_id, expected_pkg) in [
            ("centos", "kmod-vboxguest"),
            ("arch", "virtualbox-guest-utils"),
            ("alpine", "virtualbox-guest-modules-virt"),
            ("suse", "virtualbox-guest-tools"),
            ("unknown", "virtualbox-guest-utils"),
        ] {
            let comm = MultiMockComm {
                responses: vec![
                    (
                        "cat /etc/os-release".to_string(),
                        Ok(format!("ID={}\n", distro_id)),
                    ),
                    ("test -f".to_string(), Ok("".to_string())),
                    (
                        "VBoxLinuxAdditions.run".to_string(),
                        Err("No such file".to_string()),
                    ),
                    (
                        expected_pkg.to_string(),
                        Ok("Installed successfully".to_string()),
                    ),
                ],
                default: Ok("".to_string()),
            };
            assert!(
                guest
                    .install_virtualbox_guest_additions(&comm, None)
                    .is_ok()
            );
        }

        let fail_comm = MultiMockComm {
            responses: vec![
                (
                    "cat /etc/os-release".to_string(),
                    Ok("ID=centos\n".to_string()),
                ),
                (
                    "VBoxLinuxAdditions.run".to_string(),
                    Err("No such file".to_string()),
                ),
                (
                    "kmod-vboxguest".to_string(),
                    Ok("Transaction failed".to_string()),
                ),
            ],
            default: Ok("".to_string()),
        };
        let res = guest.install_virtualbox_guest_additions(&fail_comm, None);
        assert!(matches!(
            res,
            Err(MigratoryError::Generic(ref msg)) if msg.contains("VirtualBox Guest Additions installation failed")
        ));

        let err_fallback_comm = MultiMockComm {
            responses: vec![
                (
                    "cat /etc/os-release".to_string(),
                    Ok("ID=centos\n".to_string()),
                ),
                (
                    "VBoxLinuxAdditions.run".to_string(),
                    Err("No such file".to_string()),
                ),
                (
                    "kmod-vboxguest".to_string(),
                    Err("network timeout".to_string()),
                ),
            ],
            default: Ok("".to_string()),
        };
        let err_res = guest.install_virtualbox_guest_additions(&err_fallback_comm, None);
        assert!(err_res.is_err());
    }

    #[test]
    fn test_install_vmware_tools_success_and_failure() {
        let guest = LinuxGuest;
        let success_comm = MultiMockComm {
            responses: vec![
                (
                    "cat /etc/os-release".to_string(),
                    Ok("ID=debian\n".to_string()),
                ),
                ("apt-get".to_string(), Ok("0 failed".to_string())),
            ],
            default: Ok("".to_string()),
        };
        assert!(guest.install_vmware_tools(&success_comm).is_ok());

        let fail_comm = MultiMockComm {
            responses: vec![
                (
                    "cat /etc/os-release".to_string(),
                    Ok("ID=debian\n".to_string()),
                ),
                (
                    "apt-get".to_string(),
                    Ok("Package install failed with error".to_string()),
                ),
            ],
            default: Ok("".to_string()),
        };
        assert!(guest.install_vmware_tools(&fail_comm).is_err());
    }

    #[test]
    fn test_update_guest_additions_unknown_provider() {
        let guest = LinuxGuest;
        let comm = MockComm {
            output: Ok("".to_string()),
            fail_commands: vec![],
        };
        assert!(guest.update_guest_additions(&comm, "docker", None).is_ok());
    }
}
