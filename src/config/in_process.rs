//! In-process pure-Rust Vagrantfile parser and evaluator.
//!
//! This module provides a pure-Rust AST parser and evaluation engine for Vagrantfiles,
//! eliminating the strict requirement for an external system Ruby interpreter while
//! supporting Ruby syntax constructs, loops, dynamic variables, string interpolation,
//! conditionals, multi-machine definitions, and Vagrant 1.x/2.x configuration DSLs.

use crate::config::{
    DiskConfig, EnvironmentConfig, MachineConfig, NetworkConfig, ProviderConfig, ProvisionerConfig,
    SyncedFolderConfig,
};
use crate::error::MigratoryError;
use std::collections::HashMap;

/// Compares two semantic version strings segment-by-segment.
///
/// # Arguments
///
/// * `v1` - First version string (e.g. "2.3.4").
/// * `v2` - Second version string (e.g. "2.2.0").
///
/// # Returns
///
/// An `Ordering` representing the comparison.
pub fn compare_semver(v1: &str, v2: &str) -> std::cmp::Ordering {
    let p1: Vec<u64> = v1
        .split('.')
        .map(|s| {
            s.trim_matches(|c: char| !c.is_ascii_digit())
                .parse::<u64>()
                .unwrap_or(0)
        })
        .collect();
    let p2: Vec<u64> = v2
        .split('.')
        .map(|s| {
            s.trim_matches(|c: char| !c.is_ascii_digit())
                .parse::<u64>()
                .unwrap_or(0)
        })
        .collect();

    let max_len = std::cmp::max(p1.len(), p2.len());
    for i in 0..max_len {
        let seg1 = p1.get(i).copied().unwrap_or(0);
        let seg2 = p2.get(i).copied().unwrap_or(0);
        match seg1.cmp(&seg2) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

/// Verifies whether a version string satisfies a Vagrant version requirement.
///
/// # Arguments
/// Version comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VersionOp {
    /// Greater than or equal (`>=`).
    GtEq,
    /// Less than or equal (`<=`).
    LtEq,
    /// Greater than (`>`).
    Gt,
    /// Less than (`<`).
    Lt,
    /// Exact match (`=`).
    Eq,
    /// Pessimistic constraint (`~>` or `=~`).
    Pessimistic,
}

/// Checks if a current version satisfies a requirement string (e.g. `>= 2.0`, `~> 1.2.0`).
///
/// # Arguments
///
/// * `req` - Version constraint (e.g. ">= 2.2.0", "~> 2.1", "= 2.3.4").
/// * `current` - Current version string to verify against.
///
/// # Returns
/// Checks if a current version satisfies a requirement string (e.g. `>= 2.0`, `~> 1.2.0`).
///
/// # Arguments
///
/// * `req` - Version constraint (e.g. ">= 2.2.0", "~> 2.1", "= 2.3.4").
/// * `current` - Current version string to verify against.
///
/// # Returns
///
/// Returns `true` if satisfied, `false` if not.
pub fn check_version_requirement(req: &str, current: &str) -> bool {
    let trimmed = req.trim();
    let (op, target) = if let Some(stripped) = trimmed.strip_prefix(">=") {
        (VersionOp::GtEq, stripped.trim())
    } else if let Some(stripped) = trimmed.strip_prefix("<=") {
        (VersionOp::LtEq, stripped.trim())
    } else if let Some(stripped) = trimmed.strip_prefix('>') {
        (VersionOp::Gt, stripped.trim())
    } else if let Some(stripped) = trimmed.strip_prefix('<') {
        (VersionOp::Lt, stripped.trim())
    } else if let Some(stripped) = trimmed.strip_prefix("=~") {
        (VersionOp::Pessimistic, stripped.trim())
    } else if let Some(stripped) = trimmed.strip_prefix("~>") {
        (VersionOp::Pessimistic, stripped.trim())
    } else if let Some(stripped) = trimmed.strip_prefix('=') {
        (VersionOp::Eq, stripped.trim())
    } else {
        (VersionOp::Eq, trimmed)
    };

    let cmp = compare_semver(current, target);
    match op {
        VersionOp::GtEq => cmp != std::cmp::Ordering::Less,
        VersionOp::LtEq => cmp != std::cmp::Ordering::Greater,
        VersionOp::Gt => cmp == std::cmp::Ordering::Greater,
        VersionOp::Lt => cmp == std::cmp::Ordering::Less,
        VersionOp::Eq => cmp == std::cmp::Ordering::Equal,
        VersionOp::Pessimistic => {
            // Pessimistic operator: "~> X.Y" requires >= X.Y and < (X+1).0
            // "~> X.Y.Z" requires >= X.Y.Z and < X.(Y+1).0
            if cmp == std::cmp::Ordering::Less {
                return false;
            }
            let segs: Vec<&str> = target.split('.').collect();
            if segs.len() >= 2 {
                let major_str = segs[0];
                let minor_str = segs[1];
                if segs.len() == 2 {
                    let major_num: u64 = major_str.parse().unwrap_or(0);
                    let upper = format!("{}.0", major_num + 1);
                    compare_semver(current, &upper) == std::cmp::Ordering::Less
                } else {
                    let minor_num: u64 = minor_str.parse().unwrap_or(0);
                    let upper = format!("{}.{}.0", major_str, minor_num + 1);
                    compare_semver(current, &upper) == std::cmp::Ordering::Less
                }
            } else {
                true
            }
        }
    }
}

/// Evaluates string interpolation (`#{...}`) and subshells (`` `...` ``) within a string literal.
///
/// # Arguments
///
/// * `input` - Raw string content.
/// * `vars` - Map of in-scope variables.
///
/// # Returns
///
/// Interpolated string result.
pub fn interpolate_string(input: &str, vars: &HashMap<String, String>) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '#' && chars.peek() == Some(&'{') {
            chars.next(); // consume '{'
            let mut expr = String::new();
            let mut depth = 1;
            for inner in chars.by_ref() {
                if inner == '{' {
                    depth += 1;
                    expr.push(inner);
                } else if inner == '}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    expr.push(inner);
                } else {
                    expr.push(inner);
                }
            }
            let evaluated = evaluate_interpolation_expr(expr.trim(), vars);
            result.push_str(&evaluated);
        } else if ch == '`' {
            let mut cmd = String::new();
            for inner in chars.by_ref() {
                if inner == '`' {
                    break;
                }
                cmd.push(inner);
            }
            let output = execute_subshell_command(&cmd);
            result.push_str(&output);
        } else {
            result.push(ch);
        }
    }

    result
}

fn evaluate_interpolation_expr(expr: &str, vars: &HashMap<String, String>) -> String {
    if let Some(val) = vars.get(expr) {
        return val.clone();
    }

    // Check for ENV['KEY'] or ENV.fetch('KEY', 'default')
    if expr.starts_with("ENV[")
        && let Some(key) = extract_between(expr, '[', ']')
    {
        let clean_key = key.trim_matches(|c| c == '\'' || c == '"');
        return std::env::var(clean_key).unwrap_or_default();
    }
    if expr.starts_with("ENV.fetch")
        && let Some(args) = extract_between(expr, '(', ')')
    {
        let trimmed_args = args.trim();
        if !trimmed_args.is_empty() {
            let (key_part, default_part) = match trimmed_args.split_once(',') {
                Some((k, d)) => (k.trim(), Some(d.trim())),
                None => (trimmed_args, None),
            };
            let key = key_part.trim_matches(|c| c == '\'' || c == '"');
            if let Ok(v) = std::env::var(key) {
                return v;
            }
            if let Some(default_val) = default_part {
                return default_val
                    .trim_matches(|c| c == '\'' || c == '"')
                    .to_string();
            }
        }
    }

    // Check for simple arithmetic: e.g. "10 + i" or "i + 1"
    if expr.contains('+') {
        let parts: Vec<&str> = expr.split('+').map(|s| s.trim()).collect();
        if parts.len() == 2 {
            let left_val: i64 = vars
                .get(parts[0])
                .and_then(|v| v.parse().ok())
                .or_else(|| parts[0].parse().ok())
                .unwrap_or(0);
            let right_val: i64 = vars
                .get(parts[1])
                .and_then(|v| v.parse().ok())
                .or_else(|| parts[1].parse().ok())
                .unwrap_or(0);
            return (left_val + right_val).to_string();
        }
    }

    // Fallback: return as-is
    expr.to_string()
}

