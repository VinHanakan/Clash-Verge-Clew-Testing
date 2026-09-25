//! Verge-owned controller for the Clew headless helper.
//!
//! The public command accepts only an application command-line pattern and a
//! named, backend-owned strategy group. Helper/config paths and listener
//! endpoints are resolved here so the UI cannot replace them with arbitrary
//! executables or proxies.

use crate::{config::Config, core::{CoreManager, handle}};
use anyhow::{Context, Result, anyhow, ensure};
use clash_verge_logging::{Type, logging};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use serde_yaml_ng::{Mapping, Value as YamlValue};
use tauri_plugin_mihomo::models::ProxyType;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, OnceLock, RwLock,
    },
    time::{Duration, Instant},
};
#[cfg(not(windows))]
use tokio::process::Command;
use tokio::{
    net::TcpStream,
    sync::Mutex,
    time::{sleep, timeout},
};
#[cfg(windows)]
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::Threading::{GetExitCodeProcess, GetProcessId, TerminateProcess, WaitForSingleObject},
        UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
    },
    core::PCWSTR,
};

const HELPER_API: &str = "http://127.0.0.1:18080";

fn default_true() -> bool {
    true
}

fn default_process_name() -> String {
    "*".into()
}

fn default_protocol() -> String {
    "tcp".into()
}

