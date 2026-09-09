use super::{
    EnvironmentConfig, MachineConfig, NetworkConfig, ProviderConfig, ProvisionerConfig,
    SyncedFolderConfig,
};
use crate::error::MigratoryError;
use std::process::Command;

trait ResultExt<T> {
    fn wrap_err(self, msg: &str) -> Result<T, MigratoryError>;
}

impl<T, E: std::fmt::Display> ResultExt<T> for Result<T, E> {
    #[coverage(off)]
    fn wrap_err(self, msg: &str) -> Result<T, MigratoryError> {
        self.map_err(|e| MigratoryError::Generic(format!("{}: {}", msg, e)))
    }
}

/// Initializes the embedded Ruby VM.
///
/// In this implementation, we rely on the system Ruby interpreter, so this is a no-op.
pub fn init_ruby_vm() {
    // No-op
}

pub use super::in_process::evaluate_in_process;

const RUBY_PARSER_SCRIPT: &str = r#"
require 'fileutils'
require 'socket'
require 'pathname'
require 'yaml'
require 'json'

class BasicMock
  def method_missing(m, *args, &block)
    if block_given?
      yield BasicMock.new
    end
    BasicMock.new
  end

  def respond_to_missing?(method_name, include_private = false)
    true
  end
end

class Vagrant1ConfigMock < BasicMock
  def initialize(v2_config)
    @v2 = v2_config
  end

  def vm
    @vm ||= Vagrant1VMMock.new(@v2.vm)
  end

  def package
    @package ||= BasicMock.new
  end

  def method_missing(m, *args, &block)
    @v2.send(m, *args, &block)
  end
end

class Vagrant1VMMock < BasicMock
  def initialize(v2_vm)
    @v2_vm = v2_vm
  end

  def forward_port(guest, host, **options)
    opts = options.dup
    opts[:guest] = guest
    opts[:host] = host
    @v2_vm.network("forwarded_port", **opts)
  end

  def share_folder(name, guest_path, host_path, **options)
    opts = options.dup
    opts[:name] = name
    @v2_vm.synced_folder(host_path, guest_path, **opts)
  end

  def method_missing(m, *args, &block)
    @v2_vm.send(m, *args, &block)
  end
end

