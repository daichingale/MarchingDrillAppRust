//! Out-of-process plugin execution. Native libraries are deliberately unsupported.

use crate::{
    CallKind, Capability, HostCommand, PluginLimits, PluginManifest, PluginResponse, ProtocolError,
    SignatureVerifier, validate_package, validate_response,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const POLL_INTERVAL: Duration = Duration::from_millis(4);

#[derive(Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);
impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Clone, Debug)]
pub struct RunnerSpec {
    /// Must resolve to a regular, absolute executable. DLLs are never loaded.
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    /// Explicit environment allowlist. The ambient host environment is cleared.
    pub environment: Vec<(String, String)>,
    pub manifest_json: Vec<u8>,
    pub granted: BTreeSet<Capability>,
    pub kind: CallKind,
    /// Diagnostic output is never parsed and is independently bounded.
    pub stderr_bytes: usize,
}

#[derive(Debug)]
pub struct RunResult {
    pub response: PluginResponse,
    pub stderr: String,
    pub elapsed: Duration,
}

#[derive(Debug)]
pub enum RunnerError {
    NotRegistered,
    PublisherNotTrusted,
    Quarantined,
    PathNotAbsolute,
    NotRegularFile,
    ComponentTooLarge,
    Io(std::io::Error),
    Protocol(ProtocolError),
    Spawn(std::io::Error),
    Cancelled,
    DeadlineExceeded,
    OutputTooLarge,
    StderrTooLarge,
    ProcessFailed(Option<i32>),
}

#[derive(Clone, Debug)]
pub struct RegisteredPlugin {
    pub spec: RunnerSpec,
    pub approved_capabilities: BTreeSet<Capability>,
    pub trusted_publisher: bool,
    pub consecutive_failures: u8,
    pub disabled_reason: Option<String>,
}

/// Host-side lifecycle registry. It stores decisions, never loaded code or child state.
pub struct HostManager {
    plugins: BTreeMap<String, RegisteredPlugin>,
    failure_limit: u8,
}
impl Default for HostManager {
    fn default() -> Self {
        Self::new()
    }
}
impl HostManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            plugins: BTreeMap::new(),
            failure_limit: 3,
        }
    }
    pub fn register(&mut self, id: String, mut plugin: RegisteredPlugin) {
        plugin.spec.granted = plugin.approved_capabilities.clone();
        self.plugins.insert(id, plugin);
    }
    #[must_use]
    pub fn plugins(&self) -> &BTreeMap<String, RegisteredPlugin> {
        &self.plugins
    }
    pub fn approve_capabilities(&mut self, id: &str, approved: BTreeSet<Capability>) {
        if let Some(plugin) = self.plugins.get_mut(id) {
            plugin.approved_capabilities = approved.clone();
            plugin.spec.granted = approved;
            plugin.consecutive_failures = 0;
            plugin.disabled_reason = None;
        }
    }
    pub fn set_publisher_trusted(&mut self, id: &str, trusted: bool) {
        if let Some(plugin) = self.plugins.get_mut(id) {
            plugin.trusted_publisher = trusted;
        }
    }
    /// Explicitly re-enables a quarantined plugin after the user has inspected
    /// or replaced it. Trust and capability approvals are intentionally kept.
    pub fn reset_quarantine(&mut self, id: &str) -> Result<(), RunnerError> {
        let plugin = self.plugins.get_mut(id).ok_or(RunnerError::NotRegistered)?;
        plugin.consecutive_failures = 0;
        plugin.disabled_reason = None;
        Ok(())
    }
    pub fn execute(
        &mut self,
        id: &str,
        command: &HostCommand,
        verifier: &dyn SignatureVerifier,
        limits: PluginLimits,
        cancel: &CancellationToken,
    ) -> Result<RunResult, RunnerError> {
        let plugin = self.plugins.get_mut(id).ok_or(RunnerError::NotRegistered)?;
        if !plugin.trusted_publisher {
            return Err(RunnerError::PublisherNotTrusted);
        }
        if plugin.disabled_reason.is_some() {
            return Err(RunnerError::Quarantined);
        }
        let result = run(&plugin.spec, command, verifier, limits, cancel);
        match result {
            Ok(value) => {
                plugin.consecutive_failures = 0;
                Ok(value)
            }
            Err(error) => {
                // A user cancellation is not a plugin fault and must not move a
                // healthy plugin toward quarantine.
                if !matches!(error, RunnerError::Cancelled) {
                    plugin.consecutive_failures = plugin.consecutive_failures.saturating_add(1);
                    if plugin.consecutive_failures >= self.failure_limit.max(1) {
                        plugin.disabled_reason = Some("repeated isolated process failures".into());
                    }
                }
                Err(error)
            }
        }
    }
}
impl From<ProtocolError> for RunnerError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