fn default_strategy_group() -> String {
    "regular".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppProxyRule {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_process_name")]
    pub process_name: String,
    pub cmdline_pattern: String,
    #[serde(default)]
    pub hack_tree: bool,
    #[serde(default)]
    pub proxy_group_id: u32,
    #[serde(default = "default_strategy_group")]
    pub strategy_group: String,
    #[serde(default = "default_protocol")]
    pub protocol: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppProxyStartRequest {
    #[serde(default)]
    pub process_cmdline: String,
    /// Backend-owned strategy group name, not an arbitrary port/path.
    pub strategy_group: String,
    #[serde(default)]
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub rules: Option<Vec<AppProxyRule>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppProxyStatus {
    pub state: String,
    pub helper_ready: bool,
    pub proxy_ready: bool,
    pub effective_rules_version: Option<u64>,
    pub pid: Option<u32>,
    pub strategy_group: Option<String>,
    pub runtime_id: Option<String>,
    #[serde(default)]
    pub last_startup_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppProxyConnection {
    pub pid: u32,
    pub process_name: String,
    pub local_ip: String,
    pub local_port: u16,
    pub remote_ip: String,
    pub remote_port: u16,
    pub state: String,
    #[serde(default)]
    pub dest: Option<String>,
    pub hijacked: bool,
    pub pid_alive: bool,
    #[serde(default)]
    pub proxy_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppProxyProcessNode {
    pub pid: u32,
    pub parent_pid: u32,
    pub name: String,
    pub hijacked: bool,
    #[serde(default)]
    pub hijack_source: Option<String>,
    #[serde(default)]
    pub children: Vec<AppProxyProcessNode>,
    #[serde(default)]
    pub cmdline: Option<String>,
    #[serde(default)]
    pub image_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppProxyProcessDetail {
    pub pid: u32,
    pub name: String,
    pub parent_pid: Option<u32>,
    pub hijacked: bool,
    #[serde(default)]
    pub hijack_source: Option<String>,
    #[serde(default)]
    pub cmdline: Option<String>,
    #[serde(default)]
    pub image_path: Option<String>,
}

fn phase3_runtime_id() -> Option<String> {
    std::env::var("CLEW_PHASE3_RUN_ID").ok()
}

struct ManagedHelper {
    process: ManagedProcess,
    token_path: PathBuf,
    token: String,
    _process_cmdline: String,
    rules: Vec<AppProxyRule>,
    listener: String,
    listener_name: String,
    extra_listeners: BTreeMap<String, ManagedListenerSpec>,
    strategy_group: String,
    effective_rules_version: u64,
    failed: bool,
}

#[cfg(not(windows))]
struct ManagedProcess(tokio::process::Child);

#[cfg(windows)]
struct ManagedProcess {
    handle: isize,
    pid: u32,
}

#[cfg(windows)]
impl Drop for ManagedProcess {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(HANDLE(self.handle as *mut std::ffi::c_void));
        }
    }
}

impl ManagedProcess {
    fn id(&self) -> Option<u32> {
        #[cfg(windows)]
        {
            Some(self.pid)
        }
        #[cfg(not(windows))]
        {
            self.0.id()
        }
    }
    fn is_alive(&mut self) -> bool {
        self.exit_code().is_ok_and(|code| code.is_none())
    }
    fn exit_code(&mut self) -> Result<Option<u32>> {
        #[cfg(windows)]
        {
            let mut code = 0;
            ensure!(
                unsafe { GetExitCodeProcess(HANDLE(self.handle as *mut std::ffi::c_void), &mut code).is_ok() },
                "query helper process exit code"
            );
            Ok((code != 259).then_some(code))
        }
        #[cfg(not(windows))]
        {
            Ok(self.0.try_wait()?.map(|status| status.code().unwrap_or(1) as u32))
        }
    }
    async fn wait(&mut self) {
        #[cfg(windows)]
        {
            let handle = self.handle;
            let _ = tokio::task::spawn_blocking(move || unsafe {
                WaitForSingleObject(HANDLE(handle as *mut std::ffi::c_void), 10_000)
            })
            .await;
        }
        #[cfg(not(windows))]
        {
            let _ = self.0.wait().await;
        }
    }
    async fn kill(&mut self) {
        #[cfg(windows)]
        {
            unsafe {
                let _ = TerminateProcess(HANDLE(self.handle as *mut std::ffi::c_void), 1);
            }
        }
        #[cfg(not(windows))]
        {
            let _ = self.0.kill().await;
        }
    }
}

fn rules_file_path() -> Result<PathBuf> {
    let dir = crate::utils::dirs::app_home_dir()?.join("app-proxy");
    std::fs::create_dir_all(&dir).context("create app proxy directory")?;
    Ok(dir.join("rules.json"))
}

fn load_rules_from_disk() -> Vec<AppProxyRule> {
    if let Ok(path) = rules_file_path() {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(rules) = serde_json::from_str::<Vec<AppProxyRule>>(&content) {
                return rules;
            }
        }
    }
    Vec::new()
}

fn save_rules_to_disk(rules: &[AppProxyRule]) -> Result<()> {
    let path = rules_file_path()?;
    let content = serde_json::to_string_pretty(rules)?;
    let temporary = path.with_extension(format!("{}.tmp", nanoid::nanoid!(12)));
    let result = (|| -> Result<()> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temporary, &path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result?;
    Ok(())
}

fn compile_rules(
    rules: &[AppProxyRule],
    primary_group: &str,
    listeners: &BTreeMap<String, ManagedListenerSpec>,
) -> Result<(Vec<AppProxyRule>, Vec<Value>)> {
    let mut names = BTreeSet::new();
    let mut destinations = vec![primary_group.to_owned()];
    for rule in rules {
        ensure!(!rule.id.trim().is_empty() && names.insert(&rule.id), "application rule ids must be unique and nonempty");
        ensure!(!rule.cmdline_pattern.trim().is_empty(), "application rule match must not be empty");
        let broad_match = rule.cmdline_pattern.trim().trim_matches('*').to_ascii_lowercase();
        let executable_only = broad_match.trim_matches('"').trim_end();
        ensure!(!matches!(broad_match.as_str(), "" | "node.exe" | "python.exe" | "pythonw.exe"
            | "clew.exe" | "verge-mihomo.exe" | "clash-verge.exe" | "clash-verge-clew-internal.exe"),
            "rule match is too broad or targets proxy infrastructure: {}", rule.cmdline_pattern);
        ensure!(!["node.exe", "python.exe", "pythonw.exe"].iter().any(|name| executable_only.ends_with(name)),
            "script interpreter rules require a specific script command line");
        ensure!(rule.protocol == "tcp", "application proxy currently supports TCP rules only");
        if rule.enabled && !destinations.contains(&rule.strategy_group) {
            destinations.push(rule.strategy_group.clone());
        }
    }
    destinations[1..].sort();
    let mut compiled = rules.to_vec();
    for rule in &mut compiled {
        if rule.enabled {
            rule.proxy_group_id = destinations.iter().position(|name| name == &rule.strategy_group)
                .ok_or_else(|| anyhow!("missing application strategy binding"))? as u32;
        }
    }
    let groups = destinations.iter().enumerate().map(|(id, name)| {
        let spec = listeners.get(name).ok_or_else(|| anyhow!("missing managed listener for {name}"))?;
        Ok(json!({"id": id, "name": name, "host": "127.0.0.1", "port": spec.port}))
    }).collect::<Result<Vec<_>>>()?;
    Ok((compiled, groups))
}

fn required_groups(rules: &[AppProxyRule], primary_group: &str) -> BTreeSet<String> {
    let mut groups = BTreeSet::from([primary_group.to_owned()]);
    groups.extend(rules.iter().filter(|rule| rule.enabled).map(|rule| rule.strategy_group.clone()));
    groups
}

#[derive(Clone)]
pub struct AppProxyManager {
    inner: Arc<Mutex<Option<ManagedHelper>>>,
    desired_listener: Arc<RwLock<Vec<ManagedListenerSpec>>>,
    last_startup_error: Arc<RwLock<Option<String>>>,
    rules: Arc<RwLock<Vec<AppProxyRule>>>,
    rules_update_lock: Arc<Mutex<()>>,
}

#[derive(Clone)]
pub(crate) struct ManagedListenerSpec {
    pub name: String,
    pub port: u16,
    pub group: Option<String>,
}

static DESIRED_LISTENER: OnceLock<Arc<RwLock<Vec<ManagedListenerSpec>>>> = OnceLock::new();
static CONFIG_UPDATE_DEPTH: AtomicUsize = AtomicUsize::new(0);
static CONFIG_UPDATE_EPOCH: AtomicU64 = AtomicU64::new(0);
static APP_PROXY_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn ensure_profile_switch_allowed() -> Result<()> {
    ensure!(
        !APP_PROXY_RUNNING.load(Ordering::Acquire),
        "stop application proxy before switching profiles or subscriptions"
    );
    Ok(())
}

pub(crate) fn begin_config_update() {
    if CONFIG_UPDATE_DEPTH.fetch_add(1, Ordering::AcqRel) == 0 {
        CONFIG_UPDATE_EPOCH.fetch_add(1, Ordering::AcqRel);
    }
}

pub(crate) fn finish_config_update() {
    if CONFIG_UPDATE_DEPTH.fetch_sub(1, Ordering::AcqRel) == 1 {
        CONFIG_UPDATE_EPOCH.fetch_add(1, Ordering::AcqRel);
    }
}

fn config_update_snapshot() -> u64 {
    CONFIG_UPDATE_EPOCH.load(Ordering::Acquire)
}

fn config_update_is_current(epoch: u64) -> bool {
    epoch.is_multiple_of(2)
        && CONFIG_UPDATE_DEPTH.load(Ordering::Acquire) == 0
        && CONFIG_UPDATE_EPOCH.load(Ordering::Acquire) == epoch
}

impl Default for AppProxyManager {
    fn default() -> Self {
        let desired_listener = Arc::new(RwLock::new(Vec::new()));
        let _ = DESIRED_LISTENER.set(desired_listener.clone());
        let initial_rules = load_rules_from_disk();
        Self {
            inner: Arc::new(Mutex::new(None)),
            desired_listener,
            last_startup_error: Arc::new(RwLock::new(None)),
            rules: Arc::new(RwLock::new(initial_rules)),
            rules_update_lock: Arc::new(Mutex::new(())),
        }
    }
}

pub(crate) fn merge_managed_listener(config: &mut Mapping) {
    let Some(store) = DESIRED_LISTENER.get() else {
        return;
    };
    let Ok(spec) = store.read() else {
        return;
    };
    let listeners = config
        .entry(YamlValue::from("listeners"))
        .or_insert_with(|| YamlValue::Sequence(Vec::new()));
    let Some(items) = listeners.as_sequence_mut() else {
        return;
    };
    for spec in spec.iter() {
        items.retain(|item| item.get("name").and_then(YamlValue::as_str) != Some(spec.name.as_str()));
        let mut item = Mapping::new();
        item.insert("name".into(), spec.name.clone().into());
        item.insert("type".into(), "mixed".into());
        item.insert("listen".into(), "127.0.0.1".into());
        item.insert("port".into(), spec.port.into());
        if let Some(group) = spec.group.as_deref() {
            item.insert("proxy".into(), group.into());
        }
        items.push(YamlValue::Mapping(item));
    }
}

fn clear_desired_listener(name: &str) {
    if let Some(store) = DESIRED_LISTENER.get()
        && let Ok(mut desired) = store.write()
    {
        desired.retain(|spec| spec.name != name);
    }
}

struct StrategyBinding {
    group: Option<String>,
}

enum RulePublishError {
    Rejected(anyhow::Error),
    Uncertain(anyhow::Error),
}

async fn strategy_binding(name: &str) -> Result<StrategyBinding> {
    if name == "regular" {
        return Ok(StrategyBinding { group: None });
    }
    let proxies = handle::Handle::mihomo()
        .get_proxies()
        .await
        .context("read active Mihomo strategy groups")?;
    let Some(proxy) = proxies.proxies.get(name) else {
        return Err(anyhow!("Mihomo strategy group does not exist: {name}"));
    };
    ensure!(matches!(proxy.proxy_type, ProxyType::Selector | ProxyType::URLTest | ProxyType::Fallback | ProxyType::LoadBalance),
        "Mihomo entry is not a strategy group: {name}");
    Ok(StrategyBinding {
        group: Some(name.to_owned()),
    })
}

pub async fn list_strategy_groups() -> Result<Vec<String>> {
    let proxies = handle::Handle::mihomo().get_proxies().await.context("read active Mihomo strategy groups")?;
    let mut groups = proxies.proxies.into_iter()
        .filter(|(_, proxy)| matches!(proxy.proxy_type, ProxyType::Selector | ProxyType::URLTest | ProxyType::Fallback | ProxyType::LoadBalance))
        .map(|(name, _)| name)
        .collect::<Vec<_>>();
    groups.sort();
    Ok(groups)
}

async fn allocate_listener_port() -> Result<u16> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .context("allocate app proxy listener port")?;
    Ok(listener.local_addr()?.port())
}

async fn allocate_unused_listener_port(listeners: &BTreeMap<String, ManagedListenerSpec>) -> Result<u16> {
    for _ in 0..32 {
        let port = allocate_listener_port().await?;
        if listeners.values().all(|listener| listener.port != port) {
            return Ok(port);
        }
    }
    Err(anyhow!("could not allocate a distinct managed listener port"))
}

async fn configure_listener(name: &str, port: u16, group: Option<&str>) -> Result<()> {
    let outcome = CoreManager::global()
        .update_runtime_config(|runtime| {
            let Some(config) = runtime.config.as_mut() else {
                return;
            };
            let listeners = config
                .entry(YamlValue::from("listeners"))
                .or_insert_with(|| YamlValue::Sequence(Vec::new()));
            let Some(items) = listeners.as_sequence_mut() else {
                return;
            };
            items.retain(|item| item.get("name").and_then(YamlValue::as_str) != Some(name));
            let mut item = Mapping::new();
            item.insert("name".into(), name.into());
            item.insert("type".into(), "mixed".into());
            item.insert("listen".into(), "127.0.0.1".into());
            item.insert("port".into(), port.into());
            if let Some(group) = group {
                item.insert("proxy".into(), group.into());
            }
            items.push(YamlValue::Mapping(item));
        })
        .await?;
    ensure!(
        outcome.is_valid(),
        "Mihomo rejected managed app proxy listener configuration: {outcome}"
    );
    Ok(())
}

async fn remove_listener(name: &str) -> Result<()> {
    let outcome = CoreManager::global()
        .update_runtime_config(|runtime| {
            if let Some(items) = runtime
                .config
                .as_mut()
                .and_then(|config| config.get_mut("listeners"))
                .and_then(YamlValue::as_sequence_mut)
            {
                items.retain(|item| item.get("name").and_then(YamlValue::as_str) != Some(name));
            }
        })
        .await?;
    ensure!(
        outcome.is_valid(),
        "failed to remove managed Mihomo app proxy listener: {outcome}"
    );
    Ok(())
}

async fn managed_listener_loaded(name: &str, port: u16, group: Option<&str>) -> bool {
    let runtime = Config::runtime().await.latest_arc();
    runtime
        .config
        .as_ref()
        .and_then(|config| config.get("listeners"))
        .and_then(YamlValue::as_sequence)
        .is_some_and(|listeners| {
            listeners.iter().any(|listener| {
                let Some(listener) = listener.as_mapping() else {
                    return false;
                };
                listener.get(YamlValue::from("name")).and_then(YamlValue::as_str) == Some(name)
                    && listener
                        .get(YamlValue::from("port"))
                        .and_then(YamlValue::as_u64)
                        == Some(port as u64)
                    && listener
                        .get(YamlValue::from("proxy"))
                        .and_then(YamlValue::as_str)
                        == group
            })
        })
}

#[cfg(windows)]
fn spawn_elevated(helper: &PathBuf, token_path: &PathBuf, config: &PathBuf) -> Result<ManagedProcess> {
    let quote = |value: &PathBuf| format!("\"{}\"", value.display());
    let parameters = format!(
        "--headless --token-file {} --config {}",
        quote(token_path),
        quote(config)
    );
    let file: Vec<u16> = std::ffi::OsStr::new(helper)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let verb: Vec<u16> = std::ffi::OsStr::new("runas")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let params: Vec<u16> = std::ffi::OsStr::new(&parameters)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params.as_ptr()),
        nShow: 0,
        ..Default::default()
    };
    unsafe {
        ShellExecuteExW(&mut info).context("request elevated Clew helper")?;
    }
    let handle = info.hProcess;
    ensure!(!handle.is_invalid(), "elevated helper returned no process handle");
    let pid = unsafe { GetProcessId(handle) };
    ensure!(pid != 0, "elevated helper returned no process id");
    Ok(ManagedProcess {
        handle: handle.0 as isize,
        pid,
    })
}

impl AppProxyManager {
    pub fn clear_startup_error(&self) {
        if let Ok(mut guard) = self.last_startup_error.write() {
            *guard = None;
        }
    }

    fn current_startup_error(&self) -> Option<String> {
        self.last_startup_error.read().ok().and_then(|guard| guard.clone())
    }

    async fn restore_staged_listeners(
        &self,
        old: &BTreeMap<String, ManagedListenerSpec>,
        staged: &[ManagedListenerSpec],
    ) -> Result<()> {
        *self.desired_listener.write().map_err(|_| anyhow!("managed listener state lock poisoned"))? =
            old.values().cloned().collect();
        for spec in staged {
            remove_listener(&spec.name).await?;
        }
        Ok(())
    }

    pub fn get_rules(&self) -> Vec<AppProxyRule> {
        self.rules.read().map(|g| g.clone()).unwrap_or_default()
    }

    async fn apply_rules_to_helper(
        client: &Client,
        token: &str,
        rules: &[AppProxyRule],
        groups: &[Value],
        expected_version: u64,
    ) -> std::result::Result<u64, RulePublishError> {
        let payload = json!({
            "expected_version": expected_version,
            "rules": rules,
            "proxy_groups": groups
        });
        let response = client
            .put(format!("{HELPER_API}/api/helper/v1/rules"))
            .bearer_auth(token)
            .json(&payload)
            .send()
            .await
            .context("send rules update to helper")
            .map_err(RulePublishError::Uncertain)?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let error = anyhow!("helper rejected rules update (status {status}): {body}");
            return Err(if status.as_u16() == 400 {
                RulePublishError::Rejected(error)
            } else {
                RulePublishError::Uncertain(error)
            });
        }
        let result: Value = response.json().await.context("decode rules response")
            .map_err(RulePublishError::Uncertain)?;
        let effective = result
            .get("effective_version")
            .and_then(Value::as_u64)
            .ok_or_else(|| RulePublishError::Uncertain(anyhow!("helper response missing effective_version")))?;
        Ok(effective)
    }

    pub async fn set_rules(&self, new_rules: Vec<AppProxyRule>) -> Result<AppProxyStatus> {
        let _update = self.rules_update_lock.lock().await;
        let result = self.set_rules_locked(new_rules).await;
        self.settle_rule_update(result).await
    }

    async fn settle_rule_update(&self, result: Result<AppProxyStatus>) -> Result<AppProxyStatus> {
        match result {
            Ok(status) => Ok(status),
            Err(error) => {
                let faulted = self.inner.lock().await.as_ref().is_some_and(|helper| helper.failed);
                if faulted {
                    if let Ok(mut guard) = self.last_startup_error.write() {
                        *guard = Some(format!("rule update stopped after an uncertain helper state: {error:#}"));
                    }
                    self.stop_inner(true).await.context("stop faulted application proxy")?;
                }
                Err(error)
            }
        }
    }

    async fn set_rules_locked(&self, new_rules: Vec<AppProxyRule>) -> Result<AppProxyStatus> {
        let mut inner_guard = self.inner.lock().await;
        if let Some(helper) = inner_guard.as_mut() {
            ensure!(!helper.failed && helper.process.is_alive(), "application proxy is not ready for rule updates");
            let primary = helper.strategy_group.clone();
            for group in required_groups(&new_rules, &primary) {
                let _ = strategy_binding(&group).await?;
            }
            let old_listeners = std::iter::once((primary.clone(), ManagedListenerSpec {
                name: helper.listener_name.clone(),
                port: helper.listener.rsplit(':').next().and_then(|value| value.parse().ok())
                    .ok_or_else(|| anyhow!("invalid primary listener"))?,
                group: strategy_binding(&primary).await?.group,
            })).chain(helper.extra_listeners.clone()).collect::<BTreeMap<_, _>>();
            let mut listeners = old_listeners.clone();
            let mut staged = Vec::new();
            for group in required_groups(&new_rules, &primary) {
                if listeners.contains_key(&group) { continue; }
                let binding = strategy_binding(&group).await?;
                let spec = ManagedListenerSpec {
                    name: format!("clash-verge-app-proxy-{}-{}", std::process::id(), nanoid::nanoid!(8)),
                    port: allocate_unused_listener_port(&listeners).await?,
                    group: binding.group,
                };
                listeners.insert(group, spec.clone());
                staged.push(spec);
            }
            let (compiled, groups) = compile_rules(&new_rules, &primary, &listeners)?;
            let (_, old_groups) = compile_rules(&helper.rules, &primary, &old_listeners)?;
            if let Ok(mut desired) = self.desired_listener.write() {
                *desired = listeners.values().cloned().collect();
            }
            for spec in &staged {
                let loaded = configure_listener(&spec.name, spec.port, spec.group.as_deref()).await;
                if loaded.is_err()
                    || !managed_listener_loaded(&spec.name, spec.port, spec.group.as_deref()).await
                    || !Self::listener_ready(&format!("127.0.0.1:{}", spec.port)).await {
                    if let Err(cleanup_error) = self.restore_staged_listeners(&old_listeners, &staged).await {
                        helper.failed = true;
                        return Err(cleanup_error.context("restore previous listeners after staging failure"));
                    }
                    return Err(loaded.err().unwrap_or_else(|| anyhow!("managed listener did not become ready")));
                }
            }
            let client = match Self::client().await {
                Ok(client) => client,
                Err(error) => {
                    if let Err(cleanup_error) = self.restore_staged_listeners(&old_listeners, &staged).await {
                        helper.failed = true;
                        return Err(cleanup_error.context("restore previous listeners after HTTP client failure"));
                    }
                    return Err(error);
                }
            };
            let published = Self::apply_rules_to_helper(
                &client,
                &helper.token,
                &compiled,
                &groups,
                helper.effective_rules_version,
            ).await;
            let version = match published {
                Ok(version) => version,
                Err(RulePublishError::Rejected(error)) => {
                    let observed = Self::observed_rules_revision(&client, &helper.token).await;
                    if observed == Some(helper.effective_rules_version) {
                        if let Err(cleanup_error) = self.restore_staged_listeners(&old_listeners, &staged).await {
                            helper.failed = true;
                            return Err(cleanup_error.context("restore previous listeners after rejected rules"));
                        }
                    } else {
                        helper.failed = true;
                    }
                    return Err(error.context(format!("helper rules_revision={observed:?}")));
                }
                Err(RulePublishError::Uncertain(error)) => {
                    let observed = Self::observed_rules_revision(&client, &helper.token).await;
                    helper.failed = true;
                    return Err(error.context(format!("uncertain rules update; helper rules_revision={observed:?}")));
                }
            };
            let helper_pid = helper.process.id();
            if let Err(error) = Self::wait_rules_applied(
                &client, &helper.token, Some(&compiled), &groups, version, &mut helper.process, helper_pid
            ).await {
                helper.failed = true;
                return Err(error.context("confirm new application rules"));
            }
            if let Err(error) = save_rules_to_disk(&new_rules) {
                let rollback = Self::apply_rules_to_helper(
                    &client, &helper.token, &helper.rules, &old_groups, version
                ).await;
                match rollback {
                    Ok(old_version) => {
                        let pid = helper.process.id();
                        if Self::wait_rules_applied(&client, &helper.token, Some(&helper.rules), &old_groups, old_version, &mut helper.process, pid).await.is_ok() {
                            if let Err(cleanup_error) = self.restore_staged_listeners(&old_listeners, &staged).await {
                                helper.failed = true;
                                return Err(cleanup_error.context("restore previous listeners after persistence failure"));
                            }
                            helper.effective_rules_version = old_version;
                        } else {
                            helper.failed = true;
                        }
                    }
                    Err(_) => helper.failed = true,
                }
                return Err(error.context("persist application rules"));
            }
            *self.rules.write().map_err(|_| anyhow!("application rules lock poisoned"))? = new_rules.clone();
            helper.effective_rules_version = version;
            helper.rules = compiled;
            helper.extra_listeners = listeners.into_iter()
                .filter(|(group, _)| group != &primary && required_groups(&new_rules, &primary).contains(group))
                .collect();
            let primary_spec = old_listeners.get(&primary).cloned().ok_or_else(|| anyhow!("missing primary listener"))?;
            if let Ok(mut desired) = self.desired_listener.write() {
                *desired = std::iter::once(primary_spec).chain(helper.extra_listeners.values().cloned()).collect();
            }
            for (group, spec) in &old_listeners {
                if group != &primary && !helper.extra_listeners.contains_key(group) {
                    if let Err(error) = remove_listener(&spec.name).await {
                        helper.failed = true;
                        return Err(error.context("retire obsolete managed listener"));
                    }
                }
            }
        } else {
            let validation_listeners = required_groups(&new_rules, "regular").into_iter().map(|group| {
                (group.clone(), ManagedListenerSpec { name: group, port: 0, group: None })
            }).collect();
            let _ = compile_rules(&new_rules, "regular", &validation_listeners)?;
            save_rules_to_disk(&new_rules)?;
            *self.rules.write().map_err(|_| anyhow!("application rules lock poisoned"))? = new_rules;
        }
        drop(inner_guard);
        Ok(self.status().await)
    }

    pub async fn add_or_update_rule(&self, rule: AppProxyRule) -> Result<AppProxyStatus> {
        let _update = self.rules_update_lock.lock().await;
        let mut current_rules = self.get_rules();
        if let Some(pos) = current_rules.iter().position(|r| r.id == rule.id) {
            current_rules[pos] = rule;
        } else {
            current_rules.push(rule);
        }
        let result = self.set_rules_locked(current_rules).await;
        self.settle_rule_update(result).await
    }

    pub async fn remove_rule(&self, rule_id: &str) -> Result<AppProxyStatus> {
        let _update = self.rules_update_lock.lock().await;
        let mut current_rules = self.get_rules();
        current_rules.retain(|r| r.id != rule_id);
        let result = self.set_rules_locked(current_rules).await;
        self.settle_rule_update(result).await
    }

    pub async fn toggle_rule(&self, rule_id: &str, enabled: bool) -> Result<AppProxyStatus> {
        let _update = self.rules_update_lock.lock().await;
        let mut current_rules = self.get_rules();
        if let Some(r) = current_rules.iter_mut().find(|r| r.id == rule_id) {
            r.enabled = enabled;
        }
        let result = self.set_rules_locked(current_rules).await;
        self.settle_rule_update(result).await
    }

    async fn client() -> Result<Client> {
        Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .context("build helper HTTP client")
    }

    async fn observed_rules_revision(client: &Client, token: &str) -> Option<u64> {
        let response = client
            .get(format!("{HELPER_API}/api/helper/v1/status"))
            .bearer_auth(token)
            .send()
            .await
            .ok()?;
        response
            .json::<Value>()
            .await
            .ok()?
            .get("rules_revision")
            .and_then(Value::as_u64)
    }

    fn helper_path() -> Result<PathBuf> {
        if cfg!(debug_assertions) {
            if let Ok(path) = std::env::var("CLEW_DEV_HELPER_PATH") {
                let path = PathBuf::from(path);
                if path.is_file() {
                    return Ok(path);
                }
            }
        }
        let path = crate::utils::dirs::app_resources_dir()?.join("clew.exe");
        ensure!(
            path.is_file(),
            "managed Clew helper is not installed: {}",
            path.display()
        );
        Ok(path)
    }

    fn write_token() -> Result<(PathBuf, String)> {
        let dir = crate::utils::dirs::app_home_dir()?.join("app-proxy");
        std::fs::create_dir_all(&dir).context("create app proxy credential directory")?;
        let token = format!("{}{}", std::process::id(), nanoid::nanoid!(48));
        let path = dir.join(format!("helper-{}.token", nanoid::nanoid!(16)));
        #[cfg(windows)]
        let mut file = crate::core::owner_identity::create_private_current_user_file(&path)
            .context("create protected helper credential")?;
        #[cfg(unix)]
        let mut file = {
            use std::os::unix::fs::OpenOptionsExt;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            options.mode(0o600);
            options.open(&path).context("create helper credential")?
        };
        use std::io::Write;
        file.write_all(token.as_bytes()).context("write helper credential")?;
        file.sync_all().context("flush helper credential")?;
        Ok((path, token))
    }

    async fn remove_token(path: &PathBuf) -> Result<()> {
        match tokio::fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).context("remove helper credential"),
        }
    }

    async fn wait_ready(
        &self,
        client: &Client,
        token: &str,
        pid: Option<u32>,
        process: &mut ManagedProcess,
    ) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut last_handshake_error = None;
        let mut last_status_error = None;
        let mut last_http_status = None;
        let mut last_helper_ready = None;
        let mut last_protocol_version = None;
        let mut last_headless = None;
        let mut last_status_protocol_version = None;
        let mut last_status_headless = None;
        loop {
            match process.exit_code() {
                Ok(Some(code)) => {
                    return Err(anyhow!(
                        "helper exited before readiness (pid={pid:?}, exit_code={code}, last_http_status={last_http_status:?}, last_helper_ready={last_helper_ready:?}, last_protocol_version={last_protocol_version:?}, last_headless={last_headless:?}, last_status_protocol_version={last_status_protocol_version:?}, last_status_headless={last_status_headless:?}, last_handshake_error={last_handshake_error:?}, last_status_error={last_status_error:?})"
                    ));
                }
                Ok(None) => {}
                Err(error) => last_status_error = Some(format!("process status query failed: {error:#}")),
            }
            if Instant::now() >= deadline {
                return Err(anyhow!(
                    "helper readiness timeout (pid={pid:?}, last_http_status={last_http_status:?}, last_helper_ready={last_helper_ready:?}, last_protocol_version={last_protocol_version:?}, last_headless={last_headless:?}, last_status_protocol_version={last_status_protocol_version:?}, last_status_headless={last_status_headless:?}, last_handshake_error={last_handshake_error:?}, last_status_error={last_status_error:?})"
                ));
            }
            match client.get(format!("{HELPER_API}/api/helper/v1/handshake")).send().await {
                Ok(response) if response.status().is_success() => match response.json::<Value>().await {
                    Ok(body) => {
                        last_protocol_version = body.get("protocol_version").and_then(Value::as_u64);
                        last_headless = body.get("headless").and_then(Value::as_bool);
                        last_handshake_error = None;
                    }
                    Err(error) => last_handshake_error = Some(format!("decode helper handshake: {error:#}")),
                },
                Ok(response) => last_handshake_error = Some(format!("handshake returned HTTP {}", response.status())),
                Err(error) => last_handshake_error = Some(format!("handshake request failed: {error:#}")),
            }
            match client
                .get(format!("{HELPER_API}/api/helper/v1/status"))
                .bearer_auth(token)
                .send()
                .await
            {
                Ok(response) => {
                    let status = response.status();
                    last_http_status = Some(status.as_u16());
                    if status.is_success() {
                        match response.json::<Value>().await {
                            Ok(body) => {
                                last_helper_ready = body.get("helper_ready").and_then(Value::as_bool);
                                last_status_protocol_version = body.get("protocol_version").and_then(Value::as_u64);
                                last_status_headless = body.get("headless").and_then(Value::as_bool);
                                if last_helper_ready == Some(true) {
                                    logging!(info, Type::Cmd,
                                        "app proxy helper ready pid={pid:?} handshake_protocol_version={last_protocol_version:?} handshake_headless={last_headless:?} status_protocol_version={last_status_protocol_version:?} status_headless={last_status_headless:?} http_status={last_http_status:?}");
                                    return Ok(());
                                }
                                last_status_error = Some(String::from("status response reported helper_ready=false"));
                            }
                            Err(error) => last_status_error = Some(format!("decode helper status: {error:#}")),
                        }
                    } else {
                        last_status_error = Some(format!("status endpoint returned HTTP {status}"));
                    }
                }
                Err(error) => last_status_error = Some(format!("status request failed: {error:#}")),
            }
            sleep(Duration::from_millis(100)).await;
        }
    }

    async fn cleanup_failed(mut process: ManagedProcess, token_path: PathBuf, listeners: Vec<ManagedListenerSpec>) {
        let pid = process.id();
        let before = process.exit_code().ok().flatten();
        logging!(warn, Type::Cmd, "app proxy cleanup_failed pid={pid:?} exit_code_before_kill={before:?}");
        process.kill().await;
        process.wait().await;
        let after = process.exit_code().ok().flatten();
        logging!(info, Type::Cmd, "app proxy cleanup_failed pid={pid:?} exit_code_after_wait={after:?}");
        if let Err(error) = Self::remove_token(&token_path).await {
            logging!(error, Type::Cmd, "app proxy failed-start credential cleanup failed: {error:#}");
        }
        for listener in listeners {
            clear_desired_listener(&listener.name);
            let _ = remove_listener(&listener.name).await;
        }
    }

    async fn write_config(groups: &[Value]) -> Result<PathBuf> {
        let dir = crate::utils::dirs::app_home_dir()?.join("app-proxy");
        tokio::fs::create_dir_all(&dir)
            .await
            .context("create managed Clew config directory")?;
        let path = dir.join("clew.json");
        let config = json!({
            "version": 2,
            "default_proxy": {"host":"127.0.0.1", "port": groups[0]["port"]},
            "proxy_groups": groups,
            "next_group_id": groups.len(),
            "auto_rules": [], "default_exclude_cidrs": ["127.0.0.0/8","10.0.0.0/8","172.16.0.0/12","192.168.0.0/16"],
            "io_threads": 2, "log_level":"info", "dns":{"enabled":false},
            "tcp_syn_parking":{"enabled":true,"watchdog_ms":20,"pool_size":256}
        });
        tokio::fs::write(&path, serde_json::to_vec_pretty(&config)?)
            .await
            .context("write managed Clew config")?;
        Ok(path)
    }

    async fn listener_ready(listener: &str) -> bool {
        timeout(Duration::from_millis(500), TcpStream::connect(listener))
            .await
            .is_ok_and(|r| r.is_ok())
    }

    async fn api_port_free() -> bool {
        tokio::net::TcpListener::bind("127.0.0.1:18080").await.is_ok()
    }

    async fn wait_rules_applied(
        client: &Client,
        token: &str,
        expected_rules: Option<&[AppProxyRule]>,
        expected_groups: &[Value],
        expected_version: u64,
        process: &mut ManagedProcess,
        pid: Option<u32>,
    ) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match process.exit_code() {
                Ok(Some(code)) => {
                    return Err(anyhow!(
                        "helper exited while applying rules (pid={pid:?}, exit_code={code})"
                    ));
                }
                Ok(None) => {}
                Err(error) => return Err(error).context("query helper process while applying rules"),
            }
            if Instant::now() >= deadline {
                return Err(anyhow!(
                    "helper accepted rules but did not expose the requested runtime rule before the publication deadline"
                ));
            }
            let response = client
                .get(format!("{HELPER_API}/api/config"))
                .bearer_auth(token)
                .send()
                .await;
            if let Ok(response) = response {
                if response.status().is_success() {
                    if let Ok(config) = response.json::<Value>().await {
                        let group_ready = config
                            .get("proxy_groups")
                            .and_then(Value::as_array)
                            .is_some_and(|groups| {
                                groups.len() == expected_groups.len() && expected_groups.iter().all(|expected| groups.iter().any(|group| {
                                    group.get("id") == expected.get("id")
                                        && group.get("port") == expected.get("port")
                                        && group.get("host") == expected.get("host")
                                }))
                            });
                        let rule_ready = config.get("auto_rules").and_then(Value::as_array).is_some_and(|rules| {
                            if let Some(expected) = expected_rules {
                                rules.len() == expected.len() && expected.iter().all(|expected_rule| {
                                        rules.iter().any(|rule| {
                                            rule.get("id").and_then(Value::as_str) == Some(&expected_rule.id)
                                                && rule.get("cmdline_pattern").and_then(Value::as_str) == Some(&expected_rule.cmdline_pattern)
                                                && rule.get("proxy_group_id").and_then(Value::as_u64) == Some(expected_rule.proxy_group_id as u64)
                                                && rule.get("enabled").and_then(Value::as_bool) == Some(expected_rule.enabled)
                                                && rule.get("protocol").and_then(Value::as_str) == Some(&expected_rule.protocol)
                                                && rule.get("hack_tree").and_then(Value::as_bool) == Some(expected_rule.hack_tree)
                                        })
                                    })
                            } else {
                                true
                            }
                        });
                        let revision_ready = Self::observed_rules_revision(client, token).await == Some(expected_version);
                        if group_ready && rule_ready && revision_ready {
                            return Ok(());
                        }
                    }
                }
            }
            sleep(Duration::from_millis(50)).await;
        }
    }

    pub async fn start(&self, request: AppProxyStartRequest) -> Result<AppProxyStatus> {
        let _update = self.rules_update_lock.lock().await;
        self.clear_startup_error();
        let result = self.start_inner(request).await;
        if let Err(ref error) = result {
            if let Ok(mut guard) = self.last_startup_error.write() {
                *guard = Some(format!("{error:#}"));
            }
        }
        result
    }

    async fn start_inner(&self, request: AppProxyStartRequest) -> Result<AppProxyStatus> {
        let correlation_id = request.correlation_id.as_deref().unwrap_or("none");
        logging!(
            info,
            Type::Cmd,
            "app proxy start step=begin correlation_id={correlation_id} strategy_group={} process_rule={}",
            request.strategy_group,
            request.process_cmdline
        );
        let mut guard = self.inner.lock().await;
        if guard.is_some() {
            logging!(
                warn,
                Type::Cmd,
                "app proxy start step=reject reason=already_running correlation_id={correlation_id}"
            );
            ensure!(false, "app proxy helper is already running");
        }
        let effective_rules = if let Some(req_rules) = request.rules.as_ref() {
            if !req_rules.is_empty() {
                req_rules.clone()
            } else {
                self.get_rules()
            }
        } else if !request.process_cmdline.trim().is_empty() {
            vec![AppProxyRule {
                id: "verge-app-proxy".into(),
                name: "Verge selected application".into(),
                enabled: true,
                process_name: "*".into(),
                cmdline_pattern: request.process_cmdline.clone(),
                hack_tree: false,
                proxy_group_id: 0,
                strategy_group: request.strategy_group.clone(),
                protocol: "tcp".into(),
            }]
        } else {
            self.get_rules()
        };

        let has_enabled_rules = effective_rules.iter().any(|r| r.enabled);
        if !has_enabled_rules {
            logging!(
                warn,
                Type::Cmd,
                "app proxy start step=reject reason=no_enabled_rules correlation_id={correlation_id}"
            );
            ensure!(false, "no enabled process proxy rules configured");
        }
        let api_free = Self::api_port_free().await;
        logging!(
            info,
            Type::Cmd,
            "app proxy start step=listener_check api_port_free={} correlation_id={correlation_id}",
            api_free
        );
        ensure!(api_free, "Clew helper API port 18080 is already occupied");
        let primary_group = "regular";
        let binding = strategy_binding(primary_group).await?;
        for group in required_groups(&effective_rules, primary_group) {
            let _ = strategy_binding(&group).await?;
        }
        let listener_name = format!("clash-verge-app-proxy-{}", std::process::id());
        let listener_port = allocate_listener_port().await?;
        let primary = ManagedListenerSpec {
            name: listener_name.clone(),
            port: listener_port,
            group: binding.group.clone(),
        };
        let mut listeners = BTreeMap::from([(primary_group.to_owned(), primary.clone())]);
        for group in required_groups(&effective_rules, primary_group) {
            if listeners.contains_key(&group) { continue; }
            let port = allocate_unused_listener_port(&listeners).await?;
            listeners.insert(group.clone(), ManagedListenerSpec {
                name: format!("clash-verge-app-proxy-{}-{}", std::process::id(), nanoid::nanoid!(8)),
                port,
                group: Some(group),
            });
        }
        let (compiled_rules, groups) = compile_rules(&effective_rules, primary_group, &listeners)?;
        let all_listeners: Vec<_> = listeners.values().cloned().collect();
        *self.desired_listener.write().map_err(|_| anyhow!("managed listener state lock poisoned"))? = all_listeners.clone();
        for spec in &all_listeners {
            let loaded = configure_listener(&spec.name, spec.port, spec.group.as_deref()).await;
            if loaded.is_err()
                || !managed_listener_loaded(&spec.name, spec.port, spec.group.as_deref()).await
                || !Self::listener_ready(&format!("127.0.0.1:{}", spec.port)).await {
                for staged in &all_listeners {
                    clear_desired_listener(&staged.name);
                    let _ = remove_listener(&staged.name).await;
                }
                return Err(loaded.err().unwrap_or_else(|| anyhow!("managed Mihomo listener is not ready")));
            }
        }
        let listener = format!("127.0.0.1:{listener_port}");
        let listener_ready = Self::listener_ready(&listener).await;
        logging!(
            info,
            Type::Cmd,
            "app proxy start step=listener_check name={} listener={} ready={} correlation_id={correlation_id}",
            listener_name,
            listener,
            listener_ready
        );
        if !listener_ready {
            for spec in &all_listeners {
                clear_desired_listener(&spec.name);
                let _ = remove_listener(&spec.name).await;
            }
            return Err(anyhow!("managed Mihomo listener is not ready: {listener}"));
        }
        let preparation = async {
            let helper = Self::helper_path()?;
            let config = Self::write_config(&groups).await?;
            let token = Self::write_token()?;
            Ok::<_, anyhow::Error>((helper, config, token))
        }.await;
        let (helper, config, (token_path, token)) = match preparation {
            Ok(prepared) => prepared,
            Err(error) => {
                for spec in &all_listeners {
                    clear_desired_listener(&spec.name);
                    let _ = remove_listener(&spec.name).await;
                }
                return Err(error);
            }
        };
        logging!(
            info,
            Type::Cmd,
            "app proxy start step=config_generated path={} correlation_id={correlation_id}",
            config.display()
        );
        logging!(info, Type::Cmd, "app proxy helper path={} correlation_id={correlation_id}", helper.display());
        logging!(
            info,
            Type::Cmd,
            "app proxy start step=credential_created path={} acl=verified correlation_id={correlation_id}",
            token_path.display()
        );
        #[cfg(windows)]
        let mut process = match spawn_elevated(&helper, &token_path, &config) {
            Ok(process) => process,
            Err(error) => {
                if let Err(cleanup_error) = Self::remove_token(&token_path).await {
                    logging!(error, Type::Cmd, "app proxy spawn-failure credential cleanup failed: {cleanup_error:#}");
                }
                for spec in &all_listeners {
                    clear_desired_listener(&spec.name);
                    let _ = remove_listener(&spec.name).await;
                }
                return Err(error);
            }
        };
        #[cfg(not(windows))]
        let process = match Command::new(helper)
            .args(["--headless", "--token-file"])
            .arg(&token_path)
            .args(["--config"])
            .arg(&config)
            .spawn()
        {
            Ok(child) => ManagedProcess(child),
            Err(error) => {
                if let Err(cleanup_error) = Self::remove_token(&token_path).await {
                    logging!(error, Type::Cmd, "app proxy spawn-failure credential cleanup failed: {cleanup_error:#}");
                }
                for spec in &all_listeners {
                    clear_desired_listener(&spec.name);
                    let _ = remove_listener(&spec.name).await;
                }
                return Err(error).context("start managed Clew helper");
            }
        };
        let pid = process.id();
        logging!(
            info,
            Type::Cmd,
            "app proxy start step=helper_spawned pid={pid:?} elevation=requested correlation_id={correlation_id}"
        );
        let client = match Self::client().await {
            Ok(v) => v,
            Err(e) => {
                Self::cleanup_failed(process, token_path, all_listeners.clone()).await;
                return Err(e);
            }
        };
        if let Err(e) = self.wait_ready(&client, &token, pid, &mut process).await {
            Self::cleanup_failed(process, token_path, all_listeners.clone()).await;
            return Err(e);
        }
        logging!(
            info,
            Type::Cmd,
            "app proxy start step=auth_ready helper_ready=true pid={pid:?} correlation_id={correlation_id}"
        );
        let rules = json!({
            "expected_version": 0,
            "rules": compiled_rules,
            "proxy_groups": groups
        });
        logging!(info, Type::Cmd, "app proxy rules_request_started pid={pid:?} correlation_id={correlation_id}");
        let response = match client
            .put(format!("{HELPER_API}/api/helper/v1/rules"))
            .bearer_auth(&token)
            .json(&rules)
            .send()
            .await
        {
            Ok(v) => v,
            Err(e) => {
                logging!(error, Type::Cmd,
                    "app proxy rules_request_failed pid={pid:?} error={e:#} correlation_id={correlation_id}");
                Self::cleanup_failed(process, token_path, all_listeners.clone()).await;
                return Err(e.into());
            }
        };
        logging!(info, Type::Cmd,
            "app proxy rules_response pid={pid:?} status={} correlation_id={correlation_id}", response.status());
        if !response.status().is_success() {
            let status = response.status();
            Self::cleanup_failed(process, token_path, all_listeners.clone()).await;
            return Err(anyhow!("helper rejected rules: {status}"));
        }
        #[cfg(debug_assertions)]
        if let Ok(barrier_path) = std::env::var("CLEW_DEV_RULE_CONFIRM_BARRIER_FILE") {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !tokio::fs::try_exists(&barrier_path).await.unwrap_or(false) {
                if Instant::now() >= deadline {
                    Self::cleanup_failed(process, token_path, all_listeners.clone()).await;
                    return Err(anyhow!("debug rule-confirmation barrier timed out"));
                }
                sleep(Duration::from_millis(25)).await;
            }
        }
        let result: Value = match response.json().await.context("decode rule version") {
            Ok(v) => v,
            Err(e) => {
                logging!(error, Type::Cmd,
                    "app proxy rules_response_decode_failed pid={pid:?} error={e:#} correlation_id={correlation_id}");
                Self::cleanup_failed(process, token_path, all_listeners.clone()).await;
                return Err(e);
            }
        };
        let version = match result.get("effective_version").and_then(Value::as_u64) {
            Some(v) => v,
            None => {
                Self::cleanup_failed(process, token_path, all_listeners.clone()).await;
                return Err(anyhow!("helper did not publish an effective rule version"));
            }
        };
        logging!(
            info,
            Type::Cmd,
            "app proxy start step=rules_published effective_version={} correlation_id={correlation_id}",
            version
        );
        if let Err(error) =
            Self::wait_rules_applied(&client, &token, Some(&compiled_rules), &groups, version, &mut process, pid).await
        {
            logging!(error, Type::Cmd,
                "app proxy rules_activation_failed pid={pid:?} error={error:#} correlation_id={correlation_id}");
            Self::cleanup_failed(process, token_path, all_listeners.clone()).await;
            return Err(error);
        }
        if let Err(error) = save_rules_to_disk(&effective_rules) {
            Self::cleanup_failed(process, token_path, all_listeners).await;
            return Err(error.context("persist application rules after helper activation"));
        }
        if let Ok(mut stored_rules) = self.rules.write() {
            *stored_rules = effective_rules;
        } else {
            Self::cleanup_failed(process, token_path, all_listeners).await;
            return Err(anyhow!("application rules lock poisoned"));
        }
        logging!(
            info,
            Type::Cmd,
            "app proxy start step=rules_active effective_version={} correlation_id={correlation_id}",
            version
        );
        *guard = Some(ManagedHelper {
            process,
            token_path,
            token,
            _process_cmdline: request.process_cmdline,
            rules: compiled_rules,
            listener,
            listener_name,
            extra_listeners: listeners.into_iter().filter(|(group, _)| group != primary_group).collect(),
            strategy_group: primary_group.to_owned(),
            effective_rules_version: version,
            failed: false,
        });
        APP_PROXY_RUNNING.store(true, Ordering::Release);
        drop(guard);
        let status = self.status().await;
        if !status.proxy_ready {
            let _ = self.stop_inner(true).await;
            return Err(anyhow!("application proxy did not reach full readiness after rule activation"));
        }
        logging!(
            info,
            Type::Cmd,
            "app proxy start step=complete state={} proxy_ready={} effective_version={:?} correlation_id={correlation_id}",
            status.state,
            status.proxy_ready,
            status.effective_rules_version
        );
        Ok(status)
    }


    async fn stop_inner(&self, remove_managed_listener: bool) -> Result<AppProxyStatus> {
        let Some(mut helper) = self.inner.lock().await.take() else {
            APP_PROXY_RUNNING.store(false, Ordering::Release);
            return Ok(self.status().await);
        };
        if let Ok(mut desired) = self.desired_listener.write() {
            desired.clear();
        }
        if let Ok(client) = Self::client().await {
            let _ = client
                .post(format!("{HELPER_API}/api/helper/v1/stop"))
                .bearer_auth(&helper.token)
                .send()
                .await;
        }
        let pid = helper.process.id();
        let exit_code_before_wait = helper.process.exit_code().ok().flatten();
        logging!(info, Type::Cmd,
            "app proxy stop helper_wait_started pid={pid:?} exit_code_before_wait={exit_code_before_wait:?}");
        let _ = timeout(Duration::from_secs(5), helper.process.wait()).await;
        let exit_code_after_wait = helper.process.exit_code().ok().flatten();
        logging!(info, Type::Cmd,
            "app proxy stop helper_wait_completed pid={pid:?} exit_code_after_wait={exit_code_after_wait:?}");
        if helper.process.is_alive() {
            logging!(warn, Type::Cmd, "app proxy stop helper_kill_started pid={pid:?}");
            helper.process.kill().await;
            helper.process.wait().await;
            let exit_code_after_kill = helper.process.exit_code().ok().flatten();
            logging!(info, Type::Cmd,
                "app proxy stop helper_kill_completed pid={pid:?} exit_code_after_kill={exit_code_after_kill:?}");
        }
        let token_cleanup = Self::remove_token(&helper.token_path).await;
        if let Err(error) = &token_cleanup {
            logging!(error, Type::Cmd, "app proxy stop credential cleanup failed: {error:#}");
        }
        if remove_managed_listener {
            let mut cleanup_error = None;
            for spec in std::iter::once(helper.listener_name.clone())
                .chain(helper.extra_listeners.values().map(|spec| spec.name.clone())) {
                if let Err(error) = remove_listener(&spec).await {
                    logging!(error, Type::Cmd, "app proxy stop could not remove managed listener name={} error={error:#}", spec);
                    cleanup_error = Some(error);
                }
            }
            if let Some(error) = cleanup_error {
                APP_PROXY_RUNNING.store(false, Ordering::Release);
                return Err(error);
            }
        } else {
            logging!(
                info,
                Type::System,
                "app proxy shutdown skipped managed listener reload during application exit name={}",
                helper.listener_name
            );
        }
        APP_PROXY_RUNNING.store(false, Ordering::Release);
        token_cleanup?;
        Ok(AppProxyStatus {
            state: "stopped".into(),
            helper_ready: false,
            proxy_ready: false,
            effective_rules_version: Some(helper.effective_rules_version),
            pid: None,
            strategy_group: Some(helper.strategy_group),
            runtime_id: phase3_runtime_id(),
            last_startup_error: self.current_startup_error(),
        })
    }

    pub async fn stop(&self) -> Result<AppProxyStatus> {
        let _update = self.rules_update_lock.lock().await;
        self.stop_inner(true).await
    }

    pub async fn status(&self) -> AppProxyStatus {
        let last_startup_error = self.current_startup_error();
        let mut guard = self.inner.lock().await;
        let Some(helper) = guard.as_mut() else {
            return AppProxyStatus {
                state: "stopped".into(),
                helper_ready: false,
                proxy_ready: false,
                effective_rules_version: None,
                pid: None,
                strategy_group: None,
                runtime_id: phase3_runtime_id(),
                last_startup_error,
            };
        };
        if !helper.process.is_alive() {
            let token_path = helper.token_path.clone();
            let token_cleanup = Self::remove_token(&token_path).await.err();
            let listener_names = std::iter::once(helper.listener_name.clone())
                .chain(helper.extra_listeners.values().map(|spec| spec.name.clone()))
                .collect::<Vec<_>>();
            if let Ok(mut desired) = self.desired_listener.write() { desired.clear(); }
            APP_PROXY_RUNNING.store(false, Ordering::Release);
            for name in listener_names { let _ = remove_listener(&name).await; }
            let exit_code = helper.process.exit_code().ok().flatten();
            let mut crash_err = format!("helper process terminated unexpectedly (pid={:?}, exit_code={exit_code:?})", helper.process.id());
            if let Some(error) = token_cleanup {
                crash_err.push_str(&format!("; credential cleanup failed: {error:#}"));
            }
            if let Ok(mut err_guard) = self.last_startup_error.write() {
                *err_guard = Some(crash_err.clone());
            }
            *guard = None;
            return AppProxyStatus {
                state: "failed".into(),
                helper_ready: false,
                proxy_ready: false,
                effective_rules_version: None,
                pid: None,
                strategy_group: None,
                runtime_id: phase3_runtime_id(),
                last_startup_error: Some(crash_err),
            };
        }
        let token = helper.token.clone();
        let listener = helper.listener.clone();
        let version = helper.effective_rules_version;
        let pid = helper.process.id();
        let strategy_group = helper.strategy_group.clone();
        let listener_name = helper.listener_name.clone();
        let extra_listeners = helper.extra_listeners.clone();
        let failed = helper.failed;
        let update_epoch = config_update_snapshot();
        drop(guard);
        let active_groups = self.get_rules().into_iter().filter(|rule| rule.enabled)
            .map(|rule| rule.strategy_group).collect::<BTreeSet<_>>();
        let displayed_group = if active_groups.len() > 1 {
            "multiple".to_owned()
        } else {
            active_groups.into_iter().next().unwrap_or(strategy_group.clone())
        };
        let (helper_ready, observed_revision) = match Self::client().await {
            Ok(client) => match client
                .get(format!("{HELPER_API}/api/helper/v1/status"))
                .bearer_auth(token)
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => response.json::<Value>().await.ok()
                    .map(|value| (
                        value.get("helper_ready").and_then(Value::as_bool).unwrap_or(false),
                        value.get("rules_revision").and_then(Value::as_u64),
                    )).unwrap_or((false, None)),
                _ => (false, None),
            },
            Err(_) => (false, None),
        };
        let primary_binding = strategy_binding(&strategy_group).await;
        let strategy_available = primary_binding.is_ok();
        let strategy_proxy_group = primary_binding.ok().and_then(|binding| binding.group);
        let listener_port = listener.rsplit(':').next().and_then(|port| port.parse::<u16>().ok());
        let listener_loaded = match listener_port {
            Some(port) => managed_listener_loaded(&listener_name, port, strategy_proxy_group.as_deref()).await,
            None => false,
        };
        let mut all_listeners_loaded = listener_loaded;
        for (group, spec) in &extra_listeners {
            if strategy_binding(group).await.is_err()
                || !managed_listener_loaded(&spec.name, spec.port, spec.group.as_deref()).await
                || !Self::listener_ready(&format!("127.0.0.1:{}", spec.port)).await {
                all_listeners_loaded = false;
            }
        }
        let update_stable = config_update_is_current(update_epoch);
        let proxy_ready = !failed
            && strategy_available
            && update_stable
            && helper_ready
            && all_listeners_loaded
            && Self::listener_ready(&listener).await
            && observed_revision == Some(version)
            && version > 0;
        AppProxyStatus {
            state: if proxy_ready { "running" } else { "degraded" }.into(),
            helper_ready,
            proxy_ready,
            effective_rules_version: Some(version),
            pid,
            strategy_group: Some(displayed_group),
            runtime_id: phase3_runtime_id(),
            last_startup_error,
        }
    }

    pub async fn get_connections(&self) -> Result<Vec<AppProxyConnection>> {
        let guard = self.inner.lock().await;
        let Some(helper) = guard.as_ref() else {
            drop(guard);
            return super::system_telemetry::get_system_tcp_connections();
        };
        let token = helper.token.clone();
        drop(guard);

        let client = Self::client().await?;
        let res = client
            .get(format!("{HELPER_API}/api/tcp"))
            .bearer_auth(token)
            .send()
            .await.context("query helper TCP connections")?;
        ensure!(res.status().is_success(), "helper TCP telemetry returned HTTP {}", res.status());
        res.json::<Vec<AppProxyConnection>>().await.context("decode helper TCP connections")
    }

    pub async fn get_process_tree(&self) -> Result<Vec<AppProxyProcessNode>> {
        let guard = self.inner.lock().await;
        let Some(helper) = guard.as_ref() else {
            drop(guard);
            return super::system_telemetry::get_system_process_tree();
        };
        let token = helper.token.clone();
        drop(guard);

        let client = Self::client().await?;
        let res = client
            .get(format!("{HELPER_API}/api/processes"))
            .bearer_auth(token)
            .send()
            .await.context("query helper process tree")?;
        ensure!(res.status().is_success(), "helper process telemetry returned HTTP {}", res.status());
        res.json::<Vec<AppProxyProcessNode>>().await.context("decode helper process tree")
    }

    pub async fn get_process_detail(&self, pid: u32) -> Result<AppProxyProcessDetail> {
        let guard = self.inner.lock().await;
        let Some(helper) = guard.as_ref() else {
            drop(guard);
            return super::system_telemetry::get_system_process_detail(pid);
        };
        let token = helper.token.clone();
        drop(guard);

        let client = Self::client().await?;
        let res = client
            .get(format!("{HELPER_API}/api/processes/{pid}/detail"))
            .bearer_auth(token)
            .send()
            .await.context("query helper process detail")?;
        ensure!(res.status().is_success(), "helper process detail returned HTTP {}", res.status());
        res.json::<AppProxyProcessDetail>().await.context("decode helper process detail")
    }

    pub async fn shutdown(&self) {
        let _update = self.rules_update_lock.lock().await;
        let _ = self.stop_inner(false).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_independent_application_outlets() {
        let make_rule = |id: &str, group: &str| AppProxyRule {
            id: id.into(), name: id.into(), enabled: true, process_name: "*".into(),
            cmdline_pattern: format!("*{id}.exe*"), hack_tree: false,
            proxy_group_id: 0, strategy_group: group.into(), protocol: "tcp".into(),
        };
        let rules = vec![make_rule("one", "regular"), make_rule("two", "GROUP_B")];
        let listeners = BTreeMap::from([
            ("regular".into(), ManagedListenerSpec { name: "regular-listener".into(), port: 12001, group: None }),
            ("GROUP_B".into(), ManagedListenerSpec { name: "fixed-listener".into(), port: 12002, group: Some("GROUP_B".into()) }),
        ]);
        let (compiled, groups) = compile_rules(&rules, "regular", &listeners).unwrap();
        assert_eq!((compiled[0].proxy_group_id, compiled[1].proxy_group_id), (0, 1));
        assert_eq!((groups[0]["port"].as_u64(), groups[1]["port"].as_u64()), (Some(12001), Some(12002)));
        assert_eq!(required_groups(&rules, "regular").len(), 2);
    }

    #[test]
    fn rejects_broad_interpreter_rules() {
        let rule = AppProxyRule {
            id: "node".into(), name: "Node".into(), enabled: true, process_name: "*".into(),
            cmdline_pattern: "*node.exe*".into(), hack_tree: false, proxy_group_id: 0,
            strategy_group: "regular".into(), protocol: "tcp".into(),
        };
        let listeners = BTreeMap::from([("regular".into(), ManagedListenerSpec {
            name: "regular-listener".into(), port: 12001, group: None,
        })]);
        assert!(compile_rules(&[rule], "regular", &listeners).is_err());
    }

    #[test]
    fn test_rule_serialization() {
        let rule = AppProxyRule {
            id: "edge-test".into(),
            name: "Microsoft Edge".into(),
            enabled: true,
            process_name: "*".into(),
            cmdline_pattern: "*msedge.exe*".into(),
            hack_tree: false,
            proxy_group_id: 0,
            strategy_group: "regular".into(),
            protocol: "tcp".into(),
        };

        let json = serde_json::to_string(&rule).expect("serialize rule");
        assert!(json.contains("\"id\":\"edge-test\""));
        assert!(json.contains("\"cmdline_pattern\":\"*msedge.exe*\""));
        assert!(json.contains("\"enabled\":true"));

        let deserialized: AppProxyRule = serde_json::from_str(&json).expect("deserialize rule");
        assert_eq!(deserialized.id, "edge-test");
        assert_eq!(deserialized.name, "Microsoft Edge");
        assert!(deserialized.enabled);
        assert_eq!(deserialized.cmdline_pattern, "*msedge.exe*");
    }

    #[test]
    fn test_rule_defaults() {
        let json = r#"{"id":"r1","name":"Chrome","cmdline_pattern":"chrome.exe"}"#;
        let rule: AppProxyRule = serde_json::from_str(json).expect("deserialize with defaults");
        assert_eq!(rule.process_name, "*");
        assert_eq!(rule.protocol, "tcp");
        assert!(!rule.hack_tree);
        assert_eq!(rule.proxy_group_id, 0);
        assert!(rule.enabled);
    }
}