class VagrantConfigMock < BasicMock
  class VMMock < BasicMock
  attr_accessor :box_name, :box_version, :box_url, :box_check_update, :box_download_checksum, :box_download_checksum_type, :box_download_client_cert, :box_download_ca_cert, :box_download_insecure, :hostname, :post_up_message, :boot_timeout, :communicator, :graceful_halt_timeout, :networks, :providers, :provisioners, :synced_folders, :guest, :usable_port_range, :allowed_synced_folder_types, :disks, :depends_on

  def initialize
    @networks = []
    @providers = []
    @provisioners = []
    @synced_folders = []
    @usable_port_range = [2200, 2250]
    @allowed_synced_folder_types = nil
    @disks = []
    @depends_on = []
  end

  def box=(name)
    @box_name = name
  end

  def box_version=(v)
    @box_version = v
  end

  def box_url=(v)
    @box_url = v
  end

  def box_check_update=(v)
    @box_check_update = v
  end

  def box_download_checksum=(v)
    @box_download_checksum = v
  end

  def box_download_checksum_type=(v)
    @box_download_checksum_type = v
  end

  def box_download_client_cert=(v)
    @box_download_client_cert = v
  end

  def box_download_ca_cert=(v)
    @box_download_ca_cert = v
  end

  def box_download_insecure=(v)
    @box_download_insecure = v
  end

  def boot_timeout=(v)
    @boot_timeout = v
  end

  def communicator=(v)
    @communicator = v.to_s
  end

  def guest=(g)
    @guest = g.to_s
  end

  def graceful_halt_timeout=(v)
    @graceful_halt_timeout = v
  end

  def hostname=(name)
    @hostname = name
  end

  def post_up_message=(msg)
    @post_up_message = msg
  end

  def disk(type = :disk, **options)
    opts = options.dup
    opts[:disk_type] = type.to_s
    @disks << opts
  end

  # Vagrant 1 legacy compatibility
  def forward_port(guest, host, **options)
    opts = options.dup
    opts[:guest] = guest
    opts[:host] = host
    @networks << { "type" => "forwarded_port", "options" => opts }
  end

  # Vagrant 1 legacy compatibility
  def share_folder(name, guest_path, host_path, **options)
    opts = options.dup
    opts[:name] = name
    @synced_folders << { "host_path" => host_path, "guest_path" => guest_path, "options" => opts }
  end

  def network(type, **options)
    @networks << { "type" => type, "options" => options }
  end

  def synced_folder(host_path, guest_path, **options)
    @synced_folders << { "host_path" => host_path, "guest_path" => guest_path, "options" => options }
  end

  def provider(name, &block)
    provider_config = { "name" => name.to_s, "options" => {} }
    if block_given?
      mock_provider = BasicMock.new
      mock_provider.instance_variable_set(:@options, {})
      def mock_provider.method_missing(m, *args, &block)
        if m.to_s.end_with?('=')
          @options[m.to_s.chomp('=')] = args.first
        else
          super
        end
      end
      def mock_provider.customize(*args)
        (@options["customize"] ||= []) << args
      end
      def mock_provider.get_options
        @options
      end

      override_config = VagrantConfigMock.new
      if block.arity == 1 || block.arity == -1
        block.call(mock_provider)
      else
        block.call(mock_provider, override_config)
      end
      provider_config["options"] = mock_provider.get_options

      # Full merge of provider overrides into the VM state
      if override_config.vm.box_name
        @box_name = override_config.vm.box_name
      end
      if override_config.vm.box_version
        @box_version = override_config.vm.box_version
      end
      if override_config.vm.box_url
        @box_url = override_config.vm.box_url
      end
      if override_config.vm.box_check_update
        @box_check_update = override_config.vm.box_check_update
      end
      if override_config.vm.box_download_checksum
        @box_download_checksum = override_config.vm.box_download_checksum
      end
      if override_config.vm.box_download_checksum_type
        @box_download_checksum_type = override_config.vm.box_download_checksum_type
      end
      if override_config.vm.boot_timeout
        @boot_timeout = override_config.vm.boot_timeout
      end
      if override_config.vm.communicator
        @communicator = override_config.vm.communicator
      end
      if override_config.vm.guest
        @guest = override_config.vm.guest
      end
      if override_config.vm.graceful_halt_timeout
        @graceful_halt_timeout = override_config.vm.graceful_halt_timeout
      end
      if override_config.vm.hostname
        @hostname = override_config.vm.hostname
      end
      if override_config.vm.usable_port_range
        @usable_port_range = override_config.vm.usable_port_range
      end
      if override_config.vm.allowed_synced_folder_types
        @allowed_synced_folder_types = override_config.vm.allowed_synced_folder_types
      end
      @disks.concat(override_config.vm.disks)
      @networks.concat(override_config.vm.networks)
      @provisioners.concat(override_config.vm.provisioners)
      @synced_folders.concat(override_config.vm.synced_folders)
    end
    @providers << provider_config
  end
    
    def provision(name, **options, &block)
      p_type = options[:type] || name
      p_id = options[:type] ? name : nil
      p_run = options[:run]
      @provisioners << { "name" => p_type, "id" => p_id, "run" => p_run, "options" => options }
    end
  end

  class TriggerMock < BasicMock
    attr_accessor :triggers
    def initialize
      @triggers = []
    end
    def before(*actions, **options, &block)
      opts = options.dup
      if block_given?
        opts[:ruby_block] = true
        begin
          block.call(self)
        rescue => e
        end
      end
      if opts[:run].respond_to?(:call)
        opts[:ruby_block] = true
        begin
          opts[:run].call(self)
        rescue => e
        end
        opts.delete(:run)
      end
      @triggers << { "stage" => "before", "actions" => actions.map(&:to_s), "options" => opts }
    end
    def after(*actions, **options, &block)
      opts = options.dup
      if block_given?
        opts[:ruby_block] = true
        begin
          block.call(self)
        rescue => e
        end
      end
      if opts[:run].respond_to?(:call)
        opts[:ruby_block] = true
        begin
          opts[:run].call(self)
        rescue => e
        end
        opts.delete(:run)
      end
      @triggers << { "stage" => "after", "actions" => actions.map(&:to_s), "options" => opts }
    end
  end

  attr_accessor :vm, :ssh, :winrm, :vagrant, :trigger, :machines

  class SshMock < BasicMock
    attr_accessor :username, :password, :host, :port, :guest_port, :private_key_path,
                  :insert_key, :forward_agent, :forward_x11, :forward_env, :proxy_command,
                  :extra_args, :pty, :keep_alive, :shell, :export_command_template,
                  :connect_timeout, :timeout, :verify_host_key, :keys_only

    def initialize
      @forward_env = []
      @pty = false
      @keep_alive = true
      @verify_host_key = false
      @keys_only = true
    end
  end
  
  class WinrmMock < BasicMock
    attr_accessor :username, :password, :host, :port, :guest_port, :ssl, :transport,
                  :timeout, :basic_auth_only, :ssl_peer_verification,
                  :retry_limit, :retry_delay, :execution_time_limit
  end

  class VagrantMock < BasicMock
    attr_accessor :host, :plugins, :sensitive

    def initialize
      @plugins = []
      @sensitive = []
    end
    
    def plugin(name, **opts)
      @plugins << name
    end

    def sensitive=(val)
      @sensitive = val.is_a?(Array) ? val : [val.to_s]
    end
  end

  def initialize
    @vm = VMMock.new
    @ssh = SshMock.new
    @winrm = WinrmMock.new
    @vagrant = VagrantMock.new
    @trigger = TriggerMock.new
    @machines = {}
    
    mock = self
    @vm.define_singleton_method(:define) do |name, **options, &block|
      machine_config = VagrantConfigMock.new
      machine_config.instance_variable_set(:@primary, options[:primary] || false)
      machine_config.instance_variable_set(:@autostart, options.key?(:autostart) ? options[:autostart] : true)
      block.call(machine_config) if block
      mock.machines[name.to_s] = machine_config
    end
  end

  def merge_ssh(global, local)
    extra = local.extra_args || global.extra_args
    extra_val = extra.is_a?(Array) ? extra : (extra.nil? ? nil : [extra.to_s])
    f_env = (global.forward_env || []) + (local.forward_env || [])
    {
      "username" => local.username.nil? ? global.username : local.username,
      "password" => local.password.nil? ? global.password : local.password,
      "host" => local.host.nil? ? global.host : local.host,
      "port" => local.port.nil? ? global.port : local.port,
      "guest_port" => local.guest_port.nil? ? global.guest_port : local.guest_port,
      "private_key_path" => local.private_key_path.nil? ? global.private_key_path : local.private_key_path,
      "insert_key" => local.insert_key.nil? ? global.insert_key : local.insert_key,
      "forward_agent" => local.forward_agent.nil? ? global.forward_agent : local.forward_agent,
      "forward_x11" => local.forward_x11.nil? ? global.forward_x11 : local.forward_x11,
      "forward_env" => f_env.uniq,
      "proxy_command" => local.proxy_command.nil? ? global.proxy_command : local.proxy_command,
      "extra_args" => extra_val,
      "pty" => local.pty.nil? ? global.pty : local.pty,
      "keep_alive" => local.keep_alive.nil? ? global.keep_alive : local.keep_alive,
      "shell" => local.shell || global.shell,
      "export_command_template" => local.export_command_template || global.export_command_template,
      "connect_timeout" => local.connect_timeout || global.connect_timeout,
      "timeout" => local.timeout || global.timeout,
      "verify_host_key" => local.verify_host_key.nil? ? global.verify_host_key : local.verify_host_key,
      "keys_only" => local.keys_only.nil? ? global.keys_only : local.keys_only
    }
  end

  def merge_winrm(global, local)
    {
      "username" => local.username.nil? ? global.username : local.username,
      "password" => local.password.nil? ? global.password : local.password,
      "host" => local.host.nil? ? global.host : local.host,
      "port" => local.port.nil? ? global.port : local.port,
      "guest_port" => local.guest_port.nil? ? global.guest_port : local.guest_port,
      "ssl" => local.ssl.nil? ? global.ssl : local.ssl,
      "transport" => local.transport.nil? ? global.transport : local.transport,
      "timeout" => local.timeout.nil? ? global.timeout : local.timeout,
      "basic_auth_only" => local.basic_auth_only.nil? ? global.basic_auth_only : local.basic_auth_only,
      "ssl_peer_verification" => local.ssl_peer_verification.nil? ? global.ssl_peer_verification : local.ssl_peer_verification,
      "retry_limit" => local.retry_limit || global.retry_limit,
      "retry_delay" => local.retry_delay || global.retry_delay,
      "execution_time_limit" => local.execution_time_limit || global.execution_time_limit
    }
  end
  
  def merge_vagrant(global, local)
    {
      "host" => local.host.nil? ? global.host : local.host,
      "plugins" => (global.plugins + local.plugins).uniq,
      "sensitive" => (global.sensitive + local.sensitive).uniq
    }
  end
  
  def to_hash
    machine_hash = {}
    if @machines.empty?
      u_range = @vm.usable_port_range
      u_range_arr = u_range.is_a?(Range) ? [u_range.begin, u_range.end] : u_range
      machine_hash["default"] = {
        "name" => "default",
        "primary" => false,
        "autostart" => true,
        "triggers" => @trigger.triggers,
        "vm" => {
          "box_name" => @vm.box_name,
          "box_version" => @vm.box_version,
          "box_url" => @vm.box_url,
          "box_check_update" => @vm.box_check_update,
          "box_download_checksum" => @vm.box_download_checksum,
          "box_download_checksum_type" => @vm.box_download_checksum_type,
          "box_download_client_cert" => @vm.box_download_client_cert,
          "box_download_ca_cert" => @vm.box_download_ca_cert,
          "box_download_insecure" => @vm.box_download_insecure,
          "hostname" => @vm.hostname,
          "guest" => @vm.guest,
          "post_up_message" => @vm.post_up_message,
          "boot_timeout" => @vm.boot_timeout,
          "communicator" => @vm.communicator,
          "graceful_halt_timeout" => @vm.graceful_halt_timeout,
          "usable_port_range" => u_range_arr,
          "allowed_synced_folder_types" => @vm.allowed_synced_folder_types,
          "disks" => @vm.disks,
          "depends_on" => @vm.depends_on || [],
          "networks" => @vm.networks,
          "providers" => @vm.providers,
          "provisioners" => @vm.provisioners,
          "synced_folders" => @vm.synced_folders
        },
        "ssh" => merge_ssh(@ssh, @ssh),
        "winrm" => merge_winrm(@winrm, @winrm),
        "vagrant" => merge_vagrant(@vagrant, @vagrant)
      }
    else
      @machines.each do |name, config|
        u_range = config.vm.usable_port_range || @vm.usable_port_range
        u_range_arr = u_range.is_a?(Range) ? [u_range.begin, u_range.end] : u_range
        machine_hash[name] = {
          "name" => name,
          "primary" => config.instance_variable_get(:@primary) || false,
          "autostart" => config.instance_variable_get(:@autostart).nil? ? true : config.instance_variable_get(:@autostart),
          "triggers" => @trigger.triggers + config.trigger.triggers,
          "vm" => {
            "box_name" => config.vm.box_name || @vm.box_name,
            "box_version" => config.vm.box_version || @vm.box_version,
            "box_url" => config.vm.box_url || @vm.box_url,
            "box_check_update" => config.vm.box_check_update || @vm.box_check_update,
            "box_download_checksum" => config.vm.box_download_checksum || @vm.box_download_checksum,
            "box_download_checksum_type" => config.vm.box_download_checksum_type || @vm.box_download_checksum_type,
            "box_download_client_cert" => config.vm.box_download_client_cert || @vm.box_download_client_cert,
            "box_download_ca_cert" => config.vm.box_download_ca_cert || @vm.box_download_ca_cert,
            "box_download_insecure" => config.vm.box_download_insecure.nil? ? @vm.box_download_insecure : config.vm.box_download_insecure,
            "hostname" => config.vm.hostname || @vm.hostname,
            "guest" => config.vm.guest || @vm.guest,
            "post_up_message" => config.vm.post_up_message || @vm.post_up_message,
            "boot_timeout" => config.vm.boot_timeout || @vm.boot_timeout,
            "communicator" => config.vm.communicator || @vm.communicator,
            "graceful_halt_timeout" => config.vm.graceful_halt_timeout || @vm.graceful_halt_timeout,
            "usable_port_range" => u_range_arr,
            "allowed_synced_folder_types" => config.vm.allowed_synced_folder_types || @vm.allowed_synced_folder_types,
            "disks" => @vm.disks + config.vm.disks,
            "depends_on" => config.vm.depends_on || @vm.depends_on || [],
            "networks" => @vm.networks + config.vm.networks,
            "providers" => @vm.providers + config.vm.providers,
            "provisioners" => @vm.provisioners + config.vm.provisioners,
            "synced_folders" => @vm.synced_folders + config.vm.synced_folders
          },
          "ssh" => merge_ssh(@ssh, config.ssh),
          "winrm" => merge_winrm(@winrm, config.winrm),
          "vagrant" => merge_vagrant(@vagrant, config.vagrant)
        }
      end
    end
    { "machines" => machine_hash }
  end
end

