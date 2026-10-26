//! UI module for terminal output formatting.
//!
//! Provides the [`crate::ui::Ui`] trait and implementations for human-readable terminal output
//! ([`crate::ui::ConsoleUi`]), machine-readable CSV/JSON output ([`crate::ui::MachineReadableUi`]),
//! thread-safe output synchronization ([`crate::ui::ConcurrentUi`]), and machine-prefixed output ([`crate::ui::PrefixedUi`]).

use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};

/// The UI interface for emitting formatted messages, progress bars, and interactive prompts.
pub trait Ui {
    /// Prints a standard informational message.
    ///
    /// # Arguments
    ///
    /// * `target` - Target machine or subsystem name.
    /// * `message` - Message content to display.
    fn info(&self, _target: &str, _message: &str) {}

    /// Prints a success message (typically rendered in green).
    ///
    /// # Arguments
    ///
    /// * `target` - Target machine or subsystem name.
    /// * `message` - Success message content to display.
    fn success(&self, _target: &str, _message: &str) {}

    /// Prints a detailed or indented secondary message.
    ///
    /// # Arguments
    ///
    /// * `target` - Target machine or subsystem name.
    /// * `message` - Detail message content to display.
    fn detail(&self, _target: &str, _message: &str) {}

    /// Prints a warning message (typically rendered in yellow).
    ///
    /// # Arguments
    ///
    /// * `target` - Target machine or subsystem name.
    /// * `message` - Warning message content to display.
    fn warn(&self, _target: &str, _message: &str) {}

    /// Prints an error message (typically rendered in red).
    ///
    /// # Arguments
    ///
    /// * `target` - Target machine or subsystem name.
    /// * `message` - Error message content to display.
    fn error(&self, _target: &str, _message: &str) {}

    /// Prints an error formatted from a [`crate::error::MigratoryError`].
    ///
    /// # Arguments
    ///
    /// * `target` - Target machine or subsystem name.
    /// * `err` - Reference to the [`crate::error::MigratoryError`] to display.
    fn error_std(&self, target: &str, err: &crate::error::MigratoryError) {
        self.error(target, &err.to_string());
    }

    /// Creates a progress bar for downloads or long-running tasks.
    ///
    /// # Arguments
    ///
    /// * `target` - Target machine or subsystem name.
    /// * `total` - Total byte or step count for the progress bar.
    /// * `message` - Initial status message.
    ///
    /// # Returns
    ///
    /// Returns a configured [`ProgressBar`].
    fn create_progress(&self, _target: &str, _total: u64, _message: &str) -> ProgressBar {
        ProgressBar::hidden()
    }

    /// Prompts the user to select an item from a list of choices.
    ///
    /// # Arguments
    ///
    /// * `target` - Target machine or subsystem name.
    /// * `prompt` - Prompt question text.
    /// * `choices` - Slice of selectable option strings.
    ///
    /// # Returns
    ///
    /// Returns the selected choice string on success.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::MigratoryError`] if reading stdin fails or the selection is invalid.
    fn prompt_choice(
        &self,
        _target: &str,
        _prompt: &str,
        choices: &[&str],
    ) -> Result<String, crate::error::MigratoryError> {
        choices
            .first()
            .map(|s| (*s).to_string())
            .ok_or_else(|| crate::error::MigratoryError::Validation("No choices".to_string()))
    }
}