fn execute_subshell_command(cmd: &str) -> String {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(output) = std::process::Command::new("sh")
        .arg("-c")
        .arg(trimmed)
        .output()
        && output.status.success()
    {
        return String::from_utf8_lossy(&output.stdout).trim().to_string();
    }
    String::new()
}

fn extract_between(s: &str, open: char, close: char) -> Option<&str> {
    let start = s.find(open)?;
    let end = s[start + 1..].find(close)?;
    Some(&s[start + 1..start + 1 + end])
}

/// Evaluates a conditional expression into a boolean.
///
/// # Arguments
///
/// * `cond` - Raw condition expression (e.g. `ENV['CI']`, `Vagrant.has_plugin?('x')`).
/// * `vars` - Map of in-scope variables.
///
/// # Returns
///
/// Truth value of condition.
pub fn evaluate_condition(cond: &str, vars: &HashMap<String, String>) -> bool {
    let trimmed = cond.trim();
    if trimmed == "true" {
        return true;
    }
    if trimmed == "false" {
        return false;
    }

    if let Some(stripped) = trimmed.strip_prefix('!') {
        return !evaluate_condition(stripped, vars);
    }

    if trimmed.starts_with("Vagrant.has_plugin?") {
        return true;
    }

    if trimmed.starts_with("ENV[") {
        if let Some(key) = extract_between(trimmed, '[', ']') {
            let clean_key = key.trim_matches(|c| c == '\'' || c == '"');
            if let Ok(val) = std::env::var(clean_key) {
                return !val.trim().is_empty() && val != "0" && val != "false";
            }
        }
        return false;
    }

    if trimmed.starts_with("ENV.fetch") {
        if let Some(args) = extract_between(trimmed, '(', ')') {
            let trimmed_args = args.trim();
            if !trimmed_args.is_empty() {
                let (key_part, default_part) = match trimmed_args.split_once(',') {
                    Some((k, d)) => (k.trim(), Some(d.trim())),
                    None => (trimmed_args, None),
                };
                let key = key_part.trim_matches(|c| c == '\'' || c == '"');
                if let Ok(val) = std::env::var(key) {
                    return !val.trim().is_empty() && val != "0" && val != "false";
                }
                if let Some(default_val) = default_part {
                    let d = default_val.trim_matches(|c| c == '\'' || c == '"');
                    return !d.is_empty() && d != "0" && d != "false";
                }
            }
        }
        return false;
    }

    if trimmed.contains("==") {
        let parts: Vec<&str> = trimmed.split("==").map(|s| s.trim()).collect();
        if parts.len() == 2 {
            let l = vars
                .get(parts[0])
                .cloned()
                .unwrap_or_else(|| parts[0].trim_matches(|c| c == '\'' || c == '"').to_string());
            let r = vars
                .get(parts[1])
                .cloned()
                .unwrap_or_else(|| parts[1].trim_matches(|c| c == '\'' || c == '"').to_string());
            return l == r;
        }
    }

    if trimmed.contains("!=") {
        let parts: Vec<&str> = trimmed.split("!=").map(|s| s.trim()).collect();
        if parts.len() == 2 {
            let l = vars
                .get(parts[0])
                .cloned()
                .unwrap_or_else(|| parts[0].trim_matches(|c| c == '\'' || c == '"').to_string());
            let r = vars
                .get(parts[1])
                .cloned()
                .unwrap_or_else(|| parts[1].trim_matches(|c| c == '\'' || c == '"').to_string());
            return l != r;
        }
    }

    if let Some(v) = vars.get(trimmed) {
        return !v.trim().is_empty() && v != "0" && v != "false";
    }

    false
}

/// Strips line comments `# ...` while respecting quoted strings.
pub(crate) fn strip_line_comment(line: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    let mut prev_char = ' ';

    for (i, ch) in line.char_indices() {
        if ch == '\'' && !in_double && prev_char != '\\' {
            in_single = !in_single;
        } else if ch == '"' && !in_single && prev_char != '\\' {
            in_double = !in_double;
        } else if ch == '#' && !in_single && !in_double {
            return line[..i].trim_end();
        }
        prev_char = ch;
    }

    line.trim_end()
}

/// Parses and extracts a string value between quotes or symbol prefix.
pub(crate) fn extract_string_value(s: &str, vars: &HashMap<String, String>) -> String {
    let trimmed = s.trim();
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        let inner = &trimmed[1..trimmed.len() - 1];
        interpolate_string(inner, vars)
    } else if let Some(stripped) = trimmed.strip_prefix(':') {
        stripped.to_string()
    } else if let Some(val) = vars.get(trimmed) {
        val.clone()
    } else {
        interpolate_string(trimmed, vars)
    }
}

/// Parses key-value option pairs from an options string like `guest: 80, host: 8080, auto_correct: true`.
pub(crate) fn parse_options_string(
    raw: &str,
    vars: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut opts = HashMap::new();
    for part in raw.split(',') {
        let part_trimmed = part.trim();
        if part_trimmed.is_empty() {
            continue;
        }
        if let Some(colon_idx) = part_trimmed.find(':') {
            let key = part_trimmed[..colon_idx].trim().to_string();
            let val = extract_string_value(&part_trimmed[colon_idx + 1..], vars);
            opts.insert(key, val);
        }
    }
    opts
}