module Vagrant
  MIGRATORY_VERSION = "2.3.4"

  def self.cmp_versions(v1, v2)
    p1 = v1.to_s.split('.').map(&:to_i)
    p2 = v2.to_s.split('.').map(&:to_i)
    len = [p1.length, p2.length].max
    p1 += [0] * (len - p1.length)
    p2 += [0] * (len - p2.length)
    p1 <=> p2
  end

  def self.configure(version = "2", &block)
    @config ||= VagrantConfigMock.new
    if version.to_s == "1"
      v1 = Vagrant1ConfigMock.new(@config)
      block.call(v1) if block
    else
      block.call(@config) if block
    end
  end

  def self.get_config
    @config ||= VagrantConfigMock.new
  end

  def self.require_version(*args)
    args.each do |req|
      next unless req.is_a?(String)
      parts = req.strip.split(/\s+/, 2)
      op = parts.length == 2 ? parts[0] : "="
      target_ver = parts.length == 2 ? parts[1] : parts[0]
      cmp = cmp_versions(MIGRATORY_VERSION, target_ver)
      satisfied = case op
      when ">=" then cmp >= 0
      when "<=" then cmp <= 0
      when ">"  then cmp > 0
      when "<"  then cmp < 0
      when "="  then cmp == 0
      when "~>" then
        p = target_ver.split('.').map(&:to_i)
        if p.length >= 2
          upper_p = p[0..-2]
          upper_p[-1] += 1
          upper_ver = upper_p.join('.')
          cmp >= 0 && cmp_versions(MIGRATORY_VERSION, upper_ver) < 0
        else
          cmp >= 0
        end
      else
        true
      end
      unless satisfied
        raise "Vagrant version requirement '#{req}' failed (running #{MIGRATORY_VERSION})."
      end
    end
  end

  def self.require_plugin(*args)
  end

  def self.has_plugin?(*args)
    true
  end
end

bind = binding
ARGV.each do |target_file|
  next unless File.file?(target_file)
  begin
    eval(File.read(target_file), bind, target_file)
  rescue => e
    bt = e.backtrace ? e.backtrace.join("\n") : ""
    STDERR.puts "Error evaluating Vagrantfile (#{target_file}): #{e.class}: #{e.message}\n#{bt}"
    exit 1
  end
end

require 'json'
puts JSON.generate(Vagrant.get_config.to_hash)
"#;

/// Evaluates one or more Vagrantfiles in sequence, returning the aggregated environment config.
///
/// # Arguments
///
/// * `paths` - Slice of paths to Vagrantfiles in evaluation order.
///
/// # Returns
///
/// Returns an `EnvironmentConfig` loaded from the files.
///
/// # Errors
///
/// Returns a `MigratoryError` if evaluation or JSON parsing fails.
pub fn parse_vagrantfiles<P: AsRef<std::path::Path>>(
    paths: &[P],
) -> Result<EnvironmentConfig, MigratoryError> {
    let path_refs: Vec<&std::path::Path> = paths.iter().map(|p| p.as_ref()).collect();
    parse_vagrantfiles_inner(&path_refs)
}

fn parse_vagrantfiles_inner(
    paths: &[&std::path::Path],
) -> Result<EnvironmentConfig, MigratoryError> {
    if paths.is_empty() {
        return Err(MigratoryError::NotFound("Vagrantfile".to_string()));
    }

    for p in paths {
        if !p.exists() {
            return Err(MigratoryError::NotFound(p.to_string_lossy().to_string()));
        }
    }

    let mut cmd = Command::new("ruby");
    cmd.arg("-e").arg(RUBY_PARSER_SCRIPT);
    for p in paths {
        cmd.arg(p);
    }

    let is_forced_pure_rust = std::env::var("MIGRATORY_FORCE_PURE_RUST").is_ok();
    let output_result = if std::env::var("MIGRATORY_TEST_MOCK_RUBY_ERROR").is_ok() {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Mock permission denied executing ruby",
        ))
    } else if std::env::var("MIGRATORY_TEST_MOCK_RUBY_BAD_JSON").is_ok() {
        Ok(std::process::Output {
            status: std::process::ExitStatus::default(),
            stdout: b"not json".to_vec(),
            stderr: Vec::new(),
        })
    } else if is_forced_pure_rust {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Forced pure Rust evaluation",
        ))
    } else {
        cmd.output()
    };

    match output_result {
        Ok(output) => {
            if !output.status.success() {
                let err_msg = String::from_utf8_lossy(&output.stderr);
                return Err(MigratoryError::Generic(format!(
                    "Vagrantfile evaluation failed: {}",
                    err_msg
                )));
            }

            let json_str = String::from_utf8_lossy(&output.stdout);

            // Parse the JSON into our internal structs
            let parsed_json: serde_json::Value =
                serde_json::from_str(&json_str).wrap_err("Failed to parse JSON")?;

            Ok(parse_json_config(&parsed_json))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let mut aggregated = EnvironmentConfig::default();
            for p in paths {
                let content = std::fs::read_to_string(p).map_err(MigratoryError::Io)?;
                let file_config = evaluate_in_process(&content)?;
                aggregated = crate::config::merge_environment_configs(&aggregated, &file_config);
            }
            Ok(aggregated)
        }
        Err(err) => Err(MigratoryError::Generic(format!(
            "Failed to execute ruby: {}",
            err
        ))),
    }
}

/// Evaluates a Vagrantfile returning the environment config.
///
/// # Arguments
///
/// * `path` - Path to the `Vagrantfile`.
///
/// # Returns
///
/// Returns an `EnvironmentConfig` loaded from the file.
///
/// # Errors
///
/// Returns a `MigratoryError` if evaluation or JSON parsing fails.
pub fn parse_vagrantfile(path: &str) -> Result<EnvironmentConfig, MigratoryError> {
    parse_vagrantfiles(&[std::path::Path::new(path)])
}