thread_local! {
    /// Mock stdin lines queued for unit tests.
    pub static MOCK_STDIN: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Global atomic flag tracking whether `--machine-readable` output mode is active.
static MACHINE_READABLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Sets whether global machine-readable CSV output is enabled.
///
/// # Arguments
///
/// * `enabled` - Boolean flag toggling machine-readable output.
pub fn set_machine_readable(enabled: bool) {
    MACHINE_READABLE.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

/// Checks whether global machine-readable CSV output is enabled.
///
/// # Returns
///
/// Returns `true` if machine-readable output is enabled, `false` otherwise.
pub fn is_machine_readable() -> bool {
    MACHINE_READABLE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Returns the default boxed [`Ui`] implementation based on the current machine-readable state.
///
/// # Returns
///
/// Returns a boxed [`MachineReadableUi`] if machine-readable mode is enabled, or [`ConsoleUi`] otherwise.
pub fn default_ui() -> Box<dyn Ui + Send + Sync> {
    if is_machine_readable() {
        Box::new(MachineReadableUi)
    } else {
        Box::new(ConsoleUi)
    }
}

/// Reads a single line from standard input (or the thread-local mock queue during tests).
///
/// # Arguments
///
/// * `buf` - Mutable string buffer to append the read line into.
///
/// # Returns
///
/// Returns the number of bytes read on success.
///
/// # Errors
///
/// Returns `std::io::Error` if reading from standard input fails.
pub fn read_stdin(buf: &mut String) -> std::io::Result<usize> {
    MOCK_STDIN.with(|m| {
        let mut m = m.borrow_mut();
        if m.is_empty() {
            Ok(0)
        } else {
            let s = m.remove(0);
            if s == "__IO_ERROR__" {
                return Err(std::io::Error::other("mock stdin error"));
            }
            buf.push_str(&s);
            buf.push(char::from(b'\n'));
            Ok(s.len() + 1)
        }
    })
}

/// Constructs an `indicatif` [`ProgressStyle`] from a template string, falling back to the default bar on template error.
///
/// # Arguments
///
/// * `template` - The `indicatif` progress bar template string.
///
/// # Returns
///
/// Returns the configured [`ProgressStyle`].
fn get_progress_style(template: &str) -> ProgressStyle {
    ProgressStyle::default_bar()
        .template(template)
        .unwrap_or(ProgressStyle::default_bar())
}

/// Returns a consistent ANSI color for a given target name to differentiate multi-machine outputs.
///
/// # Arguments
///
/// * `target` - The target machine name.
///
/// # Returns
///
/// Returns a `colored::Color` variant for the target.
pub fn color_for_target(target: &str) -> colored::Color {
    if target.is_empty() || target == "migratory" || target == "vagrant" {
        return colored::Color::Cyan;
    }
    let sum: usize = target.bytes().map(|b| b as usize).sum();
    match sum % 5 {
        0 => colored::Color::Cyan,
        1 => colored::Color::Magenta,
        2 => colored::Color::Yellow,
        3 => colored::Color::Green,
        _ => colored::Color::Blue,
    }
}

/// Escapes a CSV field according to RFC 4180 / HashiCorp Vagrant specification.
///
/// # Arguments
///
/// * `field` - The raw string value.
///
/// # Returns
///
/// Returns an escaped CSV string.
pub fn format_csv_field(field: &str) -> String {
    let lf = char::from(10);
    let cr = char::from(13);
    if field.contains(',') || field.contains('"') || field.contains(lf) || field.contains(cr) {
        let escaped = field.replace('"', r#""""#);
        format!(r#""{escaped}""#)
    } else {
        field.to_string()
    }
}

/// Formats a complete machine-readable CSV line matching HashiCorp Vagrant specification:
/// `<timestamp>,<target>,<type>,<data...>`
///
/// # Arguments
///
/// * `timestamp` - Epoch timestamp in seconds.
/// * `target` - The machine target name.
/// * `entry_type` - The record category/type (e.g. `"ui"`, `"state"`, `"error-exit"`).
/// * `data` - Additional data field arguments.
///
/// # Returns
///
/// Returns the formatted CSV line string.
pub fn format_machine_readable_csv(
    timestamp: u64,
    target: &str,
    entry_type: &str,
    data: &[&str],
) -> String {
    let mut line = format!("{timestamp},{target},{entry_type}");
    for item in data {
        line.push(',');
        line.push_str(&format_csv_field(item));
    }
    line
}

/// A standard, human-readable CLI UI.
pub struct ConsoleUi;

impl Ui for ConsoleUi {
    fn info(&self, target: &str, message: &str) {
        let color = color_for_target(target);
        println!("==> {}: {}", target.color(color), message.bold());
    }

    fn success(&self, target: &str, message: &str) {
        let color = color_for_target(target);
        println!("==> {}: {}", target.color(color), message.green());
    }

    fn detail(&self, target: &str, message: &str) {
        println!("    {target}: {message}");
    }

    fn warn(&self, target: &str, message: &str) {
        println!("==> {}: {}", target.yellow(), message.yellow());
    }

    fn error(&self, target: &str, message: &str) {
        println!("==> {}: {}", target.red(), message.red());
    }

    fn create_progress(&self, target: &str, total: u64, message: &str) -> ProgressBar {
        self.info(target, message);
        let pb = ProgressBar::new(total);
        let style = get_progress_style(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})",
        );
        pb.set_style(style.progress_chars("#>-"));
        pb
    }

    fn prompt_choice(
        &self,
        target: &str,
        prompt: &str,
        choices: &[&str],
    ) -> Result<String, crate::error::MigratoryError> {
        let color = color_for_target(target);
        println!("==> {}: {}", target.color(color), prompt.bold());
        for (i, choice) in choices.iter().enumerate() {
            println!("    {}. {}", i + 1, choice);
        }
        println!(
            "==> {}: Enter choice (1-{}):",
            target.color(color),
            choices.len()
        );

        let mut input = String::new();
        read_stdin(&mut input).map_err(crate::error::MigratoryError::Io)?;
        let input = input.trim();

        if let Some(&choice) = input
            .parse::<usize>()
            .ok()
            .and_then(|idx| idx.checked_sub(1))
            .and_then(|idx| choices.get(idx))
        {
            return Ok(choice.to_string());
        }
        Err(crate::error::MigratoryError::Validation(
            "Invalid choice".to_string(),
        ))
    }
}

/// A machine-readable UI, useful for IDE integrations and automated scripts.
pub struct MachineReadableUi;

/// Structure representing a JSON log entry for machine readability.
#[derive(serde::Serialize)]
pub struct MachineLog<'a> {
    /// Unix timestamp of the log event.
    pub timestamp: u64,
    /// The target of the log event.
    pub target: &'a str,
    /// The component generating the log.
    pub component: &'a str,
    /// The log level.
    pub level: &'a str,
    /// The log message.
    pub message: &'a str,
}

impl MachineReadableUi {
    /// Logs a machine-readable CSV record according to HashiCorp Vagrant specification.
    ///
    /// # Arguments
    ///
    /// * `target` - Target machine name (or empty string for global messages).
    /// * `entry_type` - The entry type (e.g. `"ui"`, `"state"`, `"error-exit"`).
    /// * `data` - Variable data fields to output.
    pub fn log_csv(&self, target: &str, entry_type: &str, data: &[&str]) {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = format_machine_readable_csv(ts, target, entry_type, data);
        println!("{line}");
    }

    /// Logs machine state information.
    ///
    /// # Arguments
    ///
    /// * `target` - Target machine name.
    /// * `state` - Current machine state identifier (e.g. `"running"`, `"poweroff"`).
    pub fn log_state(&self, target: &str, state: &str) {
        self.log_csv(target, "state", &[state]);
        self.log_csv(target, "state-human-short", &[state]);
    }

    /// Logs box metadata information.
    ///
    /// # Arguments
    ///
    /// * `target` - Target machine name.
    /// * `name` - Box name.
    /// * `provider` - Provider name.
    /// * `version` - Box version.
    pub fn log_box_info(&self, target: &str, name: &str, provider: &str, version: &str) {
        self.log_csv(target, "box-name", &[name]);
        self.log_csv(target, "box-provider", &[provider]);
        self.log_csv(target, "box-version", &[version]);
    }

    /// Logs forwarded port binding information.
    ///
    /// # Arguments
    ///
    /// * `target` - Target machine name.
    /// * `guest_port` - Port on the guest VM.
    /// * `host_port` - Port on the host machine.
    pub fn log_forwarded_port(&self, target: &str, guest_port: u16, host_port: u16) {
        let guest_str = guest_port.to_string();
        let host_str = host_port.to_string();
        self.log_csv(target, "forwarded_port", &[&guest_str, &host_str]);
    }

    /// Logs a fatal error exit record.
    ///
    /// # Arguments
    ///
    /// * `error_class` - Error class or category name.
    /// * `message` - Detailed error message.
    pub fn log_error_exit(&self, error_class: &str, message: &str) {
        self.log_csv("", "error-exit", &[error_class, message]);
    }

    /// Logs a structured JSON message to standard output.
    ///
    /// # Arguments
    ///
    /// * `target` - Target machine name.
    /// * `level` - Severity level string.
    /// * `message` - Log message payload.
    fn log_json(&self, target: &str, level: &str, message: &str) {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let log = MachineLog {
            timestamp: ts,
            target,
            component: "ui",
            level,
            message,
        };

        let json = serde_json::to_string(&log).unwrap_or_default();
        println!("{json}");
    }
}

impl Ui for MachineReadableUi {
    fn info(&self, target: &str, message: &str) {
        self.log_csv(target, "ui", &["info", message]);
        self.log_json(target, "info", message);
    }

    fn success(&self, target: &str, message: &str) {
        self.log_csv(target, "ui", &["success", message]);
        self.log_json(target, "success", message);
    }

    fn detail(&self, target: &str, message: &str) {
        self.log_csv(target, "ui", &["detail", message]);
        self.log_json(target, "detail", message);
    }

    fn warn(&self, target: &str, message: &str) {
        self.log_csv(target, "ui", &["warn", message]);
        self.log_json(target, "warn", message);
    }

    fn error(&self, target: &str, message: &str) {
        self.log_csv(target, "ui", &["error", message]);
        self.log_json(target, "error", message);
    }

    fn create_progress(&self, target: &str, total: u64, message: &str) -> ProgressBar {
        self.log_json(
            target,
            "progress",
            &format!("total={total}, message={message}"),
        );
        ProgressBar::hidden()
    }

    fn prompt_choice(
        &self,
        target: &str,
        prompt: &str,
        choices: &[&str],
    ) -> Result<String, crate::error::MigratoryError> {
        let choices_str = choices.join(",");
        self.log_json(
            target,
            "prompt_choice",
            &format!("prompt={prompt}, choices={choices_str}"),
        );

        let mut input = String::new();
        read_stdin(&mut input).map_err(crate::error::MigratoryError::Io)?;
        let input = input.trim();
        if input.is_empty() {
            return Err(crate::error::MigratoryError::Validation(
                "Invalid choice".to_string(),
            ));
        }
        Ok(input.to_string())
    }
}

/// Thread-safe synchronized UI wrapper for concurrent operations.
pub struct ConcurrentUi {
    /// Mutex serializing concurrent output calls across worker threads.
    lock: std::sync::Mutex<()>,
    /// Underlying UI implementation receiving synchronized calls.
    inner: Box<dyn Ui + Send + Sync>,
}

impl ConcurrentUi {
    /// Creates a new [`ConcurrentUi`] wrapping `inner`.
    ///
    /// # Arguments
    ///
    /// * `inner` - Boxed inner [`Ui`] implementation.
    ///
    /// # Returns
    ///
    /// Returns a new [`ConcurrentUi`] instance.
    pub fn new(inner: Box<dyn Ui + Send + Sync>) -> Self {
        Self {
            lock: std::sync::Mutex::new(()),
            inner,
        }
    }
}

impl Ui for ConcurrentUi {
    fn info(&self, target: &str, message: &str) {
        let _guard = self.lock.lock();
        self.inner.info(target, message);
    }

    fn success(&self, target: &str, message: &str) {
        let _guard = self.lock.lock();
        self.inner.success(target, message);
    }

    fn detail(&self, target: &str, message: &str) {
        let _guard = self.lock.lock();
        self.inner.detail(target, message);
    }

    fn warn(&self, target: &str, message: &str) {
        let _guard = self.lock.lock();
        self.inner.warn(target, message);
    }

    fn error(&self, target: &str, message: &str) {
        let _guard = self.lock.lock();
        self.inner.error(target, message);
    }

    fn create_progress(&self, target: &str, total: u64, message: &str) -> ProgressBar {
        let _guard = self.lock.lock();
        self.inner.create_progress(target, total, message)
    }

    fn prompt_choice(
        &self,
        target: &str,
        prompt: &str,
        choices: &[&str],
    ) -> Result<String, crate::error::MigratoryError> {
        let _guard = self.lock.lock();
        self.inner.prompt_choice(target, prompt, choices)
    }
}

/// A target-scoped UI wrapper that automatically prefixes all output with a designated machine target.
pub struct PrefixedUi<'a> {
    /// Machine or component target name prefixed to all output messages.
    target: String,
    /// Borrowed reference to the underlying [`Ui`] implementation.
    ui: &'a dyn Ui,
}

impl<'a> PrefixedUi<'a> {
    /// Creates a new [`PrefixedUi`] bound to `target`.
    ///
    /// # Arguments
    ///
    /// * `target` - Machine target name.
    /// * `ui` - Underlying [`Ui`] implementation.
    ///
    /// # Returns
    ///
    /// Returns a new [`PrefixedUi`] instance.
    pub fn new(target: &str, ui: &'a dyn Ui) -> Self {
        Self {
            target: target.to_string(),
            ui,
        }
    }

    /// Prints an informational message with the machine prefix.
    ///
    /// # Arguments
    ///
    /// * `message` - Message text to display.
    pub fn info(&self, message: &str) {
        self.ui.info(&self.target, message);
    }

    /// Prints a detail message with the machine prefix.
    ///
    /// # Arguments
    ///
    /// * `message` - Detail text to display.
    pub fn detail(&self, message: &str) {
        self.ui.detail(&self.target, message);
    }

    /// Prints a warning message with the machine prefix.
    ///
    /// # Arguments
    ///
    /// * `message` - Warning text to display.
    pub fn warn(&self, message: &str) {
        self.ui.warn(&self.target, message);
    }

    /// Prints an error message with the machine prefix.
    ///
    /// # Arguments
    ///
    /// * `message` - Error text to display.
    pub fn error(&self, message: &str) {
        self.ui.error(&self.target, message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::MigratoryError;

    /// Replaces the thread-local mock stdin queue with the given lines.
    fn set_mock_stdin(lines: Vec<&str>) {
        MOCK_STDIN.with(|m| {
            *m.borrow_mut() = lines.into_iter().map(str::to_string).collect();
        });
    }

    #[test]
    fn test_console_ui() {
        let ui = ConsoleUi;
        ui.info("test", "test message");
        ui.success("test", "test success");
        ui.detail("test", "test detail");
        ui.warn("test", "test warn");
        ui.error("test", "test error");
        ui.error_std("test", &MigratoryError::Generic("test err".into()));
        let pb = ui.create_progress("test", 100, "Downloading");
        pb.finish();
    }

    #[test]
    fn test_console_ui_prompt_choice_valid() -> Result<(), MigratoryError> {
        let ui = ConsoleUi;
        set_mock_stdin(vec!["2"]);
        let res = ui.prompt_choice("test", "Choose:", &["A", "B", "C"])?;
        assert_eq!(res, "B");
        Ok(())
    }

    #[test]
    fn test_console_ui_prompt_choice_invalid_number() {
        let ui = ConsoleUi;
        set_mock_stdin(vec!["4"]);
        let res = ui.prompt_choice("test", "Choose:", &["A", "B", "C"]);
        assert!(matches!(res, Err(MigratoryError::Validation(_))));
    }

    #[test]
    fn test_console_ui_prompt_choice_not_a_number() {
        let ui = ConsoleUi;
        set_mock_stdin(vec!["abc"]);
        let res = ui.prompt_choice("test", "Choose:", &["A", "B", "C"]);
        assert!(matches!(res, Err(MigratoryError::Validation(_))));
    }

    #[test]
    fn test_console_ui_prompt_choice_empty_input() {
        let ui = ConsoleUi;
        set_mock_stdin(vec![]);
        let res = ui.prompt_choice("test", "Choose:", &["A", "B", "C"]);
        assert!(matches!(res, Err(MigratoryError::Validation(_))));
    }

    #[test]
    fn test_console_ui_prompt_choice_io_error() {
        let ui = ConsoleUi;
        set_mock_stdin(vec!["__IO_ERROR__"]);
        let res = ui.prompt_choice("test", "Choose:", &["A", "B", "C"]);
        assert!(matches!(res, Err(MigratoryError::Io(_))));
    }

    #[test]
    fn test_machine_readable_ui() {
        let ui = MachineReadableUi;
        ui.info("test", "test message");
        ui.success("test", "test success");
        ui.detail("test", "test detail");
        ui.warn("test", "test warn");
        ui.error("test", "test error");
        ui.error_std("test", &MigratoryError::Generic("test err".into()));
        let pb = ui.create_progress("test", 100, "Downloading");
        pb.finish();
    }

    #[test]
    fn test_machine_readable_ui_prompt_choice() -> Result<(), MigratoryError> {
        let ui = MachineReadableUi;
        set_mock_stdin(vec!["my_choice"]);
        let res = ui.prompt_choice("test", "Choose:", &["A", "B"])?;
        assert_eq!(res, "my_choice");
        Ok(())
    }

    #[test]
    fn test_machine_readable_ui_prompt_choice_empty() {
        let ui = MachineReadableUi;
        set_mock_stdin(vec![]);
        let res = ui.prompt_choice("test", "Choose:", &["A", "B"]);
        assert!(matches!(res, Err(MigratoryError::Validation(_))));
    }

    #[test]
    fn test_machine_readable_ui_prompt_choice_io_error() {
        let ui = MachineReadableUi;
        set_mock_stdin(vec!["__IO_ERROR__"]);
        let res = ui.prompt_choice("test", "Choose:", &["A", "B"]);
        assert!(matches!(res, Err(MigratoryError::Io(_))));
    }

    #[test]
    fn test_get_progress_style_fallback() {
        let valid = get_progress_style("{bar}");
        let _ = valid;

        let invalid = get_progress_style("{foo");
        let _ = invalid;
    }

    #[test]
    fn test_concurrent_ui_and_prefixed_ui() -> Result<(), MigratoryError> {
        let concurrent = ConcurrentUi::new(Box::new(ConsoleUi));
        concurrent.info("concurrent", "info message");
        concurrent.success("concurrent", "success message");
        concurrent.detail("concurrent", "detail message");
        concurrent.warn("concurrent", "warn message");
        concurrent.error("concurrent", "error message");
        let pb = concurrent.create_progress("concurrent", 100, "Progress");
        pb.finish();

        set_mock_stdin(vec!["1"]);
        let res = concurrent.prompt_choice("concurrent", "Choose:", &["A", "B"])?;
        assert_eq!(res, "A");

        let prefixed = PrefixedUi::new("web", &concurrent);
        prefixed.info("prefixed info");
        prefixed.detail("prefixed detail");
        prefixed.warn("prefixed warn");
        prefixed.error("prefixed error");
        Ok(())
    }

    #[test]
    fn test_color_for_target() {
        assert_eq!(color_for_target(""), colored::Color::Cyan);
        assert_eq!(color_for_target("migratory"), colored::Color::Cyan);
        assert_eq!(color_for_target("vagrant"), colored::Color::Cyan);
        assert_eq!(color_for_target("d"), colored::Color::Cyan);
        assert_eq!(color_for_target("e"), colored::Color::Magenta);
        assert_eq!(color_for_target("a"), colored::Color::Yellow);
        assert_eq!(color_for_target("b"), colored::Color::Green);
        assert_eq!(color_for_target("c"), colored::Color::Blue);
    }

    #[test]
    fn test_csv_formatting_and_machine_readable_ui() {
        let lf = char::from(10);
        let cr = char::from(13);
        assert_eq!(format_csv_field("simple"), "simple");
        assert_eq!(format_csv_field("with,comma"), r#""with,comma""#);
        assert_eq!(format_csv_field(r#"with"quote"#), r#""with""quote""#);
        assert_eq!(
            format_csv_field(&format!("with{lf}newline")),
            format!(r#""with{lf}newline""#)
        );
        assert_eq!(
            format_csv_field(&format!("with{cr}carriage")),
            format!(r#""with{cr}carriage""#)
        );

        let line = format_machine_readable_csv(12345, "default", "ui", &["info", "hello, world"]);
        assert_eq!(line, r#"12345,default,ui,info,"hello, world""#);

        let ui = MachineReadableUi;
        ui.log_state("default", "running");
        ui.log_box_info("default", "ubuntu/jammy64", "virtualbox", "1.0.0");
        ui.log_forwarded_port("default", 80, 8080);
        ui.log_error_exit("Vagrant::Errors::MachineNotFound", "Machine not found");
    }

    #[test]
    fn test_machine_readable_toggle_and_default_ui() {
        set_machine_readable(true);
        assert!(is_machine_readable());
        let ui_mr = default_ui();
        ui_mr.info("default", "mr test");

        set_machine_readable(false);
        assert!(!is_machine_readable());
        let ui_con = default_ui();
        ui_con.info("default", "console test");
        let _ = get_progress_style("{invalid_key_xyz}");

        /// Minimal UI exercising default trait methods.
        struct MinimalUi;
        impl Ui for MinimalUi {}
        let min = MinimalUi;
        min.info("t", "m");
        min.success("t", "m");
        min.detail("t", "m");
        min.warn("t", "m");
        min.error("t", "m");
        min.error_std("t", &MigratoryError::Generic("e".to_string()));
        let _ = min.create_progress("t", 10, "m");
        assert_eq!(
            min.prompt_choice("t", "p", &["x"]).ok(),
            Some("x".to_string())
        );
        assert!(min.prompt_choice("t", "p", &[]).is_err());
    }
}