fn read_bounded(mut reader: impl Read, limit: usize) -> std::io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    let mut chunk = [0_u8; 8192];
    loop {
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        let remaining = limit.saturating_add(1).saturating_sub(bytes.len());
        bytes.extend_from_slice(&chunk[..n.min(remaining)]);
        if bytes.len() > limit {
            return Ok((bytes, true));
        }
    }
    Ok((bytes, false))
}

fn verified_executable(
    spec: &RunnerSpec,
    verifier: &dyn SignatureVerifier,
    limits: PluginLimits,
) -> Result<(PathBuf, PluginManifest), RunnerError> {
    if !spec.executable.is_absolute() {
        return Err(RunnerError::PathNotAbsolute);
    }
    let path = spec.executable.canonicalize().map_err(RunnerError::Io)?;
    let meta = fs::metadata(&path).map_err(RunnerError::Io)?;
    if !meta.is_file() {
        return Err(RunnerError::NotRegularFile);
    }
    if meta.len() > limits.memory_bytes as u64 {
        return Err(RunnerError::ComponentTooLarge);
    }
    // Read and verify immediately before spawn, closing package-install TOCTOU windows.
    let component = fs::read(&path).map_err(RunnerError::Io)?;
    let manifest = validate_package(
        &spec.manifest_json,
        &component,
        &spec.granted,
        verifier,
        limits,
    )?;
    Ok((path, manifest))
}

/// Executes one request in a disposable child process. Every failure kills and
/// discards the child; no untrusted state is retained in the host process.
pub fn run(
    spec: &RunnerSpec,
    command: &HostCommand,
    verifier: &dyn SignatureVerifier,
    limits: PluginLimits,
    cancel: &CancellationToken,
) -> Result<RunResult, RunnerError> {
    if cancel.is_cancelled() {
        return Err(RunnerError::Cancelled);
    }
    let request = serde_json::to_vec(command)
        .map_err(|_| RunnerError::Protocol(ProtocolError::InvalidJson))?;
    if request.len() > limits.request_bytes {
        return Err(RunnerError::Protocol(ProtocolError::InputTooLarge));
    }
    let (path, manifest) = verified_executable(spec, verifier, limits)?;
    // Tests spawn several children at once. On Windows each one is
    // PowerShell, and a cold start can miss a short deadline if they all
    // launch together. One slot, taken before the clock starts, keeps the
    // result about the child rather than about how crowded the runner was.
    #[cfg(test)]
    let _process_slot = test_process_slot();
    let started = Instant::now();
    let mut child = Command::new(path)
        .args(&spec.arguments)
        .env_clear()
        .envs(spec.environment.iter().map(|(key, value)| (key, value)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(RunnerError::Spawn)?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let input_thread = thread::spawn(move || {
        stdin
            .write_all(&request)
            .and_then(|_| stdin.write_all(b"\n"))
    });
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let output_limit = limits.output_bytes;
    let stderr_limit = spec.stderr_bytes;
    let out_thread = thread::spawn(move || read_bounded(stdout, output_limit));
    let err_thread = thread::spawn(move || read_bounded(stderr, stderr_limit));
    let deadline = limits.deadline(spec.kind);
    let status = loop {
        if cancel.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            let _ = input_thread.join();
            let _ = out_thread.join();
            let _ = err_thread.join();
            return Err(RunnerError::Cancelled);
        }
        if started.elapsed() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = input_thread.join();
            let _ = out_thread.join();
            let _ = err_thread.join();
            return Err(RunnerError::DeadlineExceeded);
        }
        let wait = match child.try_wait() {
            Ok(wait) => wait,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = input_thread.join();
                let _ = out_thread.join();
                let _ = err_thread.join();
                return Err(RunnerError::Io(error));
            }
        };
        match wait {
            Some(status) => break status,
            None => thread::sleep(POLL_INTERVAL),
        }
    };
    input_thread
        .join()
        .map_err(|_| RunnerError::Io(std::io::Error::other("plugin input thread panicked")))?
        .map_err(RunnerError::Io)?;
    let (stdout, output_overflow) = out_thread
        .join()
        .map_err(|_| RunnerError::OutputTooLarge)?
        .map_err(RunnerError::Io)?;
    let (stderr, stderr_overflow) = err_thread
        .join()
        .map_err(|_| RunnerError::StderrTooLarge)?
        .map_err(RunnerError::Io)?;
    if output_overflow {
        return Err(RunnerError::OutputTooLarge);
    }
    if stderr_overflow {
        return Err(RunnerError::StderrTooLarge);
    }
    if !status.success() {
        return Err(RunnerError::ProcessFailed(status.code()));
    }
    let response = validate_response(command, &stdout, &manifest, limits)?;
    Ok(RunResult {
        response,
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        elapsed: started.elapsed(),
    })
}