/// Parses a raw JSON value (from Ruby evaluation) into an `EnvironmentConfig`.
///
/// # Arguments
///
/// * `parsed_json` - JSON value containing the evaluated Vagrantfile configuration.
///
/// # Returns
///
/// An `EnvironmentConfig` containing all parsed machines, providers, networks, and settings.
pub fn parse_json_config(parsed_json: &serde_json::Value) -> EnvironmentConfig {
    let mut env = EnvironmentConfig::default();

    let empty_map = serde_json::Map::new();
    let machines_obj = parsed_json
        .get("machines")
        .and_then(|m| m.as_object())
        .unwrap_or(&empty_map);
    for (name, machine_val) in machines_obj {
        let mut machine_config = MachineConfig {
            name: name.clone(),
            primary: machine_val
                .get("primary")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            autostart: machine_val
                .get("autostart")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            ..Default::default()
        };

        let empty_val = serde_json::Value::Object(empty_map.clone());
        let vm_val = machine_val.get("vm").unwrap_or(&empty_val);

        machine_config.vm.box_name = vm_val
            .get("box_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        machine_config.vm.box_version = vm_val
            .get("box_version")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        machine_config.vm.box_url = vm_val
            .get("box_url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        machine_config.vm.box_check_update =
            vm_val.get("box_check_update").and_then(|v| v.as_bool());
        machine_config.vm.box_download_checksum = vm_val
            .get("box_download_checksum")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        machine_config.vm.box_download_checksum_type = vm_val
            .get("box_download_checksum_type")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        machine_config.vm.box_download_client_cert = vm_val
            .get("box_download_client_cert")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        machine_config.vm.box_download_ca_cert = vm_val
            .get("box_download_ca_cert")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        machine_config.vm.box_download_insecure = vm_val
            .get("box_download_insecure")
            .and_then(|v| v.as_bool());
        machine_config.vm.hostname = vm_val
            .get("hostname")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        machine_config.vm.guest = vm_val
            .get("guest")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        machine_config.vm.post_up_message = vm_val
            .get("post_up_message")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        machine_config.vm.boot_timeout = vm_val.get("boot_timeout").and_then(|v| v.as_u64());
        machine_config.vm.communicator = vm_val
            .get("communicator")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        machine_config.vm.graceful_halt_timeout =
            vm_val.get("graceful_halt_timeout").and_then(|v| v.as_u64());

        if let Some(range) = vm_val.get("usable_port_range").and_then(|r| r.as_array())
            && range.len() == 2
        {
            let start = range[0].as_u64().unwrap_or(2200) as u16;
            let end = range[1].as_u64().unwrap_or(2250) as u16;
            machine_config.vm.usable_port_range = (start, end);
        }
        if let Some(deps) = vm_val.get("depends_on").and_then(|d| d.as_array()) {
            for dep in deps {
                if let Some(s) = dep.as_str() {
                    machine_config.depends_on.push(s.to_string());
                }
            }
        }
        if let Some(types) = vm_val
            .get("allowed_synced_folder_types")
            .and_then(|t| t.as_array())
        {
            machine_config.vm.allowed_synced_folder_types = Some(
                types
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect(),
            );
        }
        if let Some(disks) = vm_val.get("disks").and_then(|d| d.as_array()) {
            for d in disks {
                let d_type = d
                    .get("disk_type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("disk")
                    .to_string();
                let size = d
                    .get("size")
                    .and_then(|s| s.as_str())
                    .map(|s| s.to_string());
                let name = d
                    .get("name")
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string());
                let primary = d.get("primary").and_then(|p| p.as_bool()).unwrap_or(false);
                machine_config.vm.disks.push(crate::config::DiskConfig {
                    disk_type: d_type,
                    size,
                    name,
                    primary,
                });
            }
        }

        let empty_vec = vec![];
        let empty_map = serde_json::Map::new();

        if let Some(ssh_val) = machine_val.get("ssh").and_then(|s| s.as_object()) {
            if let Some(u) = ssh_val.get("username").and_then(|v| v.as_str()) {
                machine_config.ssh.username = u.to_string();
            }
            if let Some(h) = ssh_val.get("host").and_then(|v| v.as_str()) {
                machine_config.ssh.host = h.to_string();
            }
            if let Some(p) = ssh_val.get("port").and_then(|v| v.as_u64()) {
                machine_config.ssh.port = p as u16;
            }
            if let Some(p) = ssh_val.get("password").and_then(|v| v.as_str()) {
                machine_config.ssh.password = Some(p.to_string());
            }
            if let Some(pk) = ssh_val.get("private_key_path").and_then(|v| v.as_str()) {
                machine_config.ssh.private_key_path = Some(pk.to_string());
            }
            if let Some(ik) = ssh_val.get("insert_key").and_then(|v| v.as_bool()) {
                machine_config.ssh.insert_key = ik;
            }
            if let Some(fa) = ssh_val.get("forward_agent").and_then(|v| v.as_bool()) {
                machine_config.ssh.forward_agent = fa;
            }
            if let Some(fx) = ssh_val.get("forward_x11").and_then(|v| v.as_bool()) {
                machine_config.ssh.forward_x11 = fx;
            }
            if let Some(pc) = ssh_val.get("proxy_command").and_then(|v| v.as_str()) {
                machine_config.ssh.proxy_command = Some(pc.to_string());
            }
            if let Some(gp) = ssh_val.get("guest_port").and_then(|v| v.as_u64()) {
                machine_config.ssh.guest_port = Some(gp as u16);
            }
            if let Some(ea) = ssh_val.get("extra_args").and_then(|v| v.as_array()) {
                machine_config.ssh.extra_args = Some(
                    ea.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect(),
                );
            }
            if let Some(fe) = ssh_val.get("forward_env").and_then(|v| v.as_array()) {
                machine_config.ssh.forward_env = fe
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
            }
            if let Some(pty) = ssh_val.get("pty").and_then(|v| v.as_bool()) {
                machine_config.ssh.pty = pty;
            }
            if let Some(ka) = ssh_val.get("keep_alive").and_then(|v| v.as_bool()) {
                machine_config.ssh.keep_alive = ka;
            }
            if let Some(sh) = ssh_val.get("shell").and_then(|v| v.as_str()) {
                machine_config.ssh.shell = Some(sh.to_string());
            }
            if let Some(ect) = ssh_val
                .get("export_command_template")
                .and_then(|v| v.as_str())
            {
                machine_config.ssh.export_command_template = Some(ect.to_string());
            }
            if let Some(ct) = ssh_val.get("connect_timeout").and_then(|v| v.as_u64()) {
                machine_config.ssh.connect_timeout = Some(ct);
            }
            if let Some(t) = ssh_val.get("timeout").and_then(|v| v.as_u64()) {
                machine_config.ssh.timeout = Some(t);
            }
            if let Some(vhk) = ssh_val.get("verify_host_key").and_then(|v| v.as_bool()) {
                machine_config.ssh.verify_host_key = vhk;
            }
            if let Some(ko) = ssh_val.get("keys_only").and_then(|v| v.as_bool()) {
                machine_config.ssh.keys_only = ko;
            }
        }

        if let Some(winrm_val) = machine_val.get("winrm").and_then(|w| w.as_object()) {
            if let Some(u) = winrm_val.get("username").and_then(|v| v.as_str()) {
                machine_config.winrm.username = u.to_string();
            }
            if let Some(p) = winrm_val.get("password").and_then(|v| v.as_str()) {
                machine_config.winrm.password = Some(p.to_string());
            }
            if let Some(h) = winrm_val.get("host").and_then(|v| v.as_str()) {
                machine_config.winrm.host = h.to_string();
            }
            if let Some(p) = winrm_val.get("port").and_then(|v| v.as_u64()) {
                machine_config.winrm.port = p as u16;
            }
            if let Some(gp) = winrm_val.get("guest_port").and_then(|v| v.as_u64()) {
                machine_config.winrm.guest_port = Some(gp as u16);
            }
            if let Some(s) = winrm_val.get("ssl").and_then(|v| v.as_bool()) {
                machine_config.winrm.ssl = s;
            }
            if let Some(t) = winrm_val.get("transport").and_then(|v| v.as_str()) {
                machine_config.winrm.transport = Some(t.to_string());
            }
            if let Some(ba) = winrm_val.get("basic_auth_only").and_then(|v| v.as_bool()) {
                machine_config.winrm.basic_auth_only = ba;
            }
            if let Some(sp) = winrm_val
                .get("ssl_peer_verification")
                .and_then(|v| v.as_bool())
            {
                machine_config.winrm.ssl_peer_verification = sp;
            }
            if let Some(t) = winrm_val.get("timeout").and_then(|v| v.as_u64()) {
                machine_config.winrm.timeout = Some(t);
            }
            if let Some(rl) = winrm_val.get("retry_limit").and_then(|v| v.as_u64()) {
                machine_config.winrm.retry_limit = Some(rl as u32);
            }
            if let Some(rd) = winrm_val.get("retry_delay").and_then(|v| v.as_u64()) {
                machine_config.winrm.retry_delay = Some(rd);
            }
            if let Some(etl) = winrm_val
                .get("execution_time_limit")
                .and_then(|v| v.as_str())
            {
                machine_config.winrm.execution_time_limit = Some(etl.to_string());
            }
        }

        let dummy_map = serde_json::Map::new();
        let vagrant_val = machine_val
            .get("vagrant")
            .and_then(|v| v.as_object())
            .unwrap_or(&dummy_map);
        if let Some(h) = vagrant_val.get("host").and_then(|v| v.as_str()) {
            machine_config.vagrant.host = Some(h.to_string());
        }
        let plugins = vagrant_val
            .get("plugins")
            .and_then(|p| p.as_array())
            .unwrap_or(&empty_vec);
        let mut plugin_strs: Vec<String> = plugins
            .iter()
            .filter_map(|p| p.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        machine_config.vagrant.plugins.append(&mut plugin_strs);

        let sens = vagrant_val
            .get("sensitive")
            .and_then(|p| p.as_array())
            .unwrap_or(&empty_vec);
        let mut sens_strs: Vec<String> = sens
            .iter()
            .filter_map(|p| p.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        machine_config.vagrant.sensitive.append(&mut sens_strs);

        let networks = vm_val
            .get("networks")
            .and_then(|n| n.as_array())
            .unwrap_or(&empty_vec);
        for net in networks {
            let n_type = net.get("type").and_then(|t| t.as_str()).unwrap_or("");
            let opts = net
                .get("options")
                .and_then(|o| o.as_object())
                .unwrap_or(&empty_map);

            match n_type {
                "forwarded_port" => {
                    let guest = opts.get("guest").and_then(|g| g.as_u64()).unwrap_or(0) as u16;
                    let host = opts.get("host").and_then(|h| h.as_u64()).unwrap_or(0) as u16;
                    let auto_correct = opts
                        .get("auto_correct")
                        .and_then(|a| a.as_bool())
                        .unwrap_or(false);
                    let protocol = opts
                        .get("protocol")
                        .and_then(|p| p.as_str())
                        .map(|s| s.to_string());
                    let host_ip = opts
                        .get("host_ip")
                        .and_then(|h| h.as_str())
                        .map(|s| s.to_string());
                    machine_config
                        .vm
                        .networks
                        .push(NetworkConfig::ForwardedPort {
                            guest,
                            host,
                            auto_correct,
                            protocol,
                            host_ip,
                        });
                }
                "private_network" => {
                    let ip = opts
                        .get("ip")
                        .and_then(|ip| ip.as_str())
                        .map(|s| s.to_string());
                    let netmask = opts
                        .get("netmask")
                        .and_then(|nm| nm.as_str())
                        .map(|s| s.to_string());
                    let dhcp = opts.get("type").and_then(|t| t.as_str()) == Some("dhcp")
                        || opts.get("dhcp").and_then(|d| d.as_bool()).unwrap_or(false);
                    let virtualbox_intnet = opts
                        .get("virtualbox__intnet")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    machine_config
                        .vm
                        .networks
                        .push(NetworkConfig::PrivateNetwork {
                            ip,
                            netmask,
                            dhcp,
                            virtualbox_intnet,
                        });
                }
                "public_network" => {
                    let ip = opts
                        .get("ip")
                        .and_then(|ip| ip.as_str())
                        .map(|s| s.to_string());
                    let bridge = opts
                        .get("bridge")
                        .and_then(|b| b.as_str())
                        .map(|s| s.to_string());
                    let use_dhcp_assigned_default_route = opts
                        .get("use_dhcp_assigned_default_route")
                        .and_then(|d| d.as_bool())
                        .unwrap_or(false);
                    machine_config
                        .vm
                        .networks
                        .push(NetworkConfig::PublicNetwork {
                            ip,
                            bridge,
                            use_dhcp_assigned_default_route,
                        });
                }
                _ => {}
            }
        }

        let providers = vm_val
            .get("providers")
            .and_then(|p| p.as_array())
            .unwrap_or(&empty_vec);
        for prov in providers {
            let p_name = prov.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let mut options_map = std::collections::HashMap::new();
            let opts = prov
                .get("options")
                .and_then(|o| o.as_object())
                .unwrap_or(&empty_map);
            for (k, v) in opts {
                let val_str = match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(num) => num.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    _ => "".to_string(),
                };
                options_map.insert(k.clone(), val_str);
            }
            machine_config.vm.providers.push(ProviderConfig {
                name: p_name.to_string(),
                options: options_map,
            });
        }

        let provisioners = vm_val
            .get("provisioners")
            .and_then(|p| p.as_array())
            .unwrap_or(&empty_vec);
        for prov in provisioners {
            let p_name = prov.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let p_id = prov
                .get("id")
                .and_then(|i| i.as_str())
                .map(|s| s.to_string());
            let p_run = prov
                .get("run")
                .and_then(|r| r.as_str())
                .map(|s| s.to_string());
            let mut options_map = std::collections::HashMap::new();
            let opts = prov
                .get("options")
                .and_then(|o| o.as_object())
                .unwrap_or(&empty_map);
            for (k, v) in opts {
                let val_str = match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(num) => num.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    _ => "".to_string(),
                };
                options_map.insert(k.clone(), val_str);
            }
            machine_config.vm.provisioners.push(ProvisionerConfig {
                name: p_name.to_string(),
                config: options_map,
                id: p_id,
                run: p_run,
            });
        }

        let triggers = machine_val
            .get("triggers")
            .and_then(|t| t.as_array())
            .unwrap_or(&empty_vec);
        for trig in triggers {
            let stage = trig
                .get("stage")
                .and_then(|s| s.as_str())
                .unwrap_or("before")
                .to_string();
            let actions = trig
                .get("actions")
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let opts = trig
                .get("options")
                .and_then(|o| o.as_object())
                .unwrap_or(&empty_map);
            let run_inline = opts
                .get("run")
                .and_then(|r| r.get("inline"))
                .and_then(|i| i.as_str())
                .map(|s| s.to_string());
            let run_path = opts
                .get("run")
                .and_then(|r| r.get("path"))
                .and_then(|p| p.as_str())
                .map(|s| s.to_string());
            let run_remote_inline = opts
                .get("run_remote")
                .and_then(|r| r.get("inline"))
                .and_then(|i| i.as_str())
                .map(|s| s.to_string());
            let run_remote_path = opts
                .get("run_remote")
                .and_then(|r| r.get("path"))
                .and_then(|p| p.as_str())
                .map(|s| s.to_string());
            let ignore_errors = opts
                .get("ignore_errors")
                .and_then(|e| e.as_bool())
                .unwrap_or(false);
            let on_error = opts
                .get("on_error")
                .and_then(|o| o.as_str())
                .map(|s| s.to_string());
            let mut env_map = std::collections::HashMap::new();
            if let Some(e) = opts.get("env").and_then(|e| e.as_object()) {
                for (k, v) in e {
                    if let Some(vs) = v.as_str() {
                        env_map.insert(k.clone(), vs.to_string());
                    }
                }
            }
            machine_config.triggers.push(crate::config::TriggerConfig {
                stage,
                actions,
                run_inline,
                run_path,
                run_remote_inline,
                run_remote_path,
                ignore_errors,
                on_error,
                env: env_map,
                ..Default::default()
            });
        }

        let folders = vm_val
            .get("synced_folders")
            .and_then(|f| f.as_array())
            .unwrap_or(&empty_vec);
        for f in folders {
            if let (Some(h_path), Some(g_path)) = (
                f.get("host_path").and_then(|h| h.as_str()),
                f.get("guest_path").and_then(|g| g.as_str()),
            ) {
                let mut folder_type = None;
                let mut disabled = false;

                let opts = f
                    .get("options")
                    .and_then(|o| o.as_object())
                    .unwrap_or(&empty_map);
                let mut owner = None;
                let mut group = None;
                let mut mount_options = None;
                let mut args = None;

                if let Some(t) = opts.get("type").and_then(|t| t.as_str()) {
                    folder_type = Some(t.to_string());
                }
                if let Some(d) = opts.get("disabled").and_then(|d| d.as_bool()) {
                    disabled = d;
                }
                if let Some(o) = opts.get("owner").and_then(|o| o.as_str()) {
                    owner = Some(o.to_string());
                }
                if let Some(g) = opts.get("group").and_then(|g| g.as_str()) {
                    group = Some(g.to_string());
                }
                if let Some(mo) = opts.get("mount_options").and_then(|m| m.as_array()) {
                    mount_options = Some(
                        mo.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect(),
                    );
                }
                if let Some(ar) = opts.get("args").and_then(|a| a.as_array()) {
                    args = Some(
                        ar.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect(),
                    );
                }

                machine_config.vm.synced_folders.push(SyncedFolderConfig {
                    host_path: h_path.to_string(),
                    guest_path: g_path.to_string(),
                    folder_type,
                    disabled,
                    owner,
                    group,
                    mount_options,
                    args,
                });
            }
        }

        env.machines.insert(name.clone(), machine_config);
    }

    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_init_ruby_vm() {
        init_ruby_vm(); // Just coverage
    }

    #[test]
    fn test_parse_vagrantfile_missing() {
        let res = parse_vagrantfile("/nonexistent/Vagrantfile");
        assert!(res.is_err());
    }

    #[test]
    fn test_parse_vagrantfile_syntax_error() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let path = dir.path().join("Vagrantfile");
        fs::write(&path, "invalid ruby syntax {").expect("operation should succeed");

        let res = parse_vagrantfile(path.to_str().expect("operation should succeed"));
        assert!(res.is_err());
    }

    #[test]
    fn test_parse_vagrantfile_valid_all_features() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let path = dir.path().join("Vagrantfile");
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.box = "ubuntu/bionic64"
  config.vm.box_version = "1.0.0"
  config.vm.hostname = "test-host"

  config.vm.network "forwarded_port", guest: 80, host: 8080, auto_correct: true
  config.vm.network "private_network", ip: "192.168.33.10"
  config.vm.network "public_network", ip: "1.2.3.4", bridge: "eth0"
  config.vm.network "unknown_type"

  config.vm.synced_folder "src/", "/srv/website", type: "rsync", owner: "vagrant", group: "vagrant", mount_options: ["dmode=775", "fmode=774"], args: ["--verbose"], disabled: false

  config.vm.provider "virtualbox" do |vb|
    vb.memory = 1024
    vb.cpus = 2
    vb.gui = true
    vb.name = "vbox_vm"
    vb.arr_opt = [1, 2, 3]
  end

  config.vm.provision "shell", inline: "echo hello", privileged: false, arr_opt: [1]

  config.vm.define "web" do |web|
    web.vm.box = "apache"
    web.ssh.username = "webuser"
    web.ssh.host = "ssh-host"
    web.ssh.port = 2222
    web.ssh.private_key_path = "/path/to/key"
    web.ssh.insert_key = false
    web.winrm.username = "admin"
    web.winrm.password = "secret"
    web.winrm.host = "winrm-host"
    web.winrm.port = 5985
    web.winrm.ssl = true
    web.vagrant.host = "vagrant-host"
    web.vagrant.plugins = ["vagrant-libvirt", "vagrant-vbguest"]
  end
  
  config.vm.define "db" do |db|
  end
end
        "#;
        fs::write(&path, vagrantfile_content).expect("operation should succeed");

        let env = parse_vagrantfile(path.to_str().expect("operation should succeed"))
            .expect("Should parse");
        assert!(env.machines.contains_key("web"));
        assert!(env.machines.contains_key("db"));

        let web = &env.machines["web"];
        assert_eq!(web.ssh.username, "webuser");
        assert_eq!(web.ssh.host, "ssh-host");
        assert_eq!(web.ssh.port, 2222);
        assert_eq!(web.ssh.private_key_path.as_deref(), Some("/path/to/key"));
        assert_eq!(web.ssh.insert_key, false);
        assert_eq!(web.winrm.username, "admin");
        assert_eq!(web.winrm.password.as_deref(), Some("secret"));
        assert_eq!(web.winrm.host, "winrm-host");
        assert_eq!(web.winrm.port, 5985);
        assert_eq!(web.winrm.ssl, true);
        assert_eq!(web.vagrant.host.as_deref(), Some("vagrant-host"));
        assert_eq!(
            web.vagrant.plugins,
            vec!["vagrant-libvirt", "vagrant-vbguest"]
        );

        let db = &env.machines["db"];
        assert_eq!(db.vm.box_name.as_deref(), Some("ubuntu/bionic64"));
        assert_eq!(db.vm.box_version.as_deref(), Some("1.0.0"));
        assert_eq!(db.vm.hostname.as_deref(), Some("test-host"));

        assert_eq!(db.vm.networks.len(), 3);
        assert!(matches!(
            &db.vm.networks[0],
            NetworkConfig::ForwardedPort {
                guest: 80,
                host: 8080,
                auto_correct: true,
                protocol: None,
                host_ip: None
            }
        ));
        assert!(matches!(
            &db.vm.networks[1],
            NetworkConfig::PrivateNetwork { ip: Some(ip), .. } if ip == "192.168.33.10"
        ));
        assert!(matches!(
            &db.vm.networks[2],
            NetworkConfig::PublicNetwork { ip: Some(ip), bridge: Some(bridge), .. } if ip == "1.2.3.4" && bridge == "eth0"
        ));

        assert_eq!(db.vm.synced_folders.len(), 1);
        let sf = &db.vm.synced_folders[0];
        assert_eq!(sf.host_path, "src/");
        assert_eq!(sf.guest_path, "/srv/website");
        assert_eq!(sf.folder_type.as_deref(), Some("rsync"));
        assert!(!sf.disabled);
        assert_eq!(sf.owner.as_deref(), Some("vagrant"));
        assert_eq!(sf.group.as_deref(), Some("vagrant"));
        assert_eq!(sf.mount_options.as_ref().expect("mount_options").len(), 2);
        assert_eq!(
            sf.mount_options.as_ref().expect("mount_options")[0],
            "dmode=775"
        );
        assert_eq!(
            sf.mount_options.as_ref().expect("mount_options")[1],
            "fmode=774"
        );
        assert_eq!(sf.args.as_ref().expect("args").len(), 1);
        assert_eq!(sf.args.as_ref().expect("args")[0], "--verbose");

        assert_eq!(db.vm.providers.len(), 1);
        let prov = &db.vm.providers[0];
        assert_eq!(prov.name, "virtualbox");
        assert_eq!(prov.options["memory"], "1024");
        assert_eq!(prov.options["gui"], "true");
        assert_eq!(prov.options["name"], "vbox_vm");
        assert_eq!(prov.options["arr_opt"], "");

        assert_eq!(db.vm.provisioners.len(), 1);
        let provisioner = &db.vm.provisioners[0];
        assert_eq!(provisioner.name, "shell");
        assert_eq!(provisioner.config["inline"], "echo hello");
        assert_eq!(provisioner.config["privileged"], "false");
        assert_eq!(provisioner.config["arr_opt"], "");
    }

    #[test]
    fn test_parse_vagrantfile_incomplete_network() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let path = dir.path().join("Vagrantfile");
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.vm.network "forwarded_port"
  config.vm.network "private_network"
  config.vm.network "public_network"
  config.vm.network "unknown", option: true
  
  config.vm.synced_folder "only_host", nil
  config.vm.synced_folder nil, "/guest"
  
  config.vm.provider "virtualbox"
  config.vm.provision "shell"
  config.vm.provision "ansible", boolean_opt: true, num_opt: 42
end
        "#;
        fs::write(&path, vagrantfile_content).expect("operation should succeed");

        let env = parse_vagrantfile(path.to_str().expect("operation should succeed"))
            .expect("operation should succeed");
        let def = &env.machines["default"];

        // forwarded_port parsed (defaults), private skipped (no IP), public parsed (defaults), unknown skipped
        assert_eq!(def.vm.networks.len(), 3);

        assert_eq!(
            def.vm.networks[0],
            NetworkConfig::ForwardedPort {
                guest: 0,
                host: 0,
                auto_correct: false,
                protocol: None,
                host_ip: None
            }
        );
        assert_eq!(
            def.vm.networks[1],
            NetworkConfig::PrivateNetwork {
                ip: None,
                netmask: None,
                dhcp: false,
                virtualbox_intnet: None
            }
        );
        assert_eq!(
            def.vm.networks[2],
            NetworkConfig::PublicNetwork {
                ip: None,
                bridge: None,
                use_dhcp_assigned_default_route: false
            }
        );

        assert_eq!(def.vm.providers.len(), 1);
        assert_eq!(def.vm.provisioners.len(), 2);

        assert_eq!(def.vm.synced_folders.len(), 0); // Both invalid
    }

    #[test]
    fn test_parse_vagrantfile_machine_config() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let path = dir.path().join("Vagrantfile");
        let vagrantfile_content = r#"