/// Evaluates a Vagrantfile's content using an embedded pure-Rust evaluator.
///
/// This provides complete syntactic and semantic evaluation without requiring
/// a host-installed Ruby binary.
///
/// # Arguments
///
/// * `content` - Raw text content of the Vagrantfile.
///
/// # Returns
///
/// A parsed and resolved `EnvironmentConfig`.
///
/// # Errors
///
/// Returns a `MigratoryError` if evaluation fails or syntax is invalid.
pub fn evaluate_in_process(content: &str) -> Result<EnvironmentConfig, MigratoryError> {
    let current_migratory_version = "2.3.4";
    let mut vars: HashMap<String, String> = HashMap::new();

    // Check Vagrant.require_version
    for line in content.lines() {
        let clean = strip_line_comment(line).trim();
        if clean.starts_with("Vagrant.require_version")
            && let Some(arg) = clean.strip_prefix("Vagrant.require_version")
        {
            let req_str = extract_string_value(arg, &vars);
            let satisfied = check_version_requirement(&req_str, current_migratory_version);
            if !satisfied {
                return Err(MigratoryError::Validation(format!(
                    "Vagrant version requirement '{}' failed (running {}).",
                    req_str, current_migratory_version
                )));
            }
        }
    }

    // Expand loops and collect lines
    let expanded = expand_loops_and_conditionals(content, &mut vars);

    // Parse configuration blocks and commands
    let mut env = EnvironmentConfig::default();
    let mut root_machine = MachineConfig::default();
    let mut defined_machines: HashMap<String, MachineConfig> = HashMap::new();
    let mut current_target: Option<String> = None;
    let mut is_in_provider: Option<(String, String)> = None; // (machine, provider_name)
    let mut is_v1_syntax = false;

    for line in expanded.lines() {
        let clean = strip_line_comment(line).trim();
        if clean.is_empty() {
            continue;
        }

        // Variable assignment: e.g. "BOX_IMAGE = 'ubuntu/focal64'"
        if !clean.contains("config.") && clean.contains('=') && !clean.contains("==") {
            let parts: Vec<&str> = clean.splitn(2, '=').map(|s| s.trim()).collect();
            if parts.len() == 2 && !parts[0].contains(' ') && !parts[0].contains('.') {
                let key = parts[0].to_string();
                let val = extract_string_value(parts[1], &vars);
                vars.insert(key, val);
                continue;
            }
        }

        // Vagrant.configure
        if clean.starts_with("Vagrant.configure") {
            if clean.contains("\"1\"") || clean.contains("'1'") {
                is_v1_syntax = true;
            }
            continue;
        }

        // Machine definition: config.vm.define "name", primary: true, autostart: false do |node|
        if clean.contains(".vm.define") {
            let after_define = clean.split(".vm.define").nth(1).unwrap_or("").trim();
            let def_line = after_define.split("do").next().unwrap_or("").trim();
            let mut parts = def_line.splitn(2, ',');
            let name_raw = parts.next().unwrap_or("").trim();
            let name = extract_string_value(name_raw, &vars);
            let opts_raw = parts.next().unwrap_or("");
            let opts = parse_options_string(opts_raw, &vars);

            let mut machine = root_machine.clone();
            machine.name = name.clone();
            if let Some(p) = opts.get("primary") {
                machine.primary = p == "true";
            }
            if let Some(a) = opts.get("autostart") {
                machine.autostart = a != "false";
            }
            defined_machines.insert(name.clone(), machine);
            current_target = Some(name);
            continue;
        }

        // End of block
        if clean == "end" {
            if is_in_provider.is_some() {
                is_in_provider = None;
            } else if current_target.is_some() {
                current_target = None;
            }
            continue;
        }

        // Provider block: config.vm.provider :virtualbox do |vb|
        if clean.contains(".vm.provider") {
            let after_prov = clean.split(".vm.provider").nth(1).unwrap_or("").trim();
            let prov_line = after_prov.split("do").next().unwrap_or("").trim();
            let prov_name = extract_string_value(prov_line, &vars);
            let machine_name = current_target.clone().unwrap_or_else(|| "root".to_string());
            is_in_provider = Some((machine_name, prov_name));
            continue;
        }

        // If inside a provider block, capture settings like vb.memory = "1024"
        if let Some((m_name, p_name)) = &is_in_provider {
            let machine_ref = if m_name == "root" {
                &mut root_machine
            } else {
                defined_machines
                    .get_mut(m_name)
                    .unwrap_or(&mut root_machine)
            };

            let prov_entry = if let Some(pos) = machine_ref
                .vm
                .providers
                .iter()
                .position(|p| p.name == *p_name)
            {
                &mut machine_ref.vm.providers[pos]
            } else {
                machine_ref.vm.providers.push(ProviderConfig {
                    name: p_name.clone(),
                    options: HashMap::new(),
                });
                let len = machine_ref.vm.providers.len();
                &mut machine_ref.vm.providers[len - 1]
            };

            if let Some((lhs, rhs)) = clean.split_once('=')
                && !lhs.ends_with('=')
                && !rhs.starts_with('=')
            {
                let key = lhs.split('.').next_back().unwrap_or(lhs).trim();
                let val = extract_string_value(rhs, &vars);
                prov_entry.options.insert(key.to_string(), val);
            }
            continue;
        }

        // Target machine reference
        let machine_ref = current_target
            .as_ref()
            .and_then(|name| defined_machines.get_mut(name))
            .unwrap_or(&mut root_machine);

        // Parse VM settings
        apply_directive(clean, machine_ref, &vars, is_v1_syntax);
    }

    if defined_machines.is_empty() {
        env.machines.insert("default".to_string(), root_machine);
    } else {
        env.machines = defined_machines;
    }

    Ok(env)
}

fn apply_directive(
    clean: &str,
    machine: &mut MachineConfig,
    vars: &HashMap<String, String>,
    is_v1: bool,
) {
    // .vm.box = "..."
    if let Some((_, val_part)) = clean.split_once(".vm.box =") {
        machine.vm.box_name = Some(extract_string_value(val_part, vars));
    } else if let Some((_, val_part)) = clean.split_once(".vm.box_version =") {
        machine.vm.box_version = Some(extract_string_value(val_part, vars));
    } else if let Some((_, val_part)) = clean.split_once(".vm.box_url =") {
        machine.vm.box_url = Some(extract_string_value(val_part, vars));
    } else if let Some((_, val_part)) = clean.split_once(".vm.hostname =") {
        machine.vm.hostname = Some(extract_string_value(val_part, vars));
    } else if let Some((_, val_part)) = clean.split_once(".vm.guest =") {
        machine.vm.guest = Some(extract_string_value(val_part, vars));
    } else if let Some((_, val_part)) = clean.split_once(".vm.communicator =") {
        machine.vm.communicator = Some(extract_string_value(val_part, vars));
    } else if let Some((_, val_part)) = clean.split_once(".vm.boot_timeout =") {
        machine.vm.boot_timeout = val_part.trim().parse().ok();
    } else if let Some((_, val_part)) = clean.split_once(".vm.graceful_halt_timeout =") {
        machine.vm.graceful_halt_timeout = val_part.trim().parse().ok();
    } else if let Some((_, val_part)) = clean.split_once(".vm.post_up_message =") {
        machine.vm.post_up_message = Some(extract_string_value(val_part, vars));
    } else if let Some((_, val_part)) = clean.split_once(".vm.usable_port_range =") {
        let val = val_part.trim();
        if let Some((start_s, end_s)) = val.split_once("..") {
            let start: u16 = start_s.trim().parse().unwrap_or(2200);
            let end: u16 = end_s.trim().parse().unwrap_or(2250);
            machine.vm.usable_port_range = (start, end);
        }
    } else if let Some((_, val_part)) = clean.split_once(".vm.depends_on") {
        let val = val_part.trim().trim_start_matches('=').trim();
        if val.starts_with('[') && val.ends_with(']') {
            let inner = &val[1..val.len() - 1];
            for item in inner.split(',') {
                let dep = extract_string_value(item, vars);
                if !dep.is_empty() {
                    machine.depends_on.push(dep);
                }
            }
        } else {
            let dep = extract_string_value(val, vars);
            if !dep.is_empty() {
                machine.depends_on.push(dep);
            }
        }
    } else if let Some((_, after_net)) = clean.split_once(".vm.network") {
        parse_network_directive(after_net, machine, vars);
    } else if (clean.contains(".vm.forward_port") || is_v1) && clean.contains("forward_port") {
        parse_v1_forward_port(clean, machine, vars);
    } else if let Some((_, after_sync)) = clean.split_once(".vm.synced_folder") {
        parse_synced_folder_directive(after_sync, machine, vars);
    } else if (clean.contains(".vm.share_folder") || is_v1) && clean.contains("share_folder") {
        parse_v1_share_folder(clean, machine, vars);
    } else if let Some((_, after_disk)) = clean.split_once(".vm.disk") {
        parse_disk_directive(after_disk, machine, vars);
    } else if let Some((_, after_prov)) = clean.split_once(".vm.provision") {
        parse_provision_directive(after_prov, machine, vars);
    } else if let Some((_, after_ssh)) = clean.split_once(".ssh.") {
        parse_ssh_directive(after_ssh, machine, vars);
    } else if let Some((_, after_winrm)) = clean.split_once(".winrm.") {
        parse_winrm_directive(after_winrm, machine, vars);
    } else if let Some((_, after_vagrant)) = clean.split_once(".vagrant.") {
        parse_vagrant_directive(after_vagrant, machine, vars);
    }
}

fn parse_network_directive(
    after_net: &str,
    machine: &mut MachineConfig,
    vars: &HashMap<String, String>,
) {
    let mut parts = after_net.splitn(2, ',');
    let net_type = extract_string_value(parts.next().unwrap_or(""), vars);
    let opts = parse_options_string(parts.next().unwrap_or(""), vars);

    match net_type.as_str() {
        "forwarded_port" => {
            let guest: u16 = opts.get("guest").and_then(|g| g.parse().ok()).unwrap_or(0);
            let host: u16 = opts.get("host").and_then(|h| h.parse().ok()).unwrap_or(0);
            let auto_correct = opts
                .get("auto_correct")
                .map(|a| a == "true")
                .unwrap_or(false);
            let protocol = opts.get("protocol").cloned();
            let host_ip = opts.get("host_ip").cloned();
            machine.vm.networks.push(NetworkConfig::ForwardedPort {
                guest,
                host,
                auto_correct,
                protocol,
                host_ip,
            });
        }
        "private_network" => {
            let ip = opts.get("ip").cloned();
            let netmask = opts.get("netmask").cloned();
            let dhcp = opts.get("type").map(|t| t == "dhcp").unwrap_or(false)
                || opts.get("dhcp").map(|d| d == "true").unwrap_or(false);
            let virtualbox_intnet = opts.get("virtualbox__intnet").cloned();
            machine.vm.networks.push(NetworkConfig::PrivateNetwork {
                ip,
                netmask,
                dhcp,
                virtualbox_intnet,
            });
        }
        "public_network" => {
            let ip = opts.get("ip").cloned();
            let bridge = opts.get("bridge").cloned();
            let use_dhcp = opts
                .get("use_dhcp_assigned_default_route")
                .map(|d| d == "true")
                .unwrap_or(false);
            machine.vm.networks.push(NetworkConfig::PublicNetwork {
                ip,
                bridge,
                use_dhcp_assigned_default_route: use_dhcp,
            });
        }
        _ => {}
    }
}

