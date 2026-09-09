# Migratory CLI Usage Guide

`migratory` is an open-source, 100% compatible drop-in replacement for Vagrant. This document provides a comprehensive command-line reference, workflow examples, and flag specifications for all subcommands.

---

## Table of Contents

- [CLI Synopsis & Global Options](#cli-synopsis--global-options)
- [Machine Lifecycle](#machine-lifecycle)
  - [`init`](#init)
  - [`up`](#up)
  - [`halt`](#halt)
  - [`suspend`](#suspend)
  - [`resume`](#resume)
  - [`reload`](#reload)
  - [`destroy`](#destroy)
- [Inspection & Diagnostics](#inspection--diagnostics)
  - [`status`](#status)
  - [`global-status`](#global-status)
  - [`port`](#port)
  - [`validate`](#validate)
  - [`version`](#version)
- [Remote Access & Execution](#remote-access--execution)
  - [`ssh`](#ssh)
  - [`ssh-config`](#ssh-config)
  - [`winrm`](#winrm)
  - [`winrm-config`](#winrm-config)
  - [`powershell`](#powershell)
  - [`rdp`](#rdp)
  - [`upload`](#upload)
- [Box Management](#box-management)
  - [`box add`](#box-add)
  - [`box list`](#box-list)
  - [`box outdated`](#box-outdated)
  - [`box update`](#box-update)
  - [`box prune`](#box-prune)
  - [`box remove`](#box-remove)
  - [`box repackage`](#box-repackage)
  - [`mutate`](#mutate)
  - [`package`](#package)
- [Vagrant Cloud Integration](#vagrant-cloud-integration)
  - [`login`](#login)
  - [`cloud auth`](#cloud-auth)
  - [`cloud box`](#cloud-box)
  - [`cloud provider`](#cloud-provider)
  - [`cloud version`](#cloud-version)
  - [`cloud publish`](#cloud-publish)
  - [`cloud search`](#cloud-search)
- [Snapshots](#snapshots)
  - [`snapshot save`](#snapshot-save)
  - [`snapshot restore`](#snapshot-restore)
  - [`snapshot list`](#snapshot-list)
  - [`snapshot delete`](#snapshot-delete)
  - [`snapshot push` & `snapshot pop`](#snapshot-push--snapshot-pop)
- [Provisioning & Synchronization](#provisioning--synchronization)
  - [`provision`](#provision)
  - [`rsync`](#rsync)
  - [`rsync-auto`](#rsync-auto)
  - [`push`](#push)
- [Docker Engine Workflows](#docker-engine-workflows)
  - [`docker-run`](#docker-run)
  - [`docker-exec`](#docker-exec)
  - [`docker-logs`](#docker-logs)
- [System & Extensibility](#system--extensibility)
  - [`cap`](#cap)
  - [`provider`](#provider)
  - [`plugin`](#plugin)
  - [`autocomplete`](#autocomplete)
  - [`list-commands`](#list-commands)
- [Machine-Readable Output Mode](#machine-readable-output-mode)
- [Environment Variables](#environment-variables)

---

## CLI Synopsis & Global Options

```text
Usage: migratory [OPTIONS] [COMMAND]
```

### Global Options

These options can be passed to any `migratory` invocation:

| Option | Description |
| :--- | :--- |
| `--color` | Forces ANSI color output regardless of terminal detection. |
| `--no-color` | Disables all ANSI terminal colors and formatting. |
| `--debug` | Enables verbose debug logging across all internal subsystems. |
| `--machine-readable` | Emits structured CSV data streams suitable for script and IDE automation. |
| `--timestamp` | Prepends standard RFC3339 timestamps to log lines. |
| `--debug-timestamp` | Combines verbose `--debug` logging with high-resolution timestamps. |
| `--no-tty` | Disables interactive prompts and TTY detection for non-interactive runners. |
| `-v`, `--version` | Displays current version and build information. |
| `-h`, `--help` | Displays help information for the CLI or subcommand. |

---

## Machine Lifecycle

### `init`

Initializes a new development environment by generating a template `Vagrantfile` in the current directory.

```bash
migratory init [OPTIONS] [NAME] [URL]
```

* **Arguments:**
  * `[NAME]`: The name of the box to initialize (e.g. `ubuntu/jammy64`, `debian/bookworm64`).
  * `[URL]`: Optional direct URL or path to a `.box` archive.
* **Options:**
  * `-f`, `--force`: Overwrite any existing `Vagrantfile` in the target directory.
  * `-m`, `--minimal`: Generate a minimal `Vagrantfile` without comments.
  * `--template <PATH>`: Path to a custom template file to use instead of default.
* **Examples:**
  ```bash
  migratory init
  migratory init ubuntu/jammy64
  migratory init debian/bookworm64 --minimal
  migratory init custom-box https://example.com/boxes/custom.box -f
  ```

---

### `up`

Creates, starts, and provisions virtual machines defined in the `Vagrantfile`.

```bash
migratory up [OPTIONS] [NAME]
```

* **Arguments:**
  * `[NAME]`: Optional target machine name in multi-machine environments. If omitted, starts all machines.
* **Options:**
  * `--provider <PROVIDER>`: Target hypervisor provider (`virtualbox`, `qemu`, `vmware`, `hyperv`, `docker`).
  * `--provision`: Force provisioners to execute (default behavior on first boot).
  * `--no-provision`: Boot machines without running provisioners.
  * `--provision-with <LIST>`: Comma-separated list of provisioner types or names to run (e.g., `shell,ansible`).
  * `--destroy-on-error`: Destroy the VM if provisioning fails (default: true).
  * `--no-destroy-on-error`: Keep VM running if provisioning encounters an error.
  * `--parallel`: Spin up multi-machine environments concurrently.
  * `--no-parallel`: Enforce sequential machine boot.
  * `--install-provider`: Prompt to install hypervisor provider if missing.
* **Examples:**
  ```bash
  migratory up
  migratory up --provider qemu
  migratory up web --no-provision
  migratory up --provision-with shell
  ```

---

### `halt`

Gracefully shuts down running virtual machines.

```bash
migratory halt [OPTIONS] [NAME]
```

* **Arguments:**
  * `[NAME]`: Target machine name (defaults to all active machines).
* **Options:**
  * `-f`, `--force`: Force immediate power off instead of attempting a graceful guest OS shutdown.
* **Examples:**
  ```bash
  migratory halt
  migratory halt db
  migratory halt -f
  ```

---

### `suspend`

Suspends the current execution state and saves guest RAM to host disk.

```bash
migratory suspend [NAME]
```

* **Examples:**
  ```bash
  migratory suspend
  migratory suspend web
  ```

---

### `resume`

Resumes virtual machines from a previously suspended state.

```bash
migratory resume [OPTIONS] [NAME]
```

* **Options:**
  * `--provision`: Run provisioners after resuming.
  * `--no-provision`: Resume without running provisioners.
* **Examples:**
  ```bash
  migratory resume
  migratory resume web
  ```

---

### `reload`

Restarts virtual machines and reapplies updated `Vagrantfile` configuration settings (such as forwarded ports, synced folders, or memory limits).

```bash
migratory reload [OPTIONS] [NAME]
```

* **Options:**
  * `--provision`: Execute provisioners during restart.
  * `--no-provision`: Skip provisioners during restart.
  * `--provision-with <LIST>`: Comma-separated list of provisioners to execute.
  * `-f`, `--force`: Force shutdown without graceful guest OS coordination.
* **Examples:**
  ```bash
  migratory reload
  migratory reload --provision
  ```

---

### `destroy`

Stops and deletes all traces of the virtual machine, including associated virtual disk files and local `.vagrant/` tracking state.

```bash
migratory destroy [OPTIONS] [NAME]
```

* **Options:**
  * `-f`, `--force`: Bypass interactive confirmation prompt.
  * `--parallel`: Destroy multiple machines in parallel.
  * `--graceful`: Attempt graceful shutdown prior to deletion.
* **Examples:**
  ```bash
  migratory destroy
  migratory destroy -f
  migratory destroy web -f
  ```

---

## Inspection & Diagnostics

### `status`

Outputs the operational state (running, poweroff, suspended, not created) of machines in the active project.

```bash
migratory status [NAME]
```

* **Examples:**
  ```bash
  migratory status
  migratory status web
  ```

---

### `global-status`

Scans and reports the state of all Migratory and Vagrant environments across the host system.

```bash
migratory global-status [OPTIONS]
```

* **Options:**
  * `--prune`: Prune stale entries representing virtual machines that no longer exist on the hypervisor.
* **Examples:**
  ```bash
  migratory global-status
  migratory global-status --prune
  ```

---

### `port`

Displays active guest-to-host forwarded port mappings for the target machine.

```bash
migratory port [NAME]
```

* **Examples:**
  ```bash
  migratory port
  # Output:
  # The forwarded ports for the machine are listed below.
  # 22 (guest) => 2222 (host)
  # 80 (guest) => 8080 (host)
  ```

---

### `validate`

Parses and validates the project's `Vagrantfile` syntax, ensuring all configuration options, network declarations, and provider settings conform to expected schemas.

```bash
migratory validate [OPTIONS]
```

* **Options:**
  * `--ignore-provider`: Ignore validations specific to hypervisor providers.
* **Examples:**
  ```bash
  migratory validate
  ```

---

### `version`

Prints the current installed Migratory version, build architecture, and checks for available updates.

```bash
migratory version
```

---

## Remote Access & Execution

### `ssh`

Connects to the machine via an interactive secure shell (SSH) session, or executes an arbitrary one-off command.

```bash
migratory ssh [OPTIONS] [NAME] [-- COMMAND...]
```

* **Options:**
  * `-c`, `--command <CMD>`: Execute an inline command on the guest machine and exit.
  * `-p`, `--plain`: Connect without passing default Vagrant SSH authentication options.
  * `--extra-args <ARGS>`: Pass custom flags directly to the underlying OpenSSH binary.
* **Examples:**
  ```bash
  migratory ssh
  migratory ssh web
  migratory ssh -c "uptime"
  migratory ssh -- htop
  ```

---

### `ssh-config`

Generates an OpenSSH-compatible configuration block that can be appended to `~/.ssh/config` or used directly with tools like `ssh -F`.

```bash
migratory ssh-config [OPTIONS] [NAME]
```

* **Options:**
  * `--host <NAME>`: Custom name to assign to the OpenSSH `Host` block.
* **Examples:**
  ```bash
  migratory ssh-config
  migratory ssh-config --host my-vm >> ~/.ssh/config
  ```

---

### `winrm`

Executes commands or powershell scripts inside a Windows guest machine over WinRM (HTTP/HTTPS).

```bash
migratory winrm [OPTIONS] [NAME]
```

* **Options:**
  * `-c`, `--command <CMD>`: Command string to execute on the guest.
  * `-e`, `--elevated`: Execute command with elevated Windows administrator credentials.
* **Examples:**
  ```bash
  migratory winrm -c "Get-Service"
  migratory winrm -e -c "Restart-Service W3SVC"
  ```

---

### `winrm-config`

Outputs connection parameters (host, port, username, password/cert) for WinRM management clients.

```bash
migratory winrm-config [NAME]
```

---

### `powershell`

Opens an interactive PowerShell Remoting session into a target Windows virtual machine.

```bash
migratory powershell [NAME]
```

---

### `rdp`

Generates an `.rdp` remote desktop configuration file and launches the host OS native Remote Desktop client.

```bash
migratory rdp [NAME]
```

---

### `upload`

Uploads files or directories from the host machine into the guest environment using the active communicator (SSH or WinRM).

```bash
migratory upload [OPTIONS] <SOURCE> [DESTINATION] [NAME]
```

* **Examples:**
  ```bash
  migratory upload app.tar.gz /tmp/app.tar.gz
  migratory upload ./config /etc/myapp/ web
  ```

---

## Box Management

### `box add`

Downloads and registers a new machine image (`.box`) into the local box store (`~/.vagrant.d/boxes`).

```bash
migratory box add [OPTIONS] <NAME-OR-URL>
```

* **Options:**
  * `--name <NAME>`: Logical name for the box when downloading from a direct URL or local file path.
  * `--provider <PROVIDER>`: Provider type matching the box archive (e.g. `virtualbox`, `qemu`).
  * `--box-version <VERSION>`: Semantic version or version constraint (e.g. `~> 2.1.0`).
  * `--checksum <CHECKSUM>`: Expected cryptographic checksum for box integrity validation.
  * `--checksum-type <TYPE>`: Hash algorithm: `sha256`, `sha1`, or `md5`.
  * `-f`, `--force`: Overwrite existing box of the same name and version.
  * `--insecure`: Allow unverified SSL/TLS certificates during download.
  * `--cacert <PATH>`: Custom CA certificate for HTTPS validation.
* **Examples:**
  ```bash
  migratory box add ubuntu/jammy64
  migratory box add debian/bookworm64 --box-version "12.0.0"
  migratory box add custom-vm ./package.box --name myorg/custom-vm
  migratory box add https://example.com/boxes/fedora.box --name fedora --checksum-type sha256 --checksum <HASH>
  ```

---

### `box list`

Lists all locally installed boxes, including provider types and version tags.

```bash
migratory box list [OPTIONS]
```

* **Options:**
  * `-i`, `--box-info`: Display detailed manifest metadata for each installed box.
* **Examples:**
  ```bash
  migratory box list
  migratory box list -i
  ```

---

### `box outdated`

Checks the upstream Vagrant Cloud API to see if newer versions exist for installed boxes.

```bash
migratory box outdated [OPTIONS]
```

* **Options:**
  * `--global`: Check update status for all installed boxes, not just boxes in current project.
  * `--force`: Force check even if recently checked.
* **Examples:**
  ```bash
  migratory box outdated --global
  ```

---

### `box update`

Updates installed boxes to the latest upstream version available on Vagrant Cloud.

```bash
migratory box update [OPTIONS]
```

* **Options:**
  * `--box <NAME>`: Target specific box name to update.
  * `--provider <PROVIDER>`: Target specific provider.
* **Examples:**
  ```bash
  migratory box update
  migratory box update --box ubuntu/jammy64
  ```

---

### `box prune`

Removes older versions of installed boxes, keeping only the most recently active version to reclaim disk space.

```bash
migratory box prune [OPTIONS]
```

* **Options:**
  * `-f`, `--force`: Prune without prompting for confirmation.
  * `--name <NAME>`: Only prune versions of a specific box.
  * `--keep-active-boxes`: Preserve boxes currently tied to active machines.
* **Examples:**
  ```bash
  migratory box prune -f
  ```

---

### `box remove`

Deletes a box from the local registry.

```bash
migratory box remove [OPTIONS] <NAME>
```

* **Options:**
  * `--provider <PROVIDER>`: Provider type of the box to remove.
  * `--box-version <VERSION>`: Version number to remove.
  * `-f`, `--force`: Force removal even if currently referenced by active machines.
* **Examples:**
  ```bash
  migratory box remove ubuntu/jammy64 --provider virtualbox
  migratory box remove ubuntu/jammy64 --box-version 20240101.0.0
  ```

---

### `box repackage`

Repackages an installed local box back into a redistributable `.box` archive file.

```bash
migratory box repackage <NAME> <PROVIDER> <VERSION>
```

---

### `mutate`

Converts a `.box` file from one hypervisor format to another (for example, converting a VirtualBox box into a QEMU/libvirt box).

```bash
migratory mutate [OPTIONS] <BOX-NAME-OR-PATH> <TARGET-PROVIDER>
```

* **Options:**
  * `--input-provider <PROVIDER>`: Specify source provider if ambiguous.
* **Examples:**
  ```bash
  migratory mutate ubuntu/jammy64 qemu
  ```

---

### `package`

Packages a running virtual machine into a reusable, distributable `.box` archive.

```bash
migratory package [OPTIONS] [NAME]
```

* **Options:**
  * `--output <PATH>`: Destination filename for the resulting `.box` archive.
  * `--include <FILES>`: Comma-separated list of additional files to package inside the box archive.
  * `--vagrantfile <PATH>`: Include a custom nested `Vagrantfile` in the package.
* **Examples:**
  ```bash
  migratory package --output my-custom-box.box
  ```

---

## Vagrant Cloud Integration

### `login`

Authenticates with Vagrant Cloud via interactive prompts or environment variables.

```bash
migratory login [OPTIONS]
```

* **Options:**
  * `-u`, `--username <USER>`: Vagrant Cloud username.
  * `-p`, `--password <PASS>`: Vagrant Cloud account password.
  * `-t`, `--token <TOKEN>`: Direct API access token.
  * `-c`, `--check`: Verify current authentication status.
  * `-k`, `--logout`: Revoke active session token.
* **Examples:**
  ```bash
  migratory login
  migratory login --token $VAGRANT_CLOUD_TOKEN
  migratory login --check
  migratory login --logout
  ```

---

### `cloud auth`

Subcommands for managing Vagrant Cloud authentication:

```bash
migratory cloud auth login
migratory cloud auth logout
migratory cloud auth whoami
```

---

### `cloud box`

Manages box records registered under your Vagrant Cloud account:

```bash
migratory cloud box create <ORGANIZATION/NAME>
migratory cloud box show <ORGANIZATION/NAME>
migratory cloud box update <ORGANIZATION/NAME> --description "Updated description"
migratory cloud box delete <ORGANIZATION/NAME>
```

---

### `cloud provider`

Manages hypervisor provider binaries associated with box versions on Vagrant Cloud:

```bash
migratory cloud provider create <ORGANIZATION/NAME> <VERSION> <PROVIDER>
migratory cloud provider upload <ORGANIZATION/NAME> <VERSION> <PROVIDER> <PATH>
migratory cloud provider update <ORGANIZATION/NAME> <VERSION> <PROVIDER>
migratory cloud provider delete <ORGANIZATION/NAME> <VERSION> <PROVIDER>
```

---

### `cloud version`

Manages semantic versions of published boxes on Vagrant Cloud:

```bash
migratory cloud version create <ORGANIZATION/NAME> <VERSION>
migratory cloud version release <ORGANIZATION/NAME> <VERSION>
migratory cloud version revoke <ORGANIZATION/NAME> <VERSION>
migratory cloud version delete <ORGANIZATION/NAME> <VERSION>
```

---

### `cloud publish`

High-level command to create, upload, and release a new box version to Vagrant Cloud in a single step:

```bash
migratory cloud publish [OPTIONS] <ORGANIZATION/NAME> <VERSION> <PROVIDER> <BOX-PATH>
```

---

### `cloud search`

Searches the public Vagrant Cloud registry for available boxes:

```bash
migratory cloud search [OPTIONS] <QUERY>
```

* **Options:**
  * `--provider <PROVIDER>`: Filter results by hypervisor provider.
  * `--sort-by <FIELD>`: Sort by `downloads`, `created`, or `updated`.
  * `--page <N>`: Results page index.
* **Examples:**
  ```bash
  migratory cloud search ubuntu
  migratory cloud search debian --provider qemu
  ```

---

## Snapshots

Manage hypervisor-level snapshot points for rapid testing and rollbacks:

### `snapshot save`

Takes a snapshot of the current state of a machine.

```bash
migratory snapshot save [OPTIONS] [NAME] <SNAPSHOT-NAME>
```

* **Examples:**
  ```bash
  migratory snapshot save clean-install
  migratory snapshot save web pre-migration
  ```

---

### `snapshot restore`

Restores the machine to a saved snapshot.

```bash
migratory snapshot restore [OPTIONS] [NAME] <SNAPSHOT-NAME>
```

* **Options:**
  * `--provision`: Run provisioners after restoring.
  * `--no-provision`: Skip provisioners after restoring.
* **Examples:**
  ```bash
  migratory snapshot restore clean-install
  ```

---

### `snapshot list`

Lists all saved snapshots for the machine.

```bash
migratory snapshot list [NAME]
```

---

### `snapshot delete`

Deletes a saved snapshot.

```bash
migratory snapshot delete [NAME] <SNAPSHOT-NAME>
```

---

### `snapshot push` & `snapshot pop`

Convenience stack-based snapshot shortcuts:

* `migratory snapshot push`: Automatically saves a snapshot onto an internal snapshot stack.
* `migratory snapshot pop`: Restores and deletes the most recent snapshot from the stack.

---

## Provisioning & Synchronization

### `provision`

Executes provisioners against an already-running virtual machine without rebooting.

```bash
migratory provision [OPTIONS] [NAME]
```

* **Options:**
  * `--provision-with <LIST>`: Comma-separated list of provisioner types or names to run (e.g. `shell,ansible`).
* **Examples:**
  ```bash
  migratory provision
  migratory provision web --provision-with shell
  ```

---

### `rsync`

Triggers an immediate, one-time synchronization pass for all configured `rsync` synced folders.

```bash
migratory rsync [NAME]
```

---

### `rsync-auto`

Starts a persistent, low-latency filesystem watcher (using native OS notification engines: `inotify` on Linux, `FSEvents` on macOS, `ReadDirectoryChangesW` on Windows) that automatically replicates file changes into the guest VM in real time.

```bash
migratory rsync-auto [OPTIONS] [NAME]
```

* **Options:**
  * `--poll`: Fall back to periodic disk polling instead of OS filesystem events.
* **Examples:**
  ```bash
  migratory rsync-auto
  ```

---

### `push`

Deploys code from the local project environment to a remote target destination defined in the `Vagrantfile` configuration.

```bash
migratory push [STRATEGY]
```

---

## Docker Engine Workflows

When utilizing the Docker provider or working with containerized machines:

### `docker-run`

Runs an ephemeral one-off command in the context of a configured container service.

```bash
migratory docker-run [NAME] [-- COMMAND...]
```

---

### `docker-exec`

Attaches to an already-running Docker provider container and executes commands.

```bash
migratory docker-exec [OPTIONS] [NAME] [-- COMMAND...]
```

* **Options:**
  * `-t`, `--tty`: Allocate a pseudo-TTY for interactive terminal applications.
  * `-i`, `--interactive`: Keep standard input open.

---

### `docker-logs`

Follows and streams output logs from a running Docker provider container.

```bash
migratory docker-logs [OPTIONS] [NAME]
```

* **Options:**
  * `-f`, `--follow`: Stream and follow logs continuously.

---

## System & Extensibility

### `cap`

Checks for or executes internal guest, host, or provider capabilities.

```bash
migratory cap [OPTIONS] <TYPE> <NAME>
```

---

### `provider`

Displays the active hypervisor provider assigned to the current environment.

```bash
migratory provider
```

---

### `plugin`

Manages Migratory plugins and runtime extensions:

```bash
migratory plugin install <NAME>
migratory plugin list
migratory plugin update [NAME]
migratory plugin uninstall <NAME>
migratory plugin license <NAME> <LICENSE-FILE>
```

---

### `autocomplete`

Installs shell completion scripts into your user profile for seamless tab-completion of commands, subcommands, and flags.

```bash
migratory autocomplete install [OPTIONS]
```

* **Options:**
  * `--shell <SHELL>`: Target shell type: `bash`, `zsh`, `fish`, or `powershell`.
* **Examples:**
  ```bash
  migratory autocomplete install --shell zsh
  migratory autocomplete install --shell bash
  ```

---

### `list-commands`

Outputs all registered Migratory commands, including both primary commands and secondary subcommands.

```bash
migratory list-commands
```

---

## Machine-Readable Output Mode

Passing `--machine-readable` instructs Migratory to emit structured CSV lines to `stdout`. This format is 100% compatible with existing Vagrant IDE extensions, orchestration scripts, and CI runners.

### CSV Format Schema

```text
<timestamp>,<target-machine>,<type>,<data...>
```

* **`timestamp`**: Epoch time in seconds when the event occurred.
* **`target-machine`**: Name of the target machine (or blank for global messages).
* **`type`**: The event category or metadata key (e.g., `state`, `provider-name`, `ui`).
* **`data...`**: Zero or more comma-separated values associated with the event.

### Example Stream

```bash
$ migratory status --machine-readable
1725840000,default,metadata,provider,virtualbox
1725840000,default,provider-name,virtualbox
1725840000,default,state,running
1725840000,default,state-human-short,running
1725840000,default,state-human-long,The VM is running. To stop this machine, you can run `migratory halt`.
```

---

## Environment Variables

| Variable | Description | Default |
| :--- | :--- | :--- |
| `VAGRANT_HOME` | Directory where boxes, global state, and data are stored | `~/.vagrant.d` |
| `VAGRANT_CWD` | Working directory used to locate the `Vagrantfile` | Current working directory |
| `VAGRANT_VAGRANTFILE` | Explicit filename of the `Vagrantfile` to evaluate | `Vagrantfile` |
| `VAGRANT_DOTFILE_PATH`| Location of the local environment state directory | `.vagrant` |
| `VAGRANT_DEFAULT_PROVIDER` | Hypervisor provider to select when not specified | Automatically detected |
| `VAGRANT_CLOUD_TOKEN` | Authentication token for Vagrant Cloud API | Loaded from `~/.vagrant.d/data/vagrant_login_token` |
| `VAGRANT_CLOUD_URL` | Base endpoint URL for Vagrant Cloud | `https://app.vagrantup.com/api/v2` |
| `VAGRANT_CHECKPOINT_DISABLE` | When set, disables remote version update checks | Unset |
| `VAGRANT_VBOX_LINKED_CLONE` | When set to `true`, enables linked clones on VirtualBox | `false` |
| `VAGRANT_LIBVIRT_LINKED_CLONE` | When set to `true`, enables linked clones on libvirt/QEMU | `false` |
| `VAGRANT_WINRM_PASSWORD` | Fallback password for WinRM authentication | `vagrant` |
| `VAGRANT_SMB_USERNAME` | Fallback username for SMB synced folder mounts | `vagrant` |
| `VAGRANT_SMB_PASSWORD` | Fallback password for SMB synced folder mounts | `vagrant` |
| `MIGRATORY_FORCE_PURE_RUST` | When set, disables the external Ruby fallback evaluator | Unset |
