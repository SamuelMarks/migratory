# Migratory CLI Usage

Below is the output of `migratory --help` showing all available commands and flags:

```text
[2m2026-08-27T04:12:25.908171Z[0m [32m INFO[0m [2mmigratory[0m[2m:[0m Migratory starting up
An open-source, 100% compatible replica of Vagrant.

Usage: migratory [OPTIONS] [COMMAND]

Commands:
  autocomplete   Manages autocomplete installation on host
  cap            Checks and executes capability
  cloud          Manages everything related to Vagrant Cloud
  docker-exec    Attach to an already-running docker container
  docker-logs    Outputs the logs from the Docker container
  docker-run     Run a one-off command in the context of a container
  init           Initializes a new Migratory environment by creating a Vagrantfile
  up             Starts and provisions the vagrant environment
  destroy        Stops and deletes all traces of the vagrant machine
  halt           Stops the vagrant machine
  suspend        Suspends the machine
  resume         Resumes a suspend machine
  reload         Restarts vagrant machine, loads new Vagrantfile configuration
  ssh            Connects to machine via SSH
  ssh-config     Outputs OpenSSH valid configuration to connect to the machine
  winrm          Executes commands on a machine via WinRM
  winrm-config   Outputs WinRM configuration to connect to the machine
  rdp            Generates an RDP file for Windows guests
  status         Outputs status of the vagrant machine
  global-status  Outputs status of all vagrant machines on this system
  port           Displays information about guest port bindings
  powershell     Connects to machine via powershell remoting
  provider       Show provider for this environment
  provision      Provisions the vagrant machine
  push           Deploys code in this environment to a configured destination
  rsync          Syncs rsync synced folders to remote machine
  rsync-auto     Syncs rsync synced folders automatically when files change
  upload         Upload to machine via communicator
  validate       Validates the Vagrantfile
  version        Prints current and latest Vagrant version
  package        Packages a running vagrant environment into a box
  box            Manages boxes: installation, removal, etc
  login          Authenticates with Vagrant Cloud
  mutate         Mutates a box
  plugin         Manages plugins
  snapshot       Manages snapshots
  list-commands  Outputs all available Vagrant subcommands, even non-primary ones
  help           Print this message or the help of the given subcommand(s)

Options:
      --color             Enable or disable color output
      --no-color          Disable color output (alias for --color=false)
      --debug             Enable debug output
      --machine-readable  Turn on machine readable output
      --timestamp         Enable timestamps on log output
      --debug-timestamp   Enable debug output with timestamps
      --no-tty            Enable non-interactive output
  -v, --version           Display Vagrant version
  -h, --help              Print help
```