#[cfg(test)]
fn test_process_slot() -> std::sync::MutexGuard<'static, ()> {
    static SLOT: std::sync::Mutex<()> = std::sync::Mutex::new(());
    SLOT.lock().unwrap_or_else(|poison| poison.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ApiVersion, IntegrityMetadata, PluginInfo};
    use semver::Version;

    /// Long enough for PowerShell to start on a cold Windows runner.
    /// The tests still check the child's result, not how fast it started.
    fn child_deadline() -> Duration {
        #[cfg(windows)]
        {
            Duration::from_secs(20)
        }
        #[cfg(not(windows))]
        {
            Duration::from_secs(5)
        }
    }

    struct Verifier;
    impl SignatureVerifier for Verifier {
        fn verify(&self, key: &str, _: &[u8], signature: &str) -> bool {
            key == "test" && signature == "ok"
        }
    }
    fn spec(mode: &str) -> RunnerSpec {
        #[cfg(windows)]
        let executable = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe")
            .canonicalize()
            .unwrap();
        #[cfg(not(windows))]
        let executable = PathBuf::from("/bin/sh").canonicalize().unwrap();
        let bytes = fs::read(&executable).unwrap();
        let manifest = PluginManifest {
            schema_version: crate::MANIFEST_SCHEMA,
            api: ApiVersion::CURRENT,
            id: "org.drillforge.mock".into(),
            version: Version::new(1, 0, 0),
            publisher: "Tests".into(),
            requested: BTreeSet::new(),
            integrity: IntegrityMetadata {
                component_blake3: blake3::hash(&bytes).to_hex().to_string(),
                signing_key_id: "test".into(),
                signature: "ok".into(),
            },
        };
        let response = serde_json::to_string(&PluginResponse::Description(PluginInfo {
            provider_id: "org.drillforge.mock".into(),
            display_name_key: "mock".into(),
            version: Version::new(1, 0, 0),
            api: ApiVersion::CURRENT,
        }))
        .unwrap();
        #[cfg(windows)]
        let arguments = match mode {
            "ok" => vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-Command".into(),
                format!(
                    "$null = [Console]::In.ReadToEnd(); [Console]::Out.Write('{}')",
                    response.replace('\'', "''")
                ),
            ],
            "large" => vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-Command".into(),
                "[Console]::Out.Write(('x' * 8192))".into(),
            ],
            "stderr-large" => vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-Command".into(),
                "[Console]::Error.Write(('x' * 8192))".into(),
            ],
            "wait" => vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-Command".into(),
                "Start-Sleep -Seconds 2".into(),
            ],
            _ => vec![
                "-NoProfile".into(),
                "-Command".into(),
                "$null = [Console]::In.ReadToEnd(); exit 7".into(),
            ],
        };
        #[cfg(not(windows))]
        let arguments = match mode {
            "ok" => vec![
                "-c".into(),
                format!("cat >/dev/null; printf '%s' '{response}'"),
            ],
            "large" => vec!["-c".into(), "yes 1234567890 | head -1000".into()],
            "stderr-large" => vec!["-c".into(), "yes 1234567890 | head -1000 >&2".into()],
            "wait" => vec!["-c".into(), "sleep 2".into()],
            _ => vec!["-c".into(), "cat >/dev/null; exit 7".into()],
        };
        RunnerSpec {
            executable,
            arguments,
            #[cfg(windows)]
            environment: vec![("SystemRoot".into(), std::env::var("SystemRoot").unwrap())],
            #[cfg(not(windows))]
            environment: Vec::new(),
            manifest_json: serde_json::to_vec(&manifest).unwrap(),
            granted: BTreeSet::new(),
            kind: CallKind::Interactive,
            stderr_bytes: 4096,
        }
    }
    fn execute(mode: &str, mut limits: PluginLimits) -> Result<RunResult, RunnerError> {
        limits.memory_bytes = usize::MAX;
        if mode != "wait" {
            limits.interactive_deadline = child_deadline();
        }
        run(
            &spec(mode),
            &HostCommand::Describe,
            &Verifier,
            limits,
            &CancellationToken::default(),
        )
    }
    #[test]
    fn executes_bounded_json_child() {
        let result = execute("ok", PluginLimits::DEFAULT);
        assert!(result.is_ok(), "{result:?}");
    }
    #[test]
    fn kills_deadline() {
        let mut l = PluginLimits::DEFAULT;
        l.interactive_deadline = Duration::from_millis(40);
        assert!(matches!(
            execute("wait", l),
            Err(RunnerError::DeadlineExceeded)
        ));
    }
    #[test]
    fn cancelled_before_spawn_is_side_effect_free() {
        let token = CancellationToken::default();
        token.cancel();
        assert!(matches!(
            run(
                &spec("ok"),
                &HostCommand::Describe,
                &Verifier,
                PluginLimits::DEFAULT,
                &token
            ),
            Err(RunnerError::Cancelled)
        ));
    }
    #[test]
    fn rejects_stdout_overflow() {
        let mut l = PluginLimits::DEFAULT;
        l.output_bytes = 256;
        assert!(matches!(
            execute("large", l),
            Err(RunnerError::OutputTooLarge)
        ));
    }
    #[test]
    fn rejects_stderr_overflow() {
        let mut s = spec("stderr-large");
        s.stderr_bytes = 256;
        let mut limits = PluginLimits::DEFAULT;
        limits.memory_bytes = usize::MAX;
        limits.interactive_deadline = child_deadline();
        assert!(matches!(
            run(
                &s,
                &HostCommand::Describe,
                &Verifier,
                limits,
                &CancellationToken::default()
            ),
            Err(RunnerError::StderrTooLarge)
        ));
    }
    #[test]
    fn requires_absolute_path() {
        let mut s = spec("ok");
        s.executable = std::path::Path::new("plugin.exe").into();
        assert!(matches!(
            run(
                &s,
                &HostCommand::Describe,
                &Verifier,
                PluginLimits::DEFAULT,
                &CancellationToken::default()
            ),
            Err(RunnerError::PathNotAbsolute)
        ));
    }
    #[test]
    fn host_requires_trust_and_quarantines_repeated_failure() {
        let mut host = HostManager::new();
        host.register(
            "mock".into(),
            RegisteredPlugin {
                spec: spec("fail"),
                approved_capabilities: BTreeSet::new(),
                trusted_publisher: false,
                consecutive_failures: 0,
                disabled_reason: None,
            },
        );
        let cancel = CancellationToken::default();
        assert!(matches!(
            host.execute(
                "mock",
                &HostCommand::Describe,
                &Verifier,
                PluginLimits {
                    memory_bytes: usize::MAX,
                    interactive_deadline: child_deadline(),
                    ..PluginLimits::DEFAULT
                },
                &cancel
            ),
            Err(RunnerError::PublisherNotTrusted)
        ));
        host.set_publisher_trusted("mock", true);
        for _ in 0..3 {
            assert!(matches!(
                host.execute(
                    "mock",
                    &HostCommand::Describe,
                    &Verifier,
                    PluginLimits {
                        memory_bytes: usize::MAX,
                        interactive_deadline: child_deadline(),
                        ..PluginLimits::DEFAULT
                    },
                    &cancel
                ),
                Err(RunnerError::ProcessFailed(_))
            ));
        }
        assert!(host.plugins()["mock"].disabled_reason.is_some());
        assert!(matches!(
            host.execute(
                "mock",
                &HostCommand::Describe,
                &Verifier,
                PluginLimits::DEFAULT,
                &cancel
            ),
            Err(RunnerError::Quarantined)
        ));
        host.reset_quarantine("mock").unwrap();
        assert_eq!(host.plugins()["mock"].consecutive_failures, 0);
        assert!(host.plugins()["mock"].disabled_reason.is_none());
        host.plugins.get_mut("mock").unwrap().spec = spec("ok");
        let recovered = host.execute(
            "mock",
            &HostCommand::Describe,
            &Verifier,
            PluginLimits {
                memory_bytes: usize::MAX,
                interactive_deadline: child_deadline(),
                ..PluginLimits::DEFAULT
            },
            &cancel,
        );
        assert!(recovered.is_ok(), "{recovered:?}");
    }

    #[test]
    fn cancellation_does_not_count_as_plugin_failure() {
        let mut host = HostManager::new();
        host.register(
            "mock".into(),
            RegisteredPlugin {
                spec: spec("ok"),
                approved_capabilities: BTreeSet::new(),
                trusted_publisher: true,
                consecutive_failures: 0,
                disabled_reason: None,
            },
        );
        let cancel = CancellationToken::default();
        cancel.cancel();
        assert!(matches!(
            host.execute(
                "mock",
                &HostCommand::Describe,
                &Verifier,
                PluginLimits::DEFAULT,
                &cancel
            ),
            Err(RunnerError::Cancelled)
        ));
        assert_eq!(host.plugins()["mock"].consecutive_failures, 0);
    }
}