Vagrant.configure("2") do |config|
  config.ssh.username = "root"
  config.ssh.host = "1.2.3.4"
  config.ssh.port = 2222
  config.ssh.private_key_path = "/path/to/key"
  config.ssh.insert_key = false

  config.winrm.username = "Administrator"
  config.winrm.password = "secret"
  config.winrm.host = "5.6.7.8"
  config.winrm.port = 5986
  config.winrm.ssl = true

  config.vagrant.host = "linux"
  config.vagrant.plugins = ["vagrant-aws", "vagrant-vbguest"]
end
        "#;
        fs::write(&path, vagrantfile_content).expect("operation should succeed");

        let env = parse_vagrantfile(path.to_str().expect("operation should succeed"))
            .expect("operation should succeed");
        let def = &env.machines["default"];

        assert_eq!(def.ssh.username, "root");
        assert_eq!(def.ssh.host, "1.2.3.4");
        assert_eq!(def.ssh.port, 2222);
        assert_eq!(def.ssh.private_key_path.as_deref(), Some("/path/to/key"));
        assert_eq!(def.ssh.insert_key, false);

        assert_eq!(def.winrm.username, "Administrator");
        assert_eq!(def.winrm.password.as_deref(), Some("secret"));
        assert_eq!(def.winrm.host, "5.6.7.8");
        assert_eq!(def.winrm.port, 5986);
        assert_eq!(def.winrm.ssl, true);

        assert_eq!(def.vagrant.host.as_deref(), Some("linux"));
        assert_eq!(def.vagrant.plugins, vec!["vagrant-aws", "vagrant-vbguest"]);
    }

    #[test]
    fn test_parse_vagrantfile_invalid_types() {
        // Test parsing where ssh, winrm, and vagrant are not objects
        // to cover the `and_then(|s| s.as_object())` returning None branch
        let json_content = r#"{
            "machines": {
                "default": {
                    "ssh": "not an object",
                    "winrm": true,
                    "vagrant": {
                        "plugins": "not an array"
                    }
                }
            }
        }"#;

        let parsed_json: serde_json::Value =
            serde_json::from_str(json_content).expect("operation should succeed");
        let env = crate::config::parser::parse_json_config(&parsed_json);
        let def = &env.machines["default"];

        // The values should remain defaults
        assert_eq!(def.ssh.username, "vagrant");
        assert_eq!(def.winrm.username, "vagrant");
        assert_eq!(def.vagrant.host, None);
        assert!(def.vagrant.plugins.is_empty());
    }

    #[test]
    fn test_parse_vagrantfile_dynamic_ruby_features() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempdir().expect("operation should succeed");
        let path = dir.path().join("Vagrantfile");
        let vagrantfile_content = r#"