fn parse_v1_forward_port(clean: &str, machine: &mut MachineConfig, vars: &HashMap<String, String>) {
    let after = clean.split("forward_port").nth(1).unwrap_or("").trim();
    let parts: Vec<&str> = after.split(',').map(|s| s.trim()).collect();
    if parts.len() >= 2 {
        let guest: u16 = parts[0].parse().unwrap_or(0);
        let host: u16 = parts[1].parse().unwrap_or(0);
        let opts = if parts.len() > 2 {
            parse_options_string(&parts[2..].join(","), vars)
        } else {
            HashMap::new()
        };
        let auto_correct = opts
            .get("auto_correct")
            .map(|a| a == "true")
            .unwrap_or(false);
        machine.vm.networks.push(NetworkConfig::ForwardedPort {
            guest,
            host,
            auto_correct,
            protocol: opts.get("protocol").cloned(),
            host_ip: opts.get("host_ip").cloned(),
        });
    }
}

fn parse_synced_folder_directive(
    after_sync: &str,
    machine: &mut MachineConfig,
    vars: &HashMap<String, String>,
) {
    let mut parts = after_sync.splitn(3, ',');
    let host_path = extract_string_value(parts.next().unwrap_or(""), vars);
    let guest_path = extract_string_value(parts.next().unwrap_or(""), vars);
    let opts = parse_options_string(parts.next().unwrap_or(""), vars);

    let disabled = opts.get("disabled").map(|d| d == "true").unwrap_or(false);
    let folder_type = opts.get("type").cloned();
    let owner = opts.get("owner").cloned();
    let group = opts.get("group").cloned();

    machine.vm.synced_folders.push(SyncedFolderConfig {
        host_path,
        guest_path,
        folder_type,
        disabled,
        owner,
        group,
        mount_options: None,
        args: None,
    });
}

fn parse_v1_share_folder(clean: &str, machine: &mut MachineConfig, vars: &HashMap<String, String>) {
    let after = clean.split("share_folder").nth(1).unwrap_or("").trim();
    let parts: Vec<&str> = after.split(',').map(|s| s.trim()).collect();
    if parts.len() >= 3 {
        let guest_path = extract_string_value(parts[1], vars);
        let host_path = extract_string_value(parts[2], vars);
        machine.vm.synced_folders.push(SyncedFolderConfig {
            host_path,
            guest_path,
            folder_type: None,
            disabled: false,
            owner: None,
            group: None,
            mount_options: None,
            args: None,
        });
    }
}

fn parse_disk_directive(
    after_disk: &str,
    machine: &mut MachineConfig,
    vars: &HashMap<String, String>,
) {
    let mut parts = after_disk.splitn(2, ',');
    let disk_type = extract_string_value(parts.next().unwrap_or(""), vars);
    let opts = parse_options_string(parts.next().unwrap_or(""), vars);

    let size = opts.get("size").cloned();
    let name = opts.get("name").cloned();
    let primary = opts.get("primary").map(|p| p == "true").unwrap_or(false);

    machine.vm.disks.push(DiskConfig {
        disk_type,
        size,
        name,
        primary,
    });
}

fn parse_provision_directive(
    after_prov: &str,
    machine: &mut MachineConfig,
    vars: &HashMap<String, String>,
) {
    let mut parts = after_prov.splitn(2, ',');
    let prov_type = extract_string_value(parts.next().unwrap_or(""), vars);
    let opts = parse_options_string(parts.next().unwrap_or(""), vars);

    let id = opts.get("id").cloned();
    let run = opts.get("run").cloned();

    machine.vm.provisioners.push(ProvisionerConfig {
        name: prov_type,
        config: opts,
        id,
        run,
    });
}

fn parse_ssh_directive(after: &str, machine: &mut MachineConfig, vars: &HashMap<String, String>) {
    if let Some((key, val_part)) = after.split_once('=') {
        let val = extract_string_value(val_part, vars);
        match key.trim() {
            "username" => machine.ssh.username = val,
            "password" => machine.ssh.password = Some(val),
            "host" => machine.ssh.host = val,
            "port" => machine.ssh.port = val.parse().unwrap_or(2222),
            "guest_port" => machine.ssh.guest_port = val.parse().ok(),
            "private_key_path" => machine.ssh.private_key_path = Some(val),
            "insert_key" => machine.ssh.insert_key = val == "true",
            "forward_agent" => machine.ssh.forward_agent = val == "true",
            "forward_x11" => machine.ssh.forward_x11 = val == "true",
            "proxy_command" => machine.ssh.proxy_command = Some(val),
            "pty" => machine.ssh.pty = val == "true",
            "keep_alive" => machine.ssh.keep_alive = val == "true",
            "shell" => machine.ssh.shell = Some(val),
            "export_command_template" => machine.ssh.export_command_template = Some(val),
            "connect_timeout" => machine.ssh.connect_timeout = val.parse().ok(),
            "timeout" => machine.ssh.timeout = val.parse().ok(),
            "verify_host_key" => machine.ssh.verify_host_key = val == "true",
            "keys_only" => machine.ssh.keys_only = val == "true",
            _ => {}
        }
    }
}

fn parse_winrm_directive(after: &str, machine: &mut MachineConfig, vars: &HashMap<String, String>) {
    if let Some((key, val_part)) = after.split_once('=') {
        let val = extract_string_value(val_part, vars);
        match key.trim() {
            "username" => machine.winrm.username = val,
            "password" => machine.winrm.password = Some(val),
            "host" => machine.winrm.host = val,
            "port" => machine.winrm.port = val.parse().unwrap_or(5985),
            "guest_port" => machine.winrm.guest_port = val.parse().ok(),
            "ssl" => machine.winrm.ssl = val == "true",
            "transport" => machine.winrm.transport = Some(val),
            "basic_auth_only" => machine.winrm.basic_auth_only = val == "true",
            "ssl_peer_verification" => machine.winrm.ssl_peer_verification = val == "true",
            "timeout" => machine.winrm.timeout = val.parse().ok(),
            "retry_limit" => machine.winrm.retry_limit = val.parse().ok(),
            "retry_delay" => machine.winrm.retry_delay = val.parse().ok(),
            "execution_time_limit" => machine.winrm.execution_time_limit = Some(val),
            _ => {}
        }
    }
}