require 'base64'
ENV['MY_CUSTOM_VAR'] = 'custom-box'

Vagrant.require_version ">= 1.5.0"

Vagrant.configure("2") do |config|
  if Vagrant.has_plugin?("vagrant-vbguest")
    config.vbguest.auto_update = false
  end

  config.vm.boot_timeout = 300
  config.vm.communicator = "ssh"
  
  (1..2).each do |i|
    config.vm.define "node#{i}" do |node|
      node.vm.box = ENV['MY_CUSTOM_VAR']
    end
  end
end
        "#;
        fs::write(&path, vagrantfile_content).expect("operation should succeed");

        let env = parse_vagrantfile(path.to_str().expect("operation should succeed"))
            .expect("operation should succeed");

        assert!(env.machines.contains_key("node1"));
        assert!(env.machines.contains_key("node2"));

        let node1 = &env.machines["node1"];
        assert_eq!(node1.vm.box_name.as_deref(), Some("custom-box"));

        let node2 = &env.machines["node2"];
        assert_eq!(node2.vm.box_name.as_deref(), Some("custom-box"));
    }
}

#[cfg(test)]
mod parse_json_tests {
    use super::*;

    #[test]
    fn test_parse_json_full_coverage() {
        let json_content = serde_json::json!({
            "machines": {
                "default": {
                    "primary": true,
                    "autostart": false,
                    "ssh": {
                        "password": "ssh_password",
                        "forward_agent": true,
                        "forward_x11": true,
                        "proxy_command": "ssh -q -W %h:%p proxy"
                    },
                    "winrm": {
                        "transport": "negotiate",
                        "timeout": 3600
                    },
                    "vagrant": {
                        "plugins": ["vagrant-vbguest", "vagrant-share"]
                    },
                    "triggers": [
                        {
                            "stage": "before",
                            "actions": ["up"],
                            "options": {
                                "run": {
                                    "inline": "echo hi"
                                },
                                "ignore_errors": true,
                                "env": {
                                    "FOO": "BAR",
                                    "NON_STR": 123
                                }
                            }
                        },
                        {
                            "stage": "after",
                            "actions": ["up"],
                            "options": {
                                "run": {
                                    "path": "/path/to/script.sh"
                                },
                                "run_remote": {
                                    "inline": "echo remote",
                                    "path": "/path/to/remote.sh"
                                },
                                "on_error": "halt",
                                "ignore_errors": false,
                                "env": {
                                    "BAZ": "QUX"
                                }
                            }
                        }
                    ],
                    "vm": {
                        "post_up_message": "VM is ready!",
                        "depends_on": ["db_service", 123],
                        "box_url": "https://example.com/box.box",
                        "box_download_checksum": "abc123checksum",
                        "box_download_checksum_type": "sha256",
                        "box_download_client_cert": "/path/to/cert",
                        "box_download_ca_cert": "/path/to/ca",
                        "box_download_insecure": true,
                        "networks": [
                            {
                                "type": "forwarded_port",
                                "options": {
                                    "guest": 80,
                                    "host": 8080,
                                    "protocol": "tcp",
                                    "host_ip": "127.0.0.1"
                                }
                            },
                            {
                                "type": "private_network",
                                "options": {
                                    "ip": "192.168.50.4",
                                    "virtualbox__intnet": "my_intnet"
                                }
                            },
                            {
                                "type": "private_network",
                                "options": {
                                    "dhcp": true
                                }
                            },
                            {
                                "type": "private_network",
                                "options": {
                                    "type": "dhcp"
                                }
                            }
                        ],
                        "provisioners": [
                            {
                                "name": "shell",
                                "id": "bootstrap",
                                "run": "always",
                                "options": {
                                    "inline": "echo prov"
                                }
                            }
                        ]
                    }
                }
            }
        });

        let env_config = parse_json_config(&json_content);
        let m = env_config
            .machines
            .get("default")
            .expect("operation should succeed");
        assert!(m.primary);
        assert!(!m.autostart);
        assert_eq!(m.depends_on, vec!["db_service".to_string()]);
        assert_eq!(m.vm.post_up_message.as_deref(), Some("VM is ready!"));
        assert_eq!(m.vm.box_url.as_deref(), Some("https://example.com/box.box"));
        assert_eq!(
            m.vm.box_download_checksum.as_deref(),
            Some("abc123checksum")
        );
        assert_eq!(m.vm.box_download_checksum_type.as_deref(), Some("sha256"));
        assert_eq!(
            m.vm.box_download_client_cert.as_deref(),
            Some("/path/to/cert")
        );
        assert_eq!(m.vm.box_download_ca_cert.as_deref(), Some("/path/to/ca"));
        assert_eq!(m.vm.box_download_insecure, Some(true));
        assert_eq!(m.vm.networks.len(), 4);
        assert_eq!(
            m.vm.networks[0],
            NetworkConfig::ForwardedPort {
                guest: 80,
                host: 8080,
                auto_correct: false,
                protocol: Some("tcp".to_string()),
                host_ip: Some("127.0.0.1".to_string()),
            }
        );
        assert_eq!(
            m.vm.networks[1],
            NetworkConfig::PrivateNetwork {
                ip: Some("192.168.50.4".to_string()),
                netmask: None,
                dhcp: false,
                virtualbox_intnet: Some("my_intnet".to_string()),
            }
        );
        assert_eq!(
            m.vm.networks[2],
            NetworkConfig::PrivateNetwork {
                ip: None,
                netmask: None,
                dhcp: true,
                virtualbox_intnet: None,
            }
        );
        assert_eq!(
            m.vm.networks[3],
            NetworkConfig::PrivateNetwork {
                ip: None,
                netmask: None,
                dhcp: true,
                virtualbox_intnet: None,
            }
        );
        assert_eq!(m.vm.provisioners.len(), 1);
        assert_eq!(m.vm.provisioners[0].id.as_deref(), Some("bootstrap"));
        assert_eq!(m.vm.provisioners[0].run.as_deref(), Some("always"));
        assert_eq!(m.triggers.len(), 2);
        assert_eq!(m.triggers[0].stage, "before");
        assert_eq!(m.triggers[0].actions, vec!["up".to_string()]);
        assert_eq!(m.triggers[0].run_inline.as_deref(), Some("echo hi"));
        assert!(m.triggers[0].ignore_errors);
        assert_eq!(
            m.triggers[0].env.get("FOO").map(|s| s.as_str()),
            Some("BAR")
        );
        assert_eq!(m.triggers[1].stage, "after");
        assert_eq!(
            m.triggers[1].run_path.as_deref(),
            Some("/path/to/script.sh")
        );
        assert_eq!(
            m.triggers[1].run_remote_inline.as_deref(),
            Some("echo remote")
        );
        assert_eq!(
            m.triggers[1].run_remote_path.as_deref(),
            Some("/path/to/remote.sh")
        );
        assert_eq!(m.triggers[1].on_error.as_deref(), Some("halt"));

        assert_eq!(m.ssh.password, Some("ssh_password".to_string()));
        assert_eq!(m.ssh.forward_agent, true);
        assert_eq!(m.ssh.forward_x11, true);
        assert_eq!(
            m.ssh.proxy_command,
            Some("ssh -q -W %h:%p proxy".to_string())
        );
        assert_eq!(m.winrm.transport, Some("negotiate".to_string()));
        assert_eq!(m.winrm.timeout, Some(3600));
        assert_eq!(
            m.vagrant.plugins,
            vec!["vagrant-vbguest".to_string(), "vagrant-share".to_string()]
        );
    }