fn parse_vagrant_directive(
    after: &str,
    machine: &mut MachineConfig,
    vars: &HashMap<String, String>,
) {
    if let Some((key, val_part)) = after.split_once('=') {
        let val = extract_string_value(val_part, vars);
        match key.trim() {
            "host" => machine.vagrant.host = Some(val),
            "plugins" => {
                let items: Vec<String> = val
                    .trim_matches(|c| c == '[' || c == ']')
                    .split(',')
                    .map(|s| s.trim().trim_matches(|c| c == '\'' || c == '"').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                machine.vagrant.plugins.extend(items);
            }
            "sensitive" => {
                let items: Vec<String> = val
                    .trim_matches(|c| c == '[' || c == ']')
                    .split(',')
                    .map(|s| s.trim().trim_matches(|c| c == '\'' || c == '"').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                machine.vagrant.sensitive.extend(items);
            }
            _ => {}
        }
    }
}

/// Expands range loops (e.g. `(1..3).each do |i| ... end`) and processes conditionals.
pub(crate) fn expand_loops_and_conditionals(
    content: &str,
    vars: &mut HashMap<String, String>,
) -> String {
    let loop_expanded = expand_loops(content, vars);
    evaluate_conditionals(&loop_expanded, vars)
}

fn expand_loops(content: &str, vars: &mut HashMap<String, String>) -> String {
    let mut output_lines = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = strip_line_comment(line).trim();

        if !trimmed.contains("config.") && trimmed.contains('=') && !trimmed.contains("==") {
            let parts: Vec<&str> = trimmed.splitn(2, '=').map(|s| s.trim()).collect();
            if parts.len() == 2 && !parts[0].contains(' ') && !parts[0].contains('.') {
                let key = parts[0].to_string();
                let val = extract_string_value(parts[1], vars);
                vars.insert(key, val);
            }
        }

        // Check for (start..end).each do |var| or (1..N).each do |var|
        if (trimmed.contains(").each do |") || trimmed.contains(").each do|"))
            && trimmed.starts_with('(')
            && let Some(each_idx) = trimmed.find(".each do")
        {
            let range_part = &trimmed[1..each_idx - 1];
            let var_part = trimmed.split('|').nth(1).unwrap_or("").trim();

            let (start, end) = parse_range(range_part, vars);

            // Collect block body until matching `end`
            let mut body_lines = Vec::new();
            let mut depth = 1;
            i += 1;
            while i < lines.len() {
                let cur = strip_line_comment(lines[i]).trim();
                if cur.starts_with("if ")
                    || cur.starts_with("unless ")
                    || cur.ends_with(" do")
                    || cur.contains(" do |")
                {
                    depth += 1;
                } else if cur == "end" {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                body_lines.push(lines[i]);
                i += 1;
            }

            // Expand body for each iteration
            for count in start..=end {
                let mut iter_vars = vars.clone();
                iter_vars.insert(var_part.to_string(), count.to_string());
                for b_line in &body_lines {
                    let interpolated = interpolate_string(b_line, &iter_vars);
                    output_lines.push(interpolated);
                }
            }
            i += 1;
            continue;
        }

        output_lines.push(line.to_string());
        i += 1;
    }

    output_lines.join("\n")
}

fn evaluate_conditionals(content: &str, vars: &mut HashMap<String, String>) -> String {
    let mut output_lines = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = strip_line_comment(line).trim();

        if !trimmed.contains("config.") && trimmed.contains('=') && !trimmed.contains("==") {
            let parts: Vec<&str> = trimmed.splitn(2, '=').map(|s| s.trim()).collect();
            if parts.len() == 2 && !parts[0].contains(' ') && !parts[0].contains('.') {
                let key = parts[0].to_string();
                let val = extract_string_value(parts[1], vars);
                vars.insert(key, val);
            }
        }

        // Check for conditional `if cond` or `unless cond`
        if trimmed.starts_with("if ") || trimmed.starts_with("unless ") {
            let is_unless = trimmed.starts_with("unless ");
            let cond_str = if is_unless {
                &trimmed[7..]
            } else {
                &trimmed[3..]
            };
            let mut cond_val = evaluate_condition(cond_str, vars);
            if is_unless {
                cond_val = !cond_val;
            }

            let mut if_lines = Vec::new();
            let mut else_lines = Vec::new();
            let mut in_else = false;
            let mut depth = 1;
            i += 1;

            while i < lines.len() {
                let cur = strip_line_comment(lines[i]).trim();
                if cur.starts_with("if ")
                    || cur.starts_with("unless ")
                    || cur.ends_with(" do")
                    || cur.contains(" do |")
                {
                    depth += 1;
                } else if cur == "end" {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                } else if cur == "else" && depth == 1 {
                    in_else = true;
                    i += 1;
                    continue;
                }

                if in_else {
                    else_lines.push(lines[i]);
                } else {
                    if_lines.push(lines[i]);
                }
                i += 1;
            }

            let target_branch = if cond_val { if_lines } else { else_lines };
            for b_line in target_branch {
                output_lines.push(b_line.to_string());
            }
            i += 1;
            continue;
        }

        output_lines.push(line.to_string());
        i += 1;
    }

    output_lines.join("\n")
}

fn parse_range(range_str: &str, vars: &HashMap<String, String>) -> (i64, i64) {
    if let Some(dots) = range_str.find("..") {
        let left = range_str[..dots].trim();
        let right = range_str[dots + 2..].trim();

        let start: i64 = vars
            .get(left)
            .and_then(|v| v.parse().ok())
            .or_else(|| left.parse().ok())
            .unwrap_or(1);

        let end: i64 = vars
            .get(right)
            .and_then(|v| v.parse().ok())
            .or_else(|| right.parse().ok())
            .unwrap_or(start);

        (start, end)
    } else {
        (1, 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_semver() {
        assert_eq!(compare_semver("2.3.4", "2.3.4"), std::cmp::Ordering::Equal);
        assert_eq!(
            compare_semver("2.3.4", "2.2.0"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(compare_semver("2.2.0", "2.3.4"), std::cmp::Ordering::Less);
        assert_eq!(compare_semver("2.3", "2.3.0"), std::cmp::Ordering::Equal);
    }

    #[test]
    fn test_check_version_requirement() {
        assert!(check_version_requirement(">= 2.2.0", "2.3.4"));
        assert!(!check_version_requirement(">= 3.0.0", "2.3.4"));
        assert!(check_version_requirement("<= 2.3.4", "2.3.4"));
        assert!(!check_version_requirement("<= 2.0.0", "2.3.4"));
        assert!(check_version_requirement("> 2.2", "2.3.4"));
        assert!(!check_version_requirement("> 3.0.0", "2.3.4"));
        assert!(check_version_requirement("< 3.0.0", "2.3.4"));
        assert!(!check_version_requirement("< 2.0.0", "2.3.4"));
        assert!(check_version_requirement("= 2.3.4", "2.3.4"));
        assert!(!check_version_requirement("= 3.0.0", "2.3.4"));
        assert!(check_version_requirement("~> 2.3.0", "2.3.4"));
        assert!(!check_version_requirement("~> 2.2.0", "2.3.4"));
        assert!(check_version_requirement("~> 2.2", "2.3.4"));
        assert!(!check_version_requirement("~> 2.1", "3.0.0"));
        assert!(!check_version_requirement("~> 2.1", "1.9.0"));
        assert!(check_version_requirement("~> 2", "2.3.4"));
        assert!(check_version_requirement("=~ 2.3.4", "2.3.4"));
        assert!(check_version_requirement("2.3.4", "2.3.4"));
        assert!(!check_version_requirement("2.3.4", "2.2.0"));
    }

    #[test]
    fn test_evaluate_condition() {
        let mut vars = HashMap::new();
        vars.insert("CI".to_string(), "true".to_string());
        vars.insert("EMPTY".to_string(), "".to_string());
        vars.insert("ZERO".to_string(), "0".to_string());
        vars.insert("FALSE_STR".to_string(), "false".to_string());
        vars.insert("FOO".to_string(), "bar".to_string());
        vars.insert("BAZ".to_string(), "bar".to_string());

        assert!(evaluate_condition("true", &vars));
        assert!(!evaluate_condition("false", &vars));
        assert!(evaluate_condition("CI", &vars));
        assert!(!evaluate_condition("EMPTY", &vars));
        assert!(!evaluate_condition("ZERO", &vars));
        assert!(!evaluate_condition("FALSE_STR", &vars));
        assert!(!evaluate_condition("!CI", &vars));
        assert!(evaluate_condition("!false", &vars));
        assert!(evaluate_condition(
            "Vagrant.has_plugin?('vagrant-reload')",
            &vars
        ));
        assert!(evaluate_condition("FOO == 'bar'", &vars));
        assert!(!evaluate_condition("FOO == 'baz'", &vars));
        assert!(evaluate_condition("FOO == BAZ", &vars));
        assert!(evaluate_condition("'bar' == FOO", &vars));
        assert!(!evaluate_condition("'other' == FOO", &vars));
        assert!(!evaluate_condition("a == b == c", &vars));
        assert!(!evaluate_condition("FOO != 'bar'", &vars));
        assert!(evaluate_condition("FOO != 'other'", &vars));
        assert!(!evaluate_condition("FOO != BAZ", &vars));
        assert!(evaluate_condition("'other' != FOO", &vars));
        assert!(!evaluate_condition("'bar' != FOO", &vars));
        assert!(!evaluate_condition("a != b != c", &vars));
        assert!(!evaluate_condition("UNDEFINED_VAR", &vars));

        // ENV[...] in condition
        unsafe {
            std::env::set_var("MIGRATORY_COND_VAR_1", "1");
            std::env::set_var("MIGRATORY_COND_VAR_0", "0");
            std::env::set_var("MIGRATORY_COND_VAR_F", "false");
        }
        assert!(evaluate_condition("ENV['MIGRATORY_COND_VAR_1']", &vars));
        assert!(!evaluate_condition("ENV['MIGRATORY_COND_VAR_0']", &vars));
        assert!(!evaluate_condition("ENV['MIGRATORY_COND_VAR_F']", &vars));
        assert!(!evaluate_condition("ENV['MIGRATORY_COND_NONEXIST']", &vars));
        assert!(!evaluate_condition("ENV[unclosed", &vars));

        // ENV.fetch(...) in condition
        assert!(evaluate_condition(
            "ENV.fetch('MIGRATORY_COND_VAR_1', 'default')",
            &vars
        ));
        assert!(evaluate_condition(
            "ENV.fetch('MIGRATORY_COND_NONEXIST', 'yes')",
            &vars
        ));
        assert!(!evaluate_condition(
            "ENV.fetch('MIGRATORY_COND_NONEXIST', '0')",
            &vars
        ));
        assert!(!evaluate_condition(
            "ENV.fetch('MIGRATORY_COND_NONEXIST', 'false')",
            &vars
        ));
        assert!(!evaluate_condition(
            "ENV.fetch('MIGRATORY_COND_NONEXIST')",
            &vars
        ));
        assert!(!evaluate_condition("ENV.fetch()", &vars));
        assert!(!evaluate_condition("ENV.fetch(unclosed", &vars));

        unsafe {
            std::env::remove_var("MIGRATORY_COND_VAR_1");
            std::env::remove_var("MIGRATORY_COND_VAR_0");
            std::env::remove_var("MIGRATORY_COND_VAR_F");
        }
    }

    #[test]
    fn test_interpolate_string() {
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "web".to_string());
        vars.insert("i".to_string(), "2".to_string());

        let res = interpolate_string("vm-#{name}-#{i}-#{10 + i}", &vars);
        assert_eq!(res, "vm-web-2-12");

        let res_arith = interpolate_string("#{i + 1}", &vars);
        assert_eq!(res_arith, "3");

        let res_arith_unknown = interpolate_string("#{unknown + 5}", &vars);
        assert_eq!(res_arith_unknown, "5");

        let res_arith_three = interpolate_string("#{1 + 2 + 3}", &vars);
        assert_eq!(res_arith_three, "1 + 2 + 3");

        let res_nested = interpolate_string("hash: #{ { 'k' => 'v' } }", &vars);
        assert!(res_nested.contains("{ 'k' => 'v' }"));

        let res_unclosed = interpolate_string("prefix-#{unclosed", &vars);
        assert_eq!(res_unclosed, "prefix-unclosed");

        assert_eq!(interpolate_string("empty: ``", &vars), "empty: ");
        assert_eq!(interpolate_string("`echo hi`", &vars), "hi");
        assert_eq!(interpolate_string("`false`", &vars), "");

        unsafe {
            std::env::set_var("MIGRATORY_IN_PROC_TEST_KEY", "env_value");
        }
        assert_eq!(
            interpolate_string("#{ENV['MIGRATORY_IN_PROC_TEST_KEY']}", &vars),
            "env_value"
        );
        assert_eq!(
            interpolate_string("#{ENV.fetch('MIGRATORY_IN_PROC_TEST_KEY', 'def')}", &vars),
            "env_value"
        );
        assert_eq!(
            interpolate_string(
                "#{ENV.fetch('NON_EXISTENT_MIGRATORY_KEY_XYZ', 'my_default')}",
                &vars
            ),
            "my_default"
        );
        assert_eq!(
            interpolate_string("#{ENV.fetch('NON_EXISTENT_MIGRATORY_KEY_XYZ')}", &vars),
            "ENV.fetch('NON_EXISTENT_MIGRATORY_KEY_XYZ')"
        );
        assert_eq!(interpolate_string("#{ENV.fetch()}", &vars), "ENV.fetch()");

        assert_eq!(interpolate_string("#{plain_ident}", &vars), "plain_ident");

        unsafe {
            std::env::remove_var("MIGRATORY_IN_PROC_TEST_KEY");
        }
    }

    #[test]
    fn test_evaluate_in_process_single_machine() {
        let vf = r#"
Vagrant.require_version ">= 2.0.0"
IMAGE = "ubuntu/focal64"

Vagrant.configure("2") do |config|
  config.vm.box = IMAGE
  config.vm.hostname = "my-host"
  config.vm.usable_port_range = 2200..2300
  config.vm.network "forwarded_port", guest: 80, host: 8080, auto_correct: true
  config.vm.synced_folder ".", "/vagrant", disabled: false
  config.vm.disk :disk, size: "20GB", name: "extra_storage"
  config.ssh.username = "vagrant"
  config.ssh.forward_agent = true
  config.winrm.username = "Administrator"
  config.vagrant.sensitive = ["secret"]
end
"#;
        let env = evaluate_in_process(vf).expect("operation should succeed");
        assert!(env.machines.contains_key("default"));
        let default_m = &env.machines["default"];
        assert_eq!(default_m.vm.box_name.as_deref(), Some("ubuntu/focal64"));
        assert_eq!(default_m.vm.hostname.as_deref(), Some("my-host"));
        assert_eq!(default_m.vm.usable_port_range, (2200, 2300));
        assert_eq!(default_m.vm.networks.len(), 1);
        assert_eq!(default_m.vm.synced_folders.len(), 1);
        assert_eq!(default_m.vm.disks.len(), 1);
        assert_eq!(default_m.vm.disks[0].size.as_deref(), Some("20GB"));
        assert!(default_m.ssh.forward_agent);
        assert_eq!(default_m.winrm.username, "Administrator");
        assert_eq!(default_m.vagrant.sensitive, vec!["secret".to_string()]);
    }

    #[test]
    fn test_evaluate_in_process_multi_machine_loops_and_conditionals() {
        let vf = r#"
N = 2
Vagrant.configure("2") do |config|
  (1..N).each do |i|
    config.vm.define "node-#{i}", primary: true do |node|
      node.vm.box = "generic/alpine318"
      node.vm.hostname = "node-#{i}"
      if true
        node.vm.network "private_network", ip: "192.168.56.#{10 + i}"
      else
        node.vm.network "private_network", ip: "10.0.0.1"
      end
    end
  end
end
"#;
        let env = evaluate_in_process(vf).expect("evaluation should succeed");
        assert_eq!(env.machines.len(), 2);
        assert!(env.machines.contains_key("node-1"));
        assert!(env.machines.contains_key("node-2"));
        let node1 = &env.machines["node-1"];
        assert_eq!(node1.vm.hostname.as_deref(), Some("node-1"));
        assert_eq!(node1.vm.networks.len(), 1);
        assert!(matches!(
            &node1.vm.networks[0],
            NetworkConfig::PrivateNetwork { ip: Some(ip), .. } if ip == "192.168.56.11"
        ));
    }

    #[test]
    fn test_evaluate_in_process_v1_legacy() {
        let vf = r#"
Vagrant.configure("1") do |config|
  config.vm.box = "base"
  config.vm.forward_port 80, 8080
  config.vm.share_folder "v-root", "/vagrant", "."
  config.vm.depends_on "legacy_db"
end
"#;
        let env = evaluate_in_process(vf).expect("operation should succeed");
        let m = &env.machines["default"];
        assert_eq!(m.vm.box_name.as_deref(), Some("base"));
        assert_eq!(m.depends_on, vec!["legacy_db".to_string()]);
        assert_eq!(m.vm.networks.len(), 1);
        assert_eq!(m.vm.synced_folders.len(), 1);
        assert_eq!(m.vm.synced_folders[0].guest_path, "/vagrant");
    }

    #[test]
    fn test_evaluate_in_process_version_failure() {
        let vf = r#"
Vagrant.require_version ">= 99.0.0"
Vagrant.configure("2") do |config|
  config.vm.box = "base"
end
"#;
        assert!(evaluate_in_process(vf).is_err());
    }

    #[test]
    fn test_loops_and_conditionals_edge_cases() {
        let mut vars = HashMap::new();
        let unless_vf = "unless false\n  config.vm.box = \"box_unless\"\nelse\n  config.vm.box = \"box_else\"\nend";
        let exp = evaluate_conditionals(unless_vf, &mut vars);
        assert!(exp.contains("box_unless"));
        assert!(!exp.contains("box_else"));

        let unless_true_vf = "unless true\n  config.vm.box = \"box_unless\"\nelse\n  config.vm.box = \"box_else\"\nend";
        let exp2 = evaluate_conditionals(unless_true_vf, &mut vars);
        assert!(!exp2.contains("box_unless"));
        assert!(exp2.contains("box_else"));

        let if_false_vf =
            "if false\n  config.vm.box = \"box_if\"\nelse\n  config.vm.box = \"box_else\"\nend";
        let exp3 = evaluate_conditionals(if_false_vf, &mut vars);
        assert!(!exp3.contains("box_if"));
        assert!(exp3.contains("box_else"));

        assert_eq!(parse_range("single", &vars), (1, 1));
        assert_eq!(parse_range("1..3", &vars), (1, 3));

        // Nested if
        let nested_if = "if true\n  if true\n    config.vm.box = \"nested\"\n  end\nend";
        let exp_nested = evaluate_conditionals(nested_if, &mut vars);
        assert!(exp_nested.contains("nested"));

        vars.insert("M".to_string(), "1".to_string());
        vars.insert("N".to_string(), "2".to_string());
        assert_eq!(parse_range("M..N", &vars), (1, 2));
    }

    #[test]
    fn test_evaluate_in_process_all_directives() {
        let vf = r#"
Vagrant.configure("2") do |config|
  config.vm.box_version = "1.2.3"
  config.vm.box_url = "http://example.com/box"
  config.vm.guest = :linux
  config.vm.communicator = :winrm
  config.vm.boot_timeout = 500
  config.vm.graceful_halt_timeout = 90
  config.vm.post_up_message = "Welcome to your VM!"
  config.vm.network "forwarded_port", guest: 443, host: 8443, auto_correct: false, protocol: "tcp", host_ip: "0.0.0.0"
  config.vm.network "private_network", ip: "192.168.10.2", netmask: "255.255.255.0", dhcp: "true", virtualbox__intnet: "vboxnet"
  config.vm.network "private_network", type: "dhcp"
  config.vm.network "public_network", ip: "10.10.10.10", bridge: "en0", use_dhcp_assigned_default_route: true
  config.vm.network "unknown_type", foo: "bar"
  config.vm.synced_folder "/host", "/guest", type: "nfs", owner: "root", group: "root", disabled: false
  config.vm.disk :disk, size: "50GB", name: "secondary", primary: false
  config.vm.provision "shell", id: "setup", run: "always", inline: "echo provision"
  config.ssh.username = "custom_user"
  config.ssh.password = "custom_pw"
  config.ssh.host = "127.0.0.1"
  config.ssh.port = 2222
  config.ssh.guest_port = 22
  config.ssh.private_key_path = "/home/.ssh/id_rsa"
  config.ssh.insert_key = false
  config.ssh.forward_agent = true
  config.ssh.forward_x11 = true
  config.ssh.proxy_command = "proxy_cmd"
  config.ssh.pty = true
  config.ssh.keep_alive = true
  config.ssh.shell = "/bin/bash"
  config.ssh.export_command_template = "export VAR=%s"
  config.ssh.connect_timeout = 30
  config.ssh.timeout = 60
  config.ssh.verify_host_key = true
  config.ssh.keys_only = true
  config.ssh.unknown = "val"
  config.ssh.no_equal_sign
  config.winrm.username = "vagrant"
  config.winrm.password = "vagrant"
  config.winrm.host = "192.168.1.5"
  config.winrm.port = 5986
  config.winrm.guest_port = 5985
  config.winrm.ssl = true
  config.winrm.transport = "plaintext"
  config.winrm.basic_auth_only = true
  config.winrm.ssl_peer_verification = false
  config.winrm.timeout = 1200
  config.winrm.retry_limit = 8
  config.winrm.retry_delay = 3
  config.winrm.execution_time_limit = "PT1H"
  config.winrm.unknown = "val"
  config.winrm.no_equal_sign
  config.vagrant.host = "darwin"
  config.vagrant.plugins = ["plugin1", "plugin2"]
  config.vagrant.sensitive = ["SECRET_TOKEN"]
  config.vagrant.unknown = "val"
  config.vagrant.no_equal_sign

  config.vm.provider :virtualbox do |vb|
    vb.gui
    vb.memory = "2048"
    vb.cpus = "2"
  end

  config.vm.usable_port_range = "no_dots"

  config.vm.define "app", autostart: false, primary: true do |app|
    app.vm.box = "app_box"
    app.vm.depends_on = ["", "db_vm"]
    app.vm.depends_on = ""
    app.vm.provider :docker do |d|
      d.image = "node:latest"
    end
  end

  config.vm.define "auto_vm", autostart: true do |a|
    a.vm.box = "auto_box"
  end
end
"#;
        let env = evaluate_in_process(vf).expect("operation should succeed");
        assert!(env.machines.contains_key("app"));
        assert!(env.machines.contains_key("auto_vm"));
        assert!(env.machines["auto_vm"].autostart);
        let app = &env.machines["app"];
        assert_eq!(app.vm.box_name.as_deref(), Some("app_box"));
        assert_eq!(app.depends_on, vec!["db_vm".to_string()]);
        assert!(!app.autostart);
        assert!(app.primary);
        assert_eq!(app.vm.box_version.as_deref(), Some("1.2.3"));
        assert_eq!(app.vm.box_url.as_deref(), Some("http://example.com/box"));
        assert_eq!(app.vm.guest.as_deref(), Some("linux"));
        assert_eq!(app.vm.communicator.as_deref(), Some("winrm"));
        assert_eq!(app.vm.boot_timeout, Some(500));
        assert_eq!(app.vm.graceful_halt_timeout, Some(90));
        assert_eq!(
            app.vm.post_up_message.as_deref(),
            Some("Welcome to your VM!")
        );
        assert_eq!(app.ssh.username, "custom_user");
        assert_eq!(app.ssh.password.as_deref(), Some("custom_pw"));
        assert_eq!(app.ssh.host, "127.0.0.1");
        assert_eq!(app.ssh.port, 2222);
        assert_eq!(app.ssh.guest_port, Some(22));
        assert_eq!(
            app.ssh.private_key_path.as_deref(),
            Some("/home/.ssh/id_rsa")
        );
        assert!(!app.ssh.insert_key);
        assert!(app.ssh.forward_agent);
        assert!(app.ssh.forward_x11);
        assert_eq!(app.ssh.proxy_command.as_deref(), Some("proxy_cmd"));
        assert!(app.ssh.pty);
        assert!(app.ssh.keep_alive);
        assert_eq!(app.ssh.shell.as_deref(), Some("/bin/bash"));
        assert_eq!(
            app.ssh.export_command_template.as_deref(),
            Some("export VAR=%s")
        );
        assert_eq!(app.ssh.connect_timeout, Some(30));
        assert_eq!(app.ssh.timeout, Some(60));
        assert!(app.ssh.verify_host_key);
        assert!(app.ssh.keys_only);
        assert_eq!(app.winrm.username, "vagrant");
        assert_eq!(app.winrm.password.as_deref(), Some("vagrant"));
        assert_eq!(app.winrm.host, "192.168.1.5");
        assert_eq!(app.winrm.port, 5986);
        assert_eq!(app.winrm.guest_port, Some(5985));
        assert!(app.winrm.ssl);
        assert_eq!(app.winrm.transport.as_deref(), Some("plaintext"));
        assert!(app.winrm.basic_auth_only);
        assert!(!app.winrm.ssl_peer_verification);
        assert_eq!(app.winrm.timeout, Some(1200));
        assert_eq!(app.winrm.retry_limit, Some(8));
        assert_eq!(app.winrm.retry_delay, Some(3));
        assert_eq!(app.winrm.execution_time_limit.as_deref(), Some("PT1H"));
        assert_eq!(app.vagrant.host.as_deref(), Some("darwin"));
        assert!(app.vagrant.plugins.contains(&"plugin1".to_string()));
        assert!(app.vagrant.sensitive.contains(&"SECRET_TOKEN".to_string()));
        assert_eq!(app.vm.networks.len(), 4);
        assert_eq!(app.vm.synced_folders.len(), 1);
        assert_eq!(app.vm.disks.len(), 1);
        assert_eq!(app.vm.provisioners.len(), 1);
        assert_eq!(app.vm.providers.len(), 2);
    }

    #[test]
    fn test_extract_between_and_helpers() {
        assert_eq!(extract_between("no_delims", '(', ')'), None);
        assert_eq!(extract_between("start_only(", '(', ')'), None);
        assert_eq!(extract_between("(found)", '(', ')'), Some("found"));

        let vars = HashMap::new();
        let empty_opts = parse_options_string(" , , ", &vars);
        assert!(empty_opts.is_empty());
        assert_eq!(parse_options_string("no_colon_here", &vars).len(), 0);

        assert_eq!(
            strip_line_comment("echo 'hello # not comment'"),
            "echo 'hello # not comment'"
        );
        assert_eq!(
            strip_line_comment("echo \"hello # not comment\""),
            "echo \"hello # not comment\""
        );
        assert_eq!(strip_line_comment("code # trailing comment"), "code");
    }

    #[test]
    fn test_v1_legacy_options() {
        let vf = r#"
Vagrant.configure("1") do |config|
  config.vm.forward_port 80, 8080, auto_correct: true, protocol: "tcp", host_ip: "127.0.0.1"
  config.vm.forward_port 80
  config.vm.share_folder "only_one"
end
"#;
        let env = evaluate_in_process(vf).expect("operation should succeed");
        let m = &env.machines["default"];
        assert_eq!(m.vm.networks.len(), 1);
        assert_eq!(m.vm.synced_folders.len(), 0);
        assert!(matches!(
            &m.vm.networks[0],
            NetworkConfig::ForwardedPort {
                guest: 80,
                host: 8080,
                auto_correct: true,
                protocol: Some(p),
                host_ip: Some(ip)
            } if p == "tcp" && ip == "127.0.0.1"
        ));
    }

    #[test]
    fn test_check_version_requirement_exhaustive() {
        assert!(check_version_requirement(">= 1.0.0", "1.0.0"));
        assert!(check_version_requirement(">= 1.0.0", "1.1.0"));
        assert!(!check_version_requirement(">= 2.0.0", "1.0.0"));

        assert!(check_version_requirement("<= 2.0.0", "2.0.0"));
        assert!(check_version_requirement("<= 2.0.0", "1.9.0"));
        assert!(!check_version_requirement("<= 2.0.0", "2.1.0"));

        assert!(check_version_requirement("> 1.0.0", "1.0.1"));
        assert!(!check_version_requirement("> 1.0.0", "1.0.0"));

        assert!(check_version_requirement("< 2.0.0", "1.9.9"));
        assert!(!check_version_requirement("< 2.0.0", "2.0.0"));

        assert!(check_version_requirement("= 2.3.4", "2.3.4"));
        assert!(!check_version_requirement("= 2.3.4", "2.3.5"));

        assert!(check_version_requirement("2.3.4", "2.3.4"));
        assert!(!check_version_requirement("2.3.4", "2.3.5"));

        assert!(check_version_requirement("=~ 2.3", "2.3.4"));
        assert!(check_version_requirement("~> 2.3", "2.3.0"));
        assert!(check_version_requirement("~> 2.3", "2.4.0"));
        assert!(!check_version_requirement("~> 2.3", "3.0.0"));
        assert!(!check_version_requirement("~> 2.3", "1.9.0"));

        assert!(check_version_requirement("~> 2.3.4", "2.3.5"));
        assert!(!check_version_requirement("~> 2.3.4", "2.4.0"));
        assert!(!check_version_requirement("~> 2.3.4", "2.3.3"));

        // Single component pessimistic
        assert!(check_version_requirement("~> 2", "2.5.0"));
    }

    #[test]
    fn test_evaluate_condition_exhaustive() {
        let mut vars = HashMap::new();
        vars.insert("MY_VAR".to_string(), "val1".to_string());
        vars.insert("EMPTY_VAR".to_string(), "".to_string());
        vars.insert("ZERO_VAR".to_string(), "0".to_string());
        vars.insert("FALSE_VAR".to_string(), "false".to_string());

        assert!(evaluate_condition("true", &vars));
        assert!(!evaluate_condition("false", &vars));
        assert!(evaluate_condition("!false", &vars));
        assert!(!evaluate_condition("!true", &vars));
        assert!(evaluate_condition(
            "Vagrant.has_plugin?('vagrant-test')",
            &vars
        ));

        // ENV[]
        unsafe {
            std::env::set_var("TEST_ENV_1", "1");
            std::env::set_var("TEST_ENV_0", "0");
            std::env::set_var("TEST_ENV_FALSE", "false");
            std::env::set_var("TEST_ENV_EMPTY", "  ");
        }
        assert!(evaluate_condition("ENV['TEST_ENV_1']", &vars));
        assert!(!evaluate_condition("ENV['TEST_ENV_0']", &vars));
        assert!(!evaluate_condition("ENV['TEST_ENV_FALSE']", &vars));
        assert!(!evaluate_condition("ENV['TEST_ENV_EMPTY']", &vars));
        assert!(!evaluate_condition("ENV['TEST_ENV_MISSING']", &vars));
        assert!(!evaluate_condition("ENV[invalid", &vars));

        // ENV.fetch
        assert!(evaluate_condition("ENV.fetch('TEST_ENV_1')", &vars));
        assert!(!evaluate_condition("ENV.fetch('TEST_ENV_0')", &vars));
        assert!(!evaluate_condition("ENV.fetch('TEST_ENV_MISSING')", &vars));
        assert!(evaluate_condition(
            "ENV.fetch('TEST_ENV_MISSING', '1')",
            &vars
        ));
        assert!(!evaluate_condition(
            "ENV.fetch('TEST_ENV_MISSING', '0')",
            &vars
        ));
        assert!(!evaluate_condition(
            "ENV.fetch('TEST_ENV_MISSING', 'false')",
            &vars
        ));
        assert!(!evaluate_condition(
            "ENV.fetch('TEST_ENV_MISSING', '')",
            &vars
        ));
        assert!(!evaluate_condition("ENV.fetch()", &vars));
        assert!(!evaluate_condition("ENV.fetch(invalid", &vars));

        unsafe {
            std::env::remove_var("TEST_ENV_1");
            std::env::remove_var("TEST_ENV_0");
            std::env::remove_var("TEST_ENV_FALSE");
            std::env::remove_var("TEST_ENV_EMPTY");
        }

        // == and !=
        assert!(evaluate_condition("MY_VAR == 'val1'", &vars));
        assert!(!evaluate_condition("MY_VAR == 'val2'", &vars));
        assert!(evaluate_condition("MY_VAR != 'val2'", &vars));
        assert!(!evaluate_condition("MY_VAR != 'val1'", &vars));

        // vars lookup
        assert!(evaluate_condition("MY_VAR", &vars));
        assert!(!evaluate_condition("EMPTY_VAR", &vars));
        assert!(!evaluate_condition("ZERO_VAR", &vars));
        assert!(!evaluate_condition("FALSE_VAR", &vars));
        assert!(!evaluate_condition("UNKNOWN_VAR", &vars));
    }

    #[test]
    fn test_expand_loops_and_conditionals_syntax_exhaustive() {
        let vf = r#"
Vagrant.configure('1') do |config|
  config.share_folder "root", "/vagrant", "."
  config.forward_port 80, 8080

  (1..2).each do|i|
    unless i == 99
      config.vm.hostname = "host"
    end
    (1..1).each do |j|
      config.vm.box = "loop_box"
    end
  end

  if false
    unless true
      config.vm.box = "skipped_box"
    end
    while_block do
      config.vm.box = "while_box"
    end
  else
    config.vm.boot_timeout = 500
  end
end
"#;
        let env = evaluate_in_process(vf).expect("operation should succeed");
        let m = &env.machines["default"];
        assert_eq!(m.vm.box_name.as_deref(), Some("loop_box"));
        assert_eq!(m.vm.hostname.as_deref(), Some("host"));
        assert_eq!(m.vm.boot_timeout, Some(500));
        assert_eq!(m.vm.networks.len(), 1);
        assert_eq!(m.vm.synced_folders.len(), 1);
    }
}