    #[test]
    fn test_parse_json_empty_vagrant() {
        let json_content = serde_json::json!({
            "machines": {
                "default": {
                    "vagrant": {},
                    "vm": {}
                }
            }
        });

        let env_config = parse_json_config(&json_content);
        assert!(env_config.machines.contains_key("default"));
    }

    #[test]
    fn test_parse_json_invalid_plugin_type() {
        let json_content = serde_json::json!({
            "machines": {
                "default": {
                    "vagrant": {
                        "plugins": [123, null, "valid"]
                    },
                    "vm": {}
                }
            }
        });

        let env_config = parse_json_config(&json_content);
        let m = env_config
            .machines
            .get("default")
            .expect("operation should succeed");
        assert_eq!(m.vagrant.plugins, vec!["valid".to_string()]);
    }

    #[test]
    fn test_parse_vagrantfile_v1_legacy() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempfile::tempdir().expect("operation should succeed");
        let path = dir.path().join("Vagrantfile");
        let content = r#"
Vagrant.configure("1") do |config|
  config.vm.box = "base"
  config.vm.guest = :linux
  config.vm.forward_port 80, 8080
  config.vm.share_folder "v-root", "/vagrant", "."
  config.trigger.before :up do |t|
    # ruby block trigger
  end
  config.trigger.after :halt, run: ->(env) { }
end
"#;
        std::fs::write(&path, content).expect("operation should succeed");
        let env = parse_vagrantfile(path.to_str().expect("operation should succeed"))
            .expect("operation should succeed");
        let m = env
            .machines
            .get("default")
            .expect("operation should succeed");
        assert_eq!(m.vm.box_name.as_deref(), Some("base"));
        assert_eq!(m.vm.guest.as_deref(), Some("linux"));
        assert_eq!(m.vm.networks.len(), 1);
        assert_eq!(
            m.vm.networks[0],
            NetworkConfig::ForwardedPort {
                guest: 80,
                host: 8080,
                auto_correct: false,
                protocol: None,
                host_ip: None,
            }
        );
        assert_eq!(m.vm.synced_folders.len(), 1);
        assert_eq!(m.vm.synced_folders[0].guest_path, "/vagrant");
        assert_eq!(m.triggers.len(), 2);
    }

    #[test]
    fn test_parse_vagrantfiles_multi_file() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        let dir = tempfile::tempdir().expect("operation should succeed");
        let file1 = dir.path().join("Vagrantfile.base");
        let file2 = dir.path().join("Vagrantfile.override");

        std::fs::write(
            &file1,
            "Vagrant.configure('2') do |c|\n c.vm.box = 'box1'\n c.vm.hostname = 'host1'\nend",
        )
        .expect("operation should succeed");
        std::fs::write(
            &file2,
            "Vagrant.configure('2') do |c|\n c.vm.box = 'box2'\nend",
        )
        .expect("operation should succeed");

        let env = parse_vagrantfiles(&[&file1, &file2]).expect("operation should succeed");
        let m = env
            .machines
            .get("default")
            .expect("operation should succeed");
        assert_eq!(m.vm.box_name.as_deref(), Some("box2"));
        assert_eq!(m.vm.hostname.as_deref(), Some("host1"));

        // Test empty slice
        let empty_paths: Vec<&std::path::Path> = vec![];
        assert!(parse_vagrantfiles(&empty_paths).is_err());

        // Test nonexistent file
        let bad_path = dir.path().join("nonexistent");
        assert!(parse_vagrantfiles(&[&bad_path]).is_err());
    }

    #[test]
    fn test_parse_vagrantfiles_fallback_pure_rust() {
        let dir = tempfile::tempdir().expect("operation should succeed");
        let path = dir.path().join("Vagrantfile");
        let vf_content = r#"
Vagrant.configure("2") do |config|
  config.vm.box = "pure-rust-box"
  config.vm.hostname = "rust-host"
  config.vm.usable_port_range = 2220..2230
  config.vm.disk :disk, size: "15GB", name: "pure_disk"
  config.ssh.username = "rust_user"
  config.ssh.forward_agent = true
  config.winrm.username = "Administrator"
  config.vagrant.sensitive = ["my_secret"]
end
"#;
        std::fs::write(&path, vf_content).expect("operation should succeed");

        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");
        unsafe {
            std::env::set_var("MIGRATORY_FORCE_PURE_RUST", "1");
        }

        let env_res = parse_vagrantfile(path.to_str().expect("operation should succeed"));
        assert!(env_res.is_ok());

        // Test pure rust with nonexistent file
        let bad_path = dir.path().join("nonexistent");
        let bad_res = parse_vagrantfiles(&[&bad_path]);
        assert!(bad_res.is_err());

        // Test pure rust with directory as path (triggers std::fs::read_to_string Io error)
        let dir_as_file = dir.path();
        let io_err_res = parse_vagrantfiles(&[dir_as_file]);
        assert!(io_err_res.is_err());

        // Test pure rust with unsatisfiable version requirement (triggers evaluate_in_process error)
        let syntax_err_path = dir.path().join("Vagrantfile.bad_version");
        std::fs::write(&syntax_err_path, b"Vagrant.require_version '>= 99.0.0'").expect("write");
        let syntax_err_res = parse_vagrantfiles(&[&syntax_err_path]);
        assert!(syntax_err_res.is_err());

        unsafe {
            std::env::remove_var("MIGRATORY_FORCE_PURE_RUST");
        }

        assert!(env_res.is_ok());
        let env = env_res.expect("operation should succeed");
        assert!(env.machines.contains_key("default"));
        let def = &env.machines["default"];
        assert_eq!(def.vm.box_name.as_deref(), Some("pure-rust-box"));
        assert_eq!(def.vm.hostname.as_deref(), Some("rust-host"));
        assert_eq!(def.vm.usable_port_range, (2220, 2230));
        assert_eq!(def.vm.disks.len(), 1);
        assert_eq!(def.vm.disks[0].size.as_deref(), Some("15GB"));
        assert_eq!(def.ssh.username, "rust_user");
        assert!(def.ssh.forward_agent);
        assert_eq!(def.winrm.username, "Administrator");
        assert_eq!(def.vagrant.sensitive, vec!["my_secret".to_string()]);
    }

    #[test]
    fn test_parse_json_new_fields() {
        let json_content = serde_json::json!({
            "machines": {
                "default": {
                    "ssh": {
                        "guest_port": 2222,
                        "extra_args": ["-v", "-o", "StrictHostKeyChecking=no"],
                        "forward_env": ["VAR1", "VAR2"],
                        "pty": true,
                        "keep_alive": false,
                        "shell": "/bin/zsh",
                        "export_command_template": "export %k=%v",
                        "connect_timeout": 30,
                        "timeout": 120,
                        "verify_host_key": true,
                        "keys_only": false
                    },
                    "winrm": {
                        "guest_port": 5986,
                        "basic_auth_only": true,
                        "ssl_peer_verification": false,
                        "retry_limit": 5,
                        "retry_delay": 10,
                        "execution_time_limit": "PT1H"
                    },
                    "vagrant": {
                        "sensitive": ["token123"]
                    },
                    "vm": {
                        "guest": "freebsd",
                        "usable_port_range": [2200, 2300],
                        "allowed_synced_folder_types": ["nfs", "rsync"],
                        "disks": [
                            {
                                "disk_type": "disk",
                                "size": "25GB",
                                "name": "extra",
                                "primary": true
                            }
                        ],
                        "networks": [
                            {
                                "type": "private_network",
                                "options": {
                                    "ip": "192.168.50.4",
                                    "netmask": "255.255.255.0",
                                    "dhcp": true,
                                    "virtualbox__intnet": "intnet1"
                                }
                            },
                            {
                                "type": "public_network",
                                "options": {
                                    "bridge": "en0",
                                    "use_dhcp_assigned_default_route": true
                                }
                            }
                        ]
                    }
                }
            }
        });

        let env = parse_json_config(&json_content);
        let m = env
            .machines
            .get("default")
            .expect("operation should succeed");
        assert_eq!(m.vm.guest.as_deref(), Some("freebsd"));
        assert_eq!(m.vm.usable_port_range, (2200, 2300));
        assert_eq!(
            m.vm.allowed_synced_folder_types.as_deref(),
            Some(&["nfs".to_string(), "rsync".to_string()][..])
        );
        assert_eq!(m.vm.disks.len(), 1);
        assert_eq!(m.vm.disks[0].size.as_deref(), Some("25GB"));
        assert!(m.vm.disks[0].primary);

        assert_eq!(m.ssh.guest_port, Some(2222));
        assert_eq!(
            m.ssh.extra_args,
            Some(vec![
                "-v".to_string(),
                "-o".to_string(),
                "StrictHostKeyChecking=no".to_string()
            ])
        );
        assert_eq!(
            m.ssh.forward_env,
            vec!["VAR1".to_string(), "VAR2".to_string()]
        );
        assert!(m.ssh.pty);
        assert!(!m.ssh.keep_alive);
        assert_eq!(m.ssh.shell.as_deref(), Some("/bin/zsh"));
        assert_eq!(
            m.ssh.export_command_template.as_deref(),
            Some("export %k=%v")
        );
        assert_eq!(m.ssh.connect_timeout, Some(30));
        assert_eq!(m.ssh.timeout, Some(120));
        assert!(m.ssh.verify_host_key);
        assert!(!m.ssh.keys_only);

        assert_eq!(m.winrm.guest_port, Some(5986));
        assert!(m.winrm.basic_auth_only);
        assert!(!m.winrm.ssl_peer_verification);
        assert_eq!(m.winrm.retry_limit, Some(5));
        assert_eq!(m.winrm.retry_delay, Some(10));
        assert_eq!(m.winrm.execution_time_limit.as_deref(), Some("PT1H"));

        assert_eq!(m.vagrant.sensitive, vec!["token123".to_string()]);

        assert_eq!(m.vm.networks.len(), 2);
        assert_eq!(
            m.vm.networks[0],
            NetworkConfig::PrivateNetwork {
                ip: Some("192.168.50.4".to_string()),
                netmask: Some("255.255.255.0".to_string()),
                dhcp: true,
                virtualbox_intnet: Some("intnet1".to_string()),
            }
        );
        assert_eq!(
            m.vm.networks[1],
            NetworkConfig::PublicNetwork {
                ip: None,
                bridge: Some("en0".to_string()),
                use_dhcp_assigned_default_route: true,
            }
        );
    }

    #[test]
    fn test_parse_vagrantfiles_ruby_errors() {
        let _guard = crate::cli::commands::box_cmd::tests::ENV_LOCK
            .lock()
            .expect("lock failed");

        let dir = tempfile::tempdir().expect("operation should succeed");
        let path = dir.path().join("Vagrantfile");
        std::fs::write(&path, b"Vagrant.configure('2') do |c| end")
            .expect("operation should succeed");

        // 0. Empty paths error
        assert!(parse_vagrantfiles::<&std::path::Path>(&[]).is_err());

        // 1. Ruby execution permission denied / spawn error
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_RUBY_ERROR", "1");
        }
        let res_err = parse_vagrantfiles(&[&path]);
        assert!(res_err.is_err());
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_RUBY_ERROR");
        }

        // 2. Ruby returns invalid JSON
        unsafe {
            std::env::set_var("MIGRATORY_TEST_MOCK_RUBY_BAD_JSON", "1");
        }
        let res_bad_json = parse_vagrantfiles(&[&path]);
        assert!(res_bad_json.is_err());
        unsafe {
            std::env::remove_var("MIGRATORY_TEST_MOCK_RUBY_BAD_JSON");
        }
    }
}
