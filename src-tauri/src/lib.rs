use btleplug::{
    api::{
        Central, CentralState, CharPropFlags, Characteristic, Manager as _, Peripheral as _,
        ScanFilter, WriteType,
    },
    platform::Manager as BleManager,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager};
use tokio::time::{sleep, timeout};

const LISTENER_PORT: u16 = 17373;
const BLUETOOTH_SCAN_INTERVAL_MS: u64 = 700;
const BLUETOOTH_SCAN_STALE_MS: u128 = 10_000;
const BLUETOOTH_DEVICES_UPDATED_EVENT: &str = "bluetooth://devices-updated";
const BLUETOOTH_SCAN_STATE_EVENT: &str = "bluetooth://scan-state";
const PROTOCOL_RESPONSE_TIMEOUT_MS: u64 = 2_000;
const PROTOCOL_PREFERRED_CHAR_UUID: &str = "0000fff1-0000-1000-8000-00805f9b34fb";
const PROTOCOL_PREFERRED_SERVICE_UUID: &str = "0000fff0-0000-1000-8000-00805f9b34fb";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceSummary {
    id: String,
    name: String,
    rssi: Option<i32>,
    connectable: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BluetoothSettings {
    auto_reconnect: bool,
    last_device_id: Option<String>,
}

impl Default for BluetoothSettings {
    fn default() -> Self {
        Self {
            auto_reconnect: true,
            last_device_id: None,
        }
    }
}

#[derive(Debug)]
struct RuntimeState {
    connection_state: String,
    connected_device_id: Option<String>,
    connected_peripheral: Option<btleplug::platform::Peripheral>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            connection_state: "disconnected".to_string(),
            connected_device_id: None,
            connected_peripheral: None,
        }
    }
}

#[derive(Debug, Clone)]
struct ScannedDeviceRecord {
    summary: DeviceSummary,
    last_seen_ms: u128,
}

#[derive(Debug, Default)]
struct BluetoothScanState {
    running: bool,
    session_id: u64,
    stop_signal: Option<Arc<AtomicBool>>,
    devices: HashMap<String, ScannedDeviceRecord>,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanStatePayload {
    state: String,
    message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ValveStatus {
    running: bool,
    count: u32,
    state: u8,
    state_label: String,
    raw: String,
}

#[derive(Default)]
struct AppState {
    runtime: Mutex<RuntimeState>,
    listener: Mutex<Option<Child>>,
    bluetooth_scan: Mutex<BluetoothScanState>,
}

#[derive(Debug, Clone)]
struct HookPathsBuf {
    claude_dir: PathBuf,
    settings_path: PathBuf,
    script_dir: PathBuf,
    forwarder_path: PathBuf,
    listener_path: PathBuf,
    listener_pid_path: PathBuf,
    listener_log_path: PathBuf,
    log_path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HookPaths {
    claude_dir: String,
    settings_path: String,
    script_dir: String,
    forwarder_path: String,
    listener_path: String,
    log_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HookInstallStatus {
    settings_exists: bool,
    hooks_exists: bool,
    script_files_ready: bool,
    listener_running: bool,
    paths: HookPaths,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallClaudeHooksResult {
    ok: bool,
    backup_path: Option<String>,
    settings_path: String,
    script_dir: String,
    overwritten_hooks: bool,
    listener_started: bool,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListenerStatus {
    running: bool,
    pid: Option<u32>,
    port: u16,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HookTip {
    time: String,
    event: String,
    detail: String,
}

fn bluetooth_settings_path() -> PathBuf {
    let mut dir = dirs::config_dir().unwrap_or_else(std::env::temp_dir);
    dir.push("ClaudeNotifyBot");
    dir.push("bluetooth_settings.json");
    dir
}

fn read_settings() -> BluetoothSettings {
    let path = bluetooth_settings_path();
    let content = fs::read_to_string(path);

    match content {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => BluetoothSettings::default(),
    }
}

fn write_settings(settings: &BluetoothSettings) -> Result<(), String> {
    let path = bluetooth_settings_path();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建配置目录失败: {error}"))?;
    }

    let data =
        serde_json::to_string_pretty(settings).map_err(|error| format!("序列化失败: {error}"))?;
    fs::write(path, data).map_err(|error| format!("写入配置失败: {error}"))
}

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(std::env::temp_dir)
}

fn file_exists(path: &Path) -> bool {
    fs::metadata(path)
        .map(|meta| meta.is_file())
        .unwrap_or(false)
}

fn node_file_name() -> &'static str {
    if cfg!(windows) {
        "node.exe"
    } else {
        "node"
    }
}

fn normalize_node_candidate(path: PathBuf) -> Option<PathBuf> {
    if file_exists(&path) {
        return Some(path);
    }

    if path.is_dir() {
        let candidate = path.join(node_file_name());
        if file_exists(&candidate) {
            return Some(candidate);
        }
    }

    None
}

fn newest_path_by_modified(paths: Vec<PathBuf>) -> Option<PathBuf> {
    paths
        .into_iter()
        .filter(|path| file_exists(path))
        .max_by_key(|path| {
            fs::metadata(path)
                .and_then(|meta| meta.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        })
}

fn node_from_path_lookup() -> Option<PathBuf> {
    let mut command = if cfg!(windows) {
        let mut cmd = Command::new("where");
        cmd.arg("node");
        cmd
    } else {
        let mut cmd = Command::new("/bin/zsh");
        cmd.arg("-lc").arg("command -v node");
        cmd
    };

    let output = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    for line in raw.lines() {
        let trimmed = line.trim().trim_matches('"');
        if trimmed.is_empty() {
            continue;
        }
        if let Some(path) = normalize_node_candidate(PathBuf::from(trimmed)) {
            return Some(path);
        }
    }

    None
}

fn node_from_nvm(home: &Path) -> Option<PathBuf> {
    let versions_dir = home.join(".nvm").join("versions").join("node");
    let mut candidates = Vec::new();

    if let Ok(entries) = fs::read_dir(versions_dir) {
        for entry in entries.flatten() {
            candidates.push(entry.path().join("bin").join(node_file_name()));
        }
    }

    newest_path_by_modified(candidates)
}

fn node_from_fnm(home: &Path) -> Option<PathBuf> {
    let versions_dir = home.join(".fnm").join("node-versions");
    let mut candidates = Vec::new();

    if let Ok(entries) = fs::read_dir(versions_dir) {
        for entry in entries.flatten() {
            candidates.push(
                entry
                    .path()
                    .join("installation")
                    .join("bin")
                    .join(node_file_name()),
            );
        }
    }

    newest_path_by_modified(candidates)
}

fn node_from_nvm_windows() -> Option<PathBuf> {
    if let Ok(sym) = std::env::var("NVM_SYMLINK") {
        if let Some(path) = normalize_node_candidate(PathBuf::from(sym)) {
            return Some(path);
        }
    }

    let nvm_home = std::env::var("NVM_HOME").ok()?;
    let mut candidates = Vec::new();
    if let Ok(entries) = fs::read_dir(nvm_home) {
        for entry in entries.flatten() {
            candidates.push(entry.path().join(node_file_name()));
        }
    }
    newest_path_by_modified(candidates)
}

fn resolve_node_binary() -> Option<PathBuf> {
    if let Ok(override_path) = std::env::var("CLAUDE_NOTIFY_NODE_PATH") {
        if let Some(path) = normalize_node_candidate(PathBuf::from(override_path)) {
            return Some(path);
        }
    }

    if let Some(path) = node_from_path_lookup() {
        return Some(path);
    }

    let home = home_dir();

    let mut common_candidates = vec![home.join(".volta").join("bin").join(node_file_name())];
    if cfg!(windows) {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            common_candidates.push(
                PathBuf::from(local_app_data)
                    .join("Programs")
                    .join("nodejs")
                    .join(node_file_name()),
            );
        }
        if let Ok(program_files) = std::env::var("ProgramFiles") {
            common_candidates.push(
                PathBuf::from(program_files)
                    .join("nodejs")
                    .join(node_file_name()),
            );
        }
        if let Ok(program_files_x86) = std::env::var("ProgramFiles(x86)") {
            common_candidates.push(
                PathBuf::from(program_files_x86)
                    .join("nodejs")
                    .join(node_file_name()),
            );
        }
        common_candidates.push(
            home.join("scoop")
                .join("apps")
                .join("nodejs")
                .join("current")
                .join(node_file_name()),
        );
    } else {
        common_candidates.push(home.join(".asdf").join("shims").join(node_file_name()));
        common_candidates.push(PathBuf::from("/opt/homebrew/bin/node"));
        common_candidates.push(PathBuf::from("/usr/local/bin/node"));
        common_candidates.push(PathBuf::from("/usr/bin/node"));
    }

    for candidate in common_candidates {
        if let Some(path) = normalize_node_candidate(candidate) {
            return Some(path);
        }
    }

    if cfg!(windows) {
        if let Some(path) = node_from_nvm_windows() {
            return Some(path);
        }
    } else {
        if let Some(path) = node_from_nvm(&home) {
            return Some(path);
        }

        if let Some(path) = node_from_fnm(&home) {
            return Some(path);
        }
    }

    None
}

fn node_binary_or_error() -> Result<PathBuf, String> {
    resolve_node_binary().ok_or_else(|| {
        "未找到可用的 node 可执行文件。请先安装 Node，或设置环境变量 CLAUDE_NOTIFY_NODE_PATH 指向 node 可执行文件。".to_string()
    })
}

fn node_command_token() -> String {
    match resolve_node_binary() {
        Some(path) => quote_path(&path),
        None => "node".to_string(),
    }
}

fn hook_paths_buf() -> HookPathsBuf {
    let claude_dir = home_dir().join(".claude");
    let script_dir = claude_dir.join("claude-notify-bot");

    HookPathsBuf {
        settings_path: claude_dir.join("settings.json"),
        forwarder_path: script_dir.join("hook-forwarder.mjs"),
        listener_path: script_dir.join("listener.mjs"),
        listener_pid_path: script_dir.join("listener.pid"),
        listener_log_path: script_dir.join("listener.log"),
        log_path: script_dir.join("forwarder.log"),
        script_dir,
        claude_dir,
    }
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn hook_paths() -> HookPaths {
    let paths = hook_paths_buf();
    HookPaths {
        claude_dir: path_to_string(&paths.claude_dir),
        settings_path: path_to_string(&paths.settings_path),
        script_dir: path_to_string(&paths.script_dir),
        forwarder_path: path_to_string(&paths.forwarder_path),
        listener_path: path_to_string(&paths.listener_path),
        log_path: path_to_string(&paths.log_path),
    }
}

fn quote_path(path: &Path) -> String {
    format!("\"{}\"", path_to_string(path).replace('"', "\\\""))
}

fn listener_script_content() -> &'static str {
    r#"#!/usr/bin/env node
import http from "node:http";

const portArg = process.argv.find((x) => x.startsWith("--port="));
const port = portArg ? Number(portArg.split("=")[1]) : 17373;

function safeJsonParse(text) {
  try { return JSON.parse(text); } catch { return null; }
}

function normalizeClaudeEvent(payload) {
  const event = payload?.hook_event_name ?? "unknown";
  const details = [];

  if (payload?.session_id) details.push(`session_id=${payload.session_id}`);

  if (event === "Notification") {
    if (payload?.notification_type) details.push(`notification_type=${payload.notification_type}`);
    if (payload?.title) details.push(`title=${payload.title}`);
  }

  if (event === "PermissionRequest") {
    if (payload?.tool_name) details.push(`tool=${payload.tool_name}`);
    if (payload?.tool_input?.command) details.push(`command=${payload.tool_input.command}`);
  }

  if (event === "Elicitation") {
    if (payload?.mcp_server_name) details.push(`mcp_server=${payload.mcp_server_name}`);
    if (payload?.mode) details.push(`mode=${payload.mode}`);
  }

  if (event === "Stop") {
    if (payload?.stop_hook_active !== undefined) details.push(`stop_hook_active=${payload.stop_hook_active}`);
  }

  return { event, detail: details.join(", ") || "-" };
}

function mapToSignal(payload) {
  const event = payload?.hook_event_name;
  const notificationType = payload?.notification_type;

  if (event === "PermissionRequest") {
    return { code: "APPROVAL", pattern: "JUMP x2" };
  }

  if (event === "Notification" && (notificationType === "permission_prompt" || notificationType === "elicitation_dialog")) {
    return { code: "APPROVAL", pattern: "JUMP x2" };
  }

  if (event === "Stop") {
    return { code: "DONE", pattern: "JUMP x1" };
  }

  if (event === "UserPromptSubmit") {
    return { code: "START", pattern: "JUMP x1" };
  }

  return { code: "IGNORE", pattern: "-" };
}

const server = http.createServer((req, res) => {
  if (req.method === "GET" && req.url === "/health") {
    res.writeHead(200, { "Content-Type": "application/json; charset=utf-8" });
    res.end(JSON.stringify({ ok: true, port }));
    return;
  }

  if (req.method !== "POST" || req.url !== "/claude/event") {
    res.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
    res.end("Not Found");
    return;
  }

  let body = "";
  req.setEncoding("utf8");
  req.on("data", (chunk) => { body += chunk; });
  req.on("end", () => {
    const raw = body.trim();
    const parsed = safeJsonParse(raw) ?? { hook_event_name: "raw-text", raw };
    const info = normalizeClaudeEvent(parsed);
    const signal = mapToSignal(parsed);
    const now = new Date().toISOString();

    console.log("\\n================ CLAUDE EVENT ================");
    console.log(`[time]   ${now}`);
    console.log(`[event]  ${info.event}`);
    console.log(`[detail] ${info.detail}`);
    console.log(`[signal] ${signal.code}`);
    console.log(`[action] ${signal.pattern}`);
    if (parsed?.message) console.log(`[message] ${parsed.message}`);
    console.log(`[raw]    ${raw || "<empty>"}`);
    console.log("==============================================");

    res.writeHead(200, { "Content-Type": "application/json; charset=utf-8" });
    res.end(JSON.stringify({ ok: true, signal: signal.code }));
  });
});

server.listen(port, "127.0.0.1", () => {
  console.log(`监听已启动: http://127.0.0.1:${port}/claude/event`);
  console.log("健康检查: GET /health");
  console.log("按 Ctrl+C 退出。\\n");
});

server.on("error", (err) => {
  console.error("监听服务异常:", err);
  process.exit(1);
});
"#
}

fn forwarder_script_content() -> &'static str {
    r#"#!/usr/bin/env node
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const portArg = process.argv.find((x) => x.startsWith("--port="));
const logArg = process.argv.find((x) => x.startsWith("--log="));

const port = portArg ? Number(portArg.split("=")[1]) : 17373;
const logPath = logArg ? logArg.substring("--log=".length) : path.join(__dirname, "forwarder.log");

function safeParse(text) {
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

function writeLogLine(rawPayload) {
  const parsed = safeParse(rawPayload || "{}");
  const line = {
    time: new Date().toISOString(),
    hook_event_name: parsed?.hook_event_name ?? "unknown",
    notification_type: parsed?.notification_type ?? null,
    cwd: parsed?.cwd ?? null,
    session_id: parsed?.session_id ?? null,
    raw: rawPayload || "{}",
  };

  try {
    fs.appendFileSync(logPath, JSON.stringify(line) + "\n", "utf8");
  } catch (err) {
    console.error(`[hook-forwarder] 写日志失败: ${err.message}`);
  }
}

let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  input += chunk;
});

process.stdin.on("end", () => {
  const payload = input.trim() || "{}";
  writeLogLine(payload);

  const req = http.request(
    {
      hostname: "127.0.0.1",
      port,
      path: "/claude/event",
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "Content-Length": Buffer.byteLength(payload),
      },
      timeout: 1500,
    },
    (res) => {
      res.resume();
      res.on("end", () => process.exit(0));
    },
  );

  req.on("timeout", () => {
    req.destroy(new Error("timeout"));
  });

  req.on("error", (err) => {
    console.error(`[hook-forwarder] 转发失败: ${err.message}`);
    process.exit(0);
  });

  req.write(payload);
  req.end();
});
"#
}

fn ensure_log_file(path: &Path) -> Result<(), String> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map(|_| ())
        .map_err(|error| format!("创建日志文件失败: {error}"))
}

fn read_listener_pid(path: &Path) -> Option<u32> {
    let raw = fs::read_to_string(path).ok()?;
    raw.trim().parse::<u32>().ok()
}

fn write_listener_pid(path: &Path, pid: u32) -> Result<(), String> {
    fs::write(path, format!("{pid}\n")).map_err(|error| format!("写入 listener.pid 失败: {error}"))
}

fn remove_listener_pid(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(unix)]
fn is_pid_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_pid_alive(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
fn terminate_pid(pid: u32) {
    let pid_str = pid.to_string();
    let _ = Command::new("kill")
        .arg(&pid_str)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    for _ in 0..10 {
        if !is_pid_alive(pid) {
            return;
        }
        thread::sleep(Duration::from_millis(80));
    }

    let _ = Command::new("kill")
        .arg("-9")
        .arg(pid_str)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(unix))]
fn terminate_pid(_pid: u32) {}

#[cfg(unix)]
fn kill_rogue_listener_by_path(paths: &HookPathsBuf) {
    let _ = Command::new("pkill")
        .arg("-f")
        .arg(path_to_string(&paths.listener_path))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(unix))]
fn kill_rogue_listener_by_path(_paths: &HookPathsBuf) {}

fn write_hook_scripts() -> Result<(), String> {
    let paths = hook_paths_buf();

    fs::create_dir_all(&paths.script_dir).map_err(|error| format!("创建脚本目录失败: {error}"))?;
    fs::write(&paths.forwarder_path, forwarder_script_content())
        .map_err(|error| format!("写入 hook-forwarder.mjs 失败: {error}"))?;
    fs::write(&paths.listener_path, listener_script_content())
        .map_err(|error| format!("写入 listener.mjs 失败: {error}"))?;

    ensure_log_file(&paths.log_path)?;
    ensure_log_file(&paths.listener_log_path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o755);
        let _ = fs::set_permissions(&paths.forwarder_path, perms.clone());
        let _ = fs::set_permissions(&paths.listener_path, perms);
    }

    Ok(())
}

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn external_script_paths() -> (PathBuf, PathBuf) {
    let home = home_dir();
    (
        home.join(".claudeCodeChange")
            .join("tools")
            .join("claudeChangePreToolUse.js"),
        home.join(".claudeCodeChange")
            .join("tools")
            .join("claudeChangeStop.js"),
    )
}

fn hook_template_value(paths: &HookPathsBuf) -> Value {
    let (pre_tool_script, stop_script) = external_script_paths();
    let node_cmd = node_command_token();

    let pre_tool_cmd = format!("{node_cmd} {}", quote_path(&pre_tool_script));
    let stop_cmd = format!("{node_cmd} {}", quote_path(&stop_script));

    let forwarder_with_log_cmd = format!(
        "{node_cmd} {} --port={} --log={}",
        quote_path(&paths.forwarder_path),
        LISTENER_PORT,
        quote_path(&paths.log_path)
    );

    let forwarder_cmd = format!(
        "{node_cmd} {} --port={}",
        quote_path(&paths.forwarder_path),
        LISTENER_PORT
    );

    json!({
      "PreToolUse": [
        {
          "matcher": "MultiEdit|Edit|Write",
          "hooks": [
            {
              "type": "command",
              "command": pre_tool_cmd
            }
          ]
        },
        {
          "matcher": "Bash|Edit|Write|MultiEdit",
          "hooks": [
            {
              "type": "command",
              "command": forwarder_with_log_cmd,
              "timeout": 2
            }
          ]
        }
      ],
      "Stop": [
        {
          "hooks": [
            {
              "type": "command",
              "command": stop_cmd
            }
          ]
        },
        {
          "hooks": [
            {
              "type": "command",
              "command": forwarder_cmd,
              "timeout": 2
            }
          ]
        }
      ],
      "Notification": [
        {
          "hooks": [
            {
              "type": "command",
              "command": forwarder_cmd,
              "timeout": 2
            }
          ]
        }
      ],
      "UserPromptSubmit": [
        {
          "hooks": [
            {
              "type": "command",
              "command": forwarder_cmd,
              "timeout": 2
            }
          ]
        }
      ],
      "PermissionRequest": [
        {
          "hooks": [
            {
              "type": "command",
              "command": forwarder_cmd,
              "timeout": 2
            }
          ]
        }
      ],
      "Elicitation": [
        {
          "hooks": [
            {
              "type": "command",
              "command": forwarder_cmd,
              "timeout": 2
            }
          ]
        }
      ],
      "ElicitationResult": [
        {
          "hooks": [
            {
              "type": "command",
              "command": forwarder_cmd,
              "timeout": 2
            }
          ]
        }
      ]
    })
}

fn external_script_warnings() -> Vec<String> {
    let mut warnings = Vec::new();
    let (pre_tool_script, stop_script) = external_script_paths();

    if !pre_tool_script.exists() {
        warnings.push(format!(
            "未找到外部脚本: {}",
            path_to_string(&pre_tool_script)
        ));
    }

    if !stop_script.exists() {
        warnings.push(format!("未找到外部脚本: {}", path_to_string(&stop_script)));
    }

    warnings
}

fn parse_hook_tip(line: &str) -> Option<HookTip> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    let time = value.get("time").and_then(Value::as_str)?.to_string();
    let event = value
        .get("hook_event_name")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let notification_type = value
        .get("notification_type")
        .and_then(Value::as_str)
        .unwrap_or("");

    let detail = match event.as_str() {
        "PermissionRequest" => "等待权限确认".to_string(),
        "UserPromptSubmit" => "用户提交了新请求".to_string(),
        "Stop" => "任务结束".to_string(),
        "Notification" if !notification_type.is_empty() => {
            format!("通知类型: {notification_type}")
        }
        _ => "Hook 触发".to_string(),
    };

    Some(HookTip {
        time,
        event,
        detail,
    })
}

fn listener_status_internal(state: &AppState) -> Result<ListenerStatus, String> {
    let paths = hook_paths_buf();
    let mut listener = state
        .listener
        .lock()
        .map_err(|_| "监听器状态锁已损坏".to_string())?;

    if let Some(child) = listener.as_mut() {
        match child.try_wait() {
            Ok(Some(_)) => {
                *listener = None;
                remove_listener_pid(&paths.listener_pid_path);
                Ok(ListenerStatus {
                    running: false,
                    pid: None,
                    port: LISTENER_PORT,
                })
            }
            Ok(None) => Ok(ListenerStatus {
                running: true,
                pid: Some(child.id()),
                port: LISTENER_PORT,
            }),
            Err(_) => {
                *listener = None;
                remove_listener_pid(&paths.listener_pid_path);
                Ok(ListenerStatus {
                    running: false,
                    pid: None,
                    port: LISTENER_PORT,
                })
            }
        }
    } else {
        if let Some(pid) = read_listener_pid(&paths.listener_pid_path) {
            if is_pid_alive(pid) {
                return Ok(ListenerStatus {
                    running: true,
                    pid: Some(pid),
                    port: LISTENER_PORT,
                });
            }
            remove_listener_pid(&paths.listener_pid_path);
        }

        Ok(ListenerStatus {
            running: false,
            pid: None,
            port: LISTENER_PORT,
        })
    }
}

fn start_listener_internal(state: &AppState) -> Result<ListenerStatus, String> {
    let status = listener_status_internal(state)?;
    if status.running {
        return Ok(status);
    }

    let paths = hook_paths_buf();
    if !paths.listener_path.exists() {
        return Err("listener.mjs 不存在，请先执行一键安装 Hooks".to_string());
    }

    let node_bin = node_binary_or_error()?;
    let listener_log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.listener_log_path)
        .map_err(|error| format!("打开 listener.log 失败: {error}"))?;
    let listener_log_for_stderr = listener_log
        .try_clone()
        .map_err(|error| format!("复制 listener.log 句柄失败: {error}"))?;

    let mut child = Command::new(&node_bin)
        .arg(&paths.listener_path)
        .arg(format!("--port={LISTENER_PORT}"))
        .current_dir(&paths.script_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(listener_log))
        .stderr(Stdio::from(listener_log_for_stderr))
        .spawn()
        .map_err(|error| {
            format!(
                "启动监听器失败（node: {}）: {error}",
                path_to_string(&node_bin)
            )
        })?;

    thread::sleep(Duration::from_millis(250));
    if let Ok(Some(exit_status)) = child.try_wait() {
        return Err(format!(
            "监听器启动后立即退出（exit: {exit_status}）。请检查端口 {LISTENER_PORT} 是否被占用，或查看 {}",
            path_to_string(&paths.listener_log_path)
        ));
    }

    let pid = child.id();

    let mut listener = state
        .listener
        .lock()
        .map_err(|_| "监听器状态锁已损坏".to_string())?;
    *listener = Some(child);
    write_listener_pid(&paths.listener_pid_path, pid)?;

    Ok(ListenerStatus {
        running: true,
        pid: Some(pid),
        port: LISTENER_PORT,
    })
}

fn stop_listener_internal(state: &AppState) -> Result<ListenerStatus, String> {
    let paths = hook_paths_buf();
    let mut listener = state
        .listener
        .lock()
        .map_err(|_| "监听器状态锁已损坏".to_string())?;

    if let Some(child) = listener.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }

    if let Some(pid) = read_listener_pid(&paths.listener_pid_path) {
        terminate_pid(pid);
    }
    kill_rogue_listener_by_path(&paths);
    remove_listener_pid(&paths.listener_pid_path);

    *listener = None;

    Ok(ListenerStatus {
        running: false,
        pid: None,
        port: LISTENER_PORT,
    })
}

fn maybe_auto_start_listener(state: &AppState) {
    let paths = hook_paths_buf();
    if paths.listener_path.exists() && paths.forwarder_path.exists() {
        let _ = start_listener_internal(state);
    }
}

fn current_unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn compact_identifier(input: &str) -> String {
    let compact = input
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(8)
        .collect::<String>();
    if compact.is_empty() {
        "unknown".to_string()
    } else {
        compact
    }
}

fn normalize_ble_error(raw: String) -> String {
    let lower = raw.to_ascii_lowercase();

    if lower.contains("permission")
        || lower.contains("not authorized")
        || lower.contains("denied")
        || lower.contains("forbidden")
    {
        return "蓝牙权限被拒绝，请在系统设置中允许 Claude Notify Bot 使用蓝牙".to_string();
    }

    if lower.contains("powered off")
        || lower.contains("bluetooth is off")
        || lower.contains("bluetooth turned off")
        || lower.contains("disabled")
    {
        return "蓝牙未开启，请先开启系统蓝牙后重试".to_string();
    }

    if lower.contains("adapter") && (lower.contains("not found") || lower.contains("unavailable")) {
        return "未检测到可用蓝牙适配器，请检查蓝牙硬件或系统设置".to_string();
    }

    format!("蓝牙扫描失败：{raw}")
}

fn sorted_scanned_devices(scan_state: &BluetoothScanState) -> Vec<DeviceSummary> {
    let mut snapshot = scan_state
        .devices
        .values()
        .map(|record| record.summary.clone())
        .collect::<Vec<_>>();

    snapshot.sort_by(|left, right| {
        right
            .rssi
            .unwrap_or(-200)
            .cmp(&left.rssi.unwrap_or(-200))
            .then_with(|| left.name.cmp(&right.name))
    });

    snapshot
}

fn emit_scan_state(app: &tauri::AppHandle, state: &str, message: Option<String>) {
    let payload = ScanStatePayload {
        state: state.to_string(),
        message,
    };
    let _ = app.emit(BLUETOOTH_SCAN_STATE_EVENT, payload);
}

fn emit_scan_devices(app: &tauri::AppHandle, devices: &[DeviceSummary]) {
    let _ = app.emit(BLUETOOTH_DEVICES_UPDATED_EVENT, devices.to_vec());
}

fn apply_scan_tick(
    scan_state: &mut BluetoothScanState,
    discovered: Vec<DeviceSummary>,
) -> Vec<DeviceSummary> {
    let now = current_unix_millis();

    for device in discovered {
        scan_state.devices.insert(
            device.id.clone(),
            ScannedDeviceRecord {
                summary: device,
                last_seen_ms: now,
            },
        );
    }

    scan_state
        .devices
        .retain(|_, record| now.saturating_sub(record.last_seen_ms) <= BLUETOOTH_SCAN_STALE_MS);

    sorted_scanned_devices(scan_state)
}

fn finish_scan_as_idle(app: &tauri::AppHandle, session_id: u64) {
    let state = app.state::<AppState>();
    let should_emit = if let Ok(mut scan_state) = state.bluetooth_scan.lock() {
        if scan_state.session_id != session_id || !scan_state.running {
            false
        } else {
            scan_state.running = false;
            scan_state.stop_signal = None;
            scan_state.last_error = None;
            true
        }
    } else {
        false
    };

    if should_emit {
        emit_scan_state(app, "idle", None);
    }
}

fn finish_scan_as_error(app: &tauri::AppHandle, session_id: u64, message: String) {
    let state = app.state::<AppState>();
    let should_emit = if let Ok(mut scan_state) = state.bluetooth_scan.lock() {
        if scan_state.session_id != session_id || !scan_state.running {
            false
        } else {
            scan_state.running = false;
            scan_state.stop_signal = None;
            scan_state.last_error = Some(message.clone());
            true
        }
    } else {
        false
    };

    if should_emit {
        emit_scan_state(app, "error", Some(message));
    }
}

async fn run_ble_scan_loop(
    app: tauri::AppHandle,
    stop_signal: Arc<AtomicBool>,
    session_id: u64,
) -> Result<(), String> {
    let manager = BleManager::new()
        .await
        .map_err(|error| normalize_ble_error(error.to_string()))?;
    let adapters = manager
        .adapters()
        .await
        .map_err(|error| normalize_ble_error(error.to_string()))?;
    let adapter = adapters
        .into_iter()
        .next()
        .ok_or_else(|| "未检测到可用蓝牙适配器，请检查系统蓝牙状态".to_string())?;
    let adapter_state = adapter
        .adapter_state()
        .await
        .map_err(|error| normalize_ble_error(error.to_string()))?;
    if adapter_state != CentralState::PoweredOn {
        return Err("蓝牙未开启，请先在系统设置中打开蓝牙后重试".to_string());
    }

    adapter
        .start_scan(ScanFilter::default())
        .await
        .map_err(|error| normalize_ble_error(error.to_string()))?;

    loop {
        if stop_signal.load(Ordering::Relaxed) {
            break;
        }

        let peripherals = adapter
            .peripherals()
            .await
            .map_err(|error| normalize_ble_error(error.to_string()))?;
        let mut discovered = Vec::new();

        for peripheral in peripherals {
            let peripheral_id = format!("{:?}", peripheral.id());
            match peripheral.properties().await {
                Ok(Some(properties)) => {
                    let id = peripheral_id.clone();
                    let has_local_name = properties.local_name.is_some();
                    let name = properties
                        .local_name
                        .unwrap_or_else(|| format!("BLE-{}", compact_identifier(&id)));
                    let connectable = if !properties.services.is_empty() || has_local_name {
                        Some(true)
                    } else {
                        None
                    };
                    discovered.push(DeviceSummary {
                        id,
                        name,
                        rssi: properties.rssi.map(i32::from),
                        connectable,
                    });
                }
                Ok(None) => {
                    discovered.push(DeviceSummary {
                        id: peripheral_id.clone(),
                        name: format!("BLE-{}", compact_identifier(&peripheral_id)),
                        rssi: None,
                        connectable: None,
                    });
                }
                Err(_) => {
                    discovered.push(DeviceSummary {
                        id: peripheral_id.clone(),
                        name: format!("BLE-{}", compact_identifier(&peripheral_id)),
                        rssi: None,
                        connectable: None,
                    });
                }
            }
        }

        let snapshot = {
            let state = app.state::<AppState>();
            let mut scan_state = state
                .bluetooth_scan
                .lock()
                .map_err(|_| "蓝牙扫描状态锁已损坏".to_string())?;

            if scan_state.session_id != session_id || !scan_state.running {
                None
            } else {
                scan_state.last_error = None;
                Some(apply_scan_tick(&mut scan_state, discovered))
            }
        };

        if let Some(snapshot) = snapshot {
            emit_scan_devices(&app, &snapshot);
        }

        sleep(Duration::from_millis(BLUETOOTH_SCAN_INTERVAL_MS)).await;
    }

    let _ = adapter.stop_scan().await;
    Ok(())
}

#[tauri::command]
fn start_device_scan(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let (stop_signal, session_id) = {
        let mut scan_state = state
            .bluetooth_scan
            .lock()
            .map_err(|_| "蓝牙扫描状态锁已损坏".to_string())?;

        if scan_state.running {
            return Ok(());
        }

        scan_state.running = true;
        scan_state.last_error = None;
        scan_state.devices.clear();
        scan_state.session_id = scan_state.session_id.wrapping_add(1);
        if scan_state.session_id == 0 {
            scan_state.session_id = 1;
        }

        let stop_signal = Arc::new(AtomicBool::new(false));
        scan_state.stop_signal = Some(stop_signal.clone());
        (stop_signal, scan_state.session_id)
    };

    emit_scan_state(&app, "scanning", None);

    let app_for_task = app.clone();
    tauri::async_runtime::spawn(async move {
        match run_ble_scan_loop(app_for_task.clone(), stop_signal, session_id).await {
            Ok(()) => finish_scan_as_idle(&app_for_task, session_id),
            Err(error) => finish_scan_as_error(&app_for_task, session_id, error),
        }
    });

    Ok(())
}

#[tauri::command]
fn stop_device_scan(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let should_emit = {
        let mut scan_state = state
            .bluetooth_scan
            .lock()
            .map_err(|_| "蓝牙扫描状态锁已损坏".to_string())?;

        if let Some(stop_signal) = scan_state.stop_signal.take() {
            stop_signal.store(true, Ordering::Relaxed);
        }

        let was_running = scan_state.running;
        scan_state.running = false;
        scan_state.last_error = None;
        was_running
    };

    if should_emit {
        emit_scan_state(&app, "idle", None);
    }

    Ok(())
}

#[tauri::command]
fn get_scanned_devices(state: tauri::State<'_, AppState>) -> Result<Vec<DeviceSummary>, String> {
    let scan_state = state
        .bluetooth_scan
        .lock()
        .map_err(|_| "蓝牙扫描状态锁已损坏".to_string())?;

    Ok(sorted_scanned_devices(&scan_state))
}

#[tauri::command]
fn scan_devices(state: tauri::State<'_, AppState>) -> Result<Vec<DeviceSummary>, String> {
    get_scanned_devices(state)
}

async fn resolve_peripheral_by_id(
    device_id: &str,
) -> Result<btleplug::platform::Peripheral, String> {
    let manager = BleManager::new()
        .await
        .map_err(|error| normalize_ble_error(error.to_string()))?;
    let adapters = manager
        .adapters()
        .await
        .map_err(|error| normalize_ble_error(error.to_string()))?;

    for adapter in adapters {
        let mut peripherals = adapter
            .peripherals()
            .await
            .map_err(|error| normalize_ble_error(error.to_string()))?;

        if peripherals
            .iter()
            .all(|item| format!("{:?}", item.id()) != device_id)
        {
            let _ = adapter.start_scan(ScanFilter::default()).await;
            sleep(Duration::from_millis(1200)).await;
            peripherals = adapter
                .peripherals()
                .await
                .map_err(|error| normalize_ble_error(error.to_string()))?;
            let _ = adapter.stop_scan().await;
        }

        for peripheral in peripherals {
            if format!("{:?}", peripheral.id()) == device_id {
                return Ok(peripheral);
            }
        }
    }

    Err("未找到目标设备，请先扫描并确认设备在可连接范围内".to_string())
}

fn is_protocol_char_match(characteristic: &Characteristic) -> bool {
    let char_uuid = characteristic.uuid.to_string().to_ascii_lowercase();
    let service_uuid = characteristic.service_uuid.to_string().to_ascii_lowercase();

    let char_match = char_uuid == PROTOCOL_PREFERRED_CHAR_UUID || char_uuid.contains("fff1");
    let service_match =
        service_uuid == PROTOCOL_PREFERRED_SERVICE_UUID || service_uuid.contains("fff0");

    char_match && service_match
}

fn write_attempts(peripheral: &btleplug::platform::Peripheral) -> Vec<(Characteristic, WriteType)> {
    let characteristics = peripheral.characteristics().into_iter().collect::<Vec<_>>();
    let mut attempts = Vec::<(Characteristic, WriteType)>::new();

    let mut push_attempt = |characteristic: &Characteristic, write_type: WriteType| {
        let exists = attempts.iter().any(|(existing, existing_type)| {
            existing.uuid == characteristic.uuid && *existing_type == write_type
        });
        if !exists {
            attempts.push((characteristic.clone(), write_type));
        }
    };

    for characteristic in &characteristics {
        if is_protocol_char_match(characteristic)
            && characteristic
                .properties
                .contains(CharPropFlags::WRITE_WITHOUT_RESPONSE)
        {
            push_attempt(characteristic, WriteType::WithoutResponse);
        }
    }

    for characteristic in &characteristics {
        if is_protocol_char_match(characteristic)
            && characteristic.properties.contains(CharPropFlags::WRITE)
        {
            push_attempt(characteristic, WriteType::WithResponse);
        }
    }

    for characteristic in &characteristics {
        if characteristic
            .properties
            .contains(CharPropFlags::WRITE_WITHOUT_RESPONSE)
        {
            push_attempt(characteristic, WriteType::WithoutResponse);
        }
    }

    for characteristic in &characteristics {
        if characteristic.properties.contains(CharPropFlags::WRITE) {
            push_attempt(characteristic, WriteType::WithResponse);
        }
    }

    attempts
}

fn pick_response_characteristic(
    peripheral: &btleplug::platform::Peripheral,
) -> Option<Characteristic> {
    let characteristics = peripheral.characteristics().into_iter().collect::<Vec<_>>();

    for characteristic in &characteristics {
        if is_protocol_char_match(characteristic)
            && (characteristic.properties.contains(CharPropFlags::NOTIFY)
                || characteristic.properties.contains(CharPropFlags::INDICATE)
                || characteristic.properties.contains(CharPropFlags::READ))
        {
            return Some(characteristic.clone());
        }
    }

    for characteristic in &characteristics {
        if characteristic.properties.contains(CharPropFlags::NOTIFY)
            || characteristic.properties.contains(CharPropFlags::INDICATE)
        {
            return Some(characteristic.clone());
        }
    }

    for characteristic in &characteristics {
        if characteristic.properties.contains(CharPropFlags::READ) {
            return Some(characteristic.clone());
        }
    }

    None
}

fn normalize_protocol_text(raw: Vec<u8>) -> Option<String> {
    let text = String::from_utf8_lossy(&raw)
        .replace('\u{0}', "")
        .trim()
        .trim_matches('\r')
        .trim_matches('\n')
        .trim()
        .to_string();

    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn valve_state_label(state: u8) -> String {
    match state {
        0 => "IDLE".to_string(),
        1 => "PUSHING".to_string(),
        2 => "RELEASING".to_string(),
        3 => "WAITING".to_string(),
        _ => "UNKNOWN".to_string(),
    }
}

fn parse_valve_status(raw: &str) -> Result<ValveStatus, String> {
    let normalized = raw.trim();
    let payload = normalized
        .strip_prefix("STS:")
        .ok_or_else(|| format!("设备响应格式错误：{normalized}"))?;

    let mut parts = payload.split(',');
    let running_raw = parts
        .next()
        .ok_or_else(|| format!("设备响应字段缺失：{normalized}"))?;
    let count_raw = parts
        .next()
        .ok_or_else(|| format!("设备响应字段缺失：{normalized}"))?;
    let state_raw = parts
        .next()
        .ok_or_else(|| format!("设备响应字段缺失：{normalized}"))?;

    let running_num = running_raw
        .parse::<u8>()
        .map_err(|_| format!("running 字段非法：{running_raw}"))?;
    let count = count_raw
        .parse::<u32>()
        .map_err(|_| format!("count 字段非法：{count_raw}"))?;
    let state = state_raw
        .parse::<u8>()
        .map_err(|_| format!("state 字段非法：{state_raw}"))?;

    Ok(ValveStatus {
        running: running_num == 1,
        count,
        state,
        state_label: valve_state_label(state),
        raw: normalized.to_string(),
    })
}

async fn ensure_protocol_ready(device_id: &str) -> Result<btleplug::platform::Peripheral, String> {
    let peripheral = match resolve_peripheral_by_id(device_id).await {
        Ok(found) => found,
        Err(_) => {
            let manager = BleManager::new()
                .await
                .map_err(|error| normalize_ble_error(error.to_string()))?;
            let adapters = manager
                .adapters()
                .await
                .map_err(|error| normalize_ble_error(error.to_string()))?;

            let mut fallback: Option<btleplug::platform::Peripheral> = None;
            for adapter in adapters {
                let peripherals = adapter
                    .peripherals()
                    .await
                    .map_err(|error| normalize_ble_error(error.to_string()))?;
                for peripheral in peripherals {
                    if let Ok(true) = peripheral.is_connected().await {
                        fallback = Some(peripheral);
                        break;
                    }
                }
                if fallback.is_some() {
                    break;
                }
            }

            fallback.ok_or_else(|| "未找到目标设备，请先重新扫描并连接蓝牙设备".to_string())?
        }
    };

    let connected = peripheral
        .is_connected()
        .await
        .map_err(|error| format!("读取连接状态失败：{error}"))?;
    if !connected {
        peripheral
            .connect()
            .await
            .map_err(|error| format!("连接设备失败：{error}"))?;
    }

    peripheral
        .discover_services()
        .await
        .map_err(|error| format!("发现服务失败：{error}"))?;

    Ok(peripheral)
}

async fn ensure_connected_and_discovered(
    peripheral: btleplug::platform::Peripheral,
) -> Result<btleplug::platform::Peripheral, String> {
    let connected = peripheral
        .is_connected()
        .await
        .map_err(|error| format!("读取连接状态失败：{error}"))?;
    if !connected {
        peripheral
            .connect()
            .await
            .map_err(|error| format!("连接设备失败：{error}"))?;
    }

    peripheral
        .discover_services()
        .await
        .map_err(|error| format!("发现服务失败：{error}"))?;

    Ok(peripheral)
}

async fn send_protocol_command(
    state: &AppState,
    command: &str,
    expect_response: bool,
) -> Result<Option<String>, String> {
    let (device_id, cached_peripheral) = {
        let runtime = state
            .runtime
            .lock()
            .map_err(|_| "状态锁已损坏".to_string())?;
        (
            runtime
                .connected_device_id
                .clone()
                .ok_or_else(|| "当前没有已连接蓝牙设备".to_string())?,
            runtime.connected_peripheral.clone(),
        )
    };

    let peripheral = if let Some(peripheral) = cached_peripheral {
        ensure_connected_and_discovered(peripheral).await?
    } else {
        ensure_protocol_ready(&device_id).await?
    };

    {
        let mut runtime = state
            .runtime
            .lock()
            .map_err(|_| "状态锁已损坏".to_string())?;
        runtime.connected_peripheral = Some(peripheral.clone());
    }
    let attempts = write_attempts(&peripheral);
    if attempts.is_empty() {
        return Err("设备未提供可写入特征，无法发送协议命令".to_string());
    }

    let response_char = if expect_response {
        Some(
            pick_response_characteristic(&peripheral)
                .ok_or_else(|| "设备未提供可读取响应特征".to_string())?,
        )
    } else {
        None
    };

    let mut notifications = if let Some(ref chara) = response_char {
        if chara.properties.contains(CharPropFlags::NOTIFY)
            || chara.properties.contains(CharPropFlags::INDICATE)
        {
            let _ = peripheral.subscribe(chara).await;
            peripheral.notifications().await.ok()
        } else {
            None
        }
    } else {
        None
    };

    let mut payload_candidates = vec![command.trim().to_string()];
    payload_candidates.push(format!("{}\n", command.trim()));
    payload_candidates.push(format!("{}\r", command.trim()));
    payload_candidates.dedup();

    let mut write_errors = Vec::<String>::new();
    let mut sent_ok = false;
    for payload in &payload_candidates {
        for (write_char, write_type) in &attempts {
            match peripheral
                .write(write_char, payload.as_bytes(), *write_type)
                .await
            {
                Ok(()) => {
                    sent_ok = true;
                    break;
                }
                Err(error) => {
                    write_errors.push(format!(
                        "uuid={}, type={:?}, payload={:?}, error={error}",
                        write_char.uuid, write_type, payload
                    ));
                }
            }
        }
        if sent_ok {
            break;
        }
    }

    if !sent_ok {
        return Err(format!("发送命令失败：{}", write_errors.join(" | ")));
    }

    if !expect_response {
        return Ok(None);
    }

    let response_char = response_char.ok_or_else(|| "设备未提供可读取响应特征".to_string())?;
    let mut response_text: Option<String> = None;

    if let Some(ref mut stream) = notifications {
        let target_uuid = response_char.uuid;
        if let Ok(found) = timeout(Duration::from_millis(PROTOCOL_RESPONSE_TIMEOUT_MS), async {
            while let Some(event) = stream.next().await {
                if event.uuid != target_uuid {
                    continue;
                }

                if let Some(text) = normalize_protocol_text(event.value) {
                    return Some(text);
                }
            }
            None
        })
        .await
        {
            response_text = found;
        }
    }

    if response_text.is_none() && response_char.properties.contains(CharPropFlags::READ) {
        let bytes = peripheral
            .read(&response_char)
            .await
            .map_err(|error| format!("读取设备响应失败：{error}"))?;
        response_text = normalize_protocol_text(bytes);
    }

    response_text
        .map(Some)
        .ok_or_else(|| "设备未返回有效响应，请确认固件已实现 QUERY 返回".to_string())
}

#[tauri::command]
async fn connect_device(
    device_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let scanned_devices = {
        let scan_state = state
            .bluetooth_scan
            .lock()
            .map_err(|_| "蓝牙扫描状态锁已损坏".to_string())?;
        sorted_scanned_devices(&scan_state)
    };

    let matched = scanned_devices
        .into_iter()
        .find(|device| device.id == device_id)
        .ok_or_else(|| "设备不存在".to_string())?;

    if matched.connectable == Some(false) {
        return Err("设备暂不可连接".to_string());
    }

    {
        let mut runtime = state
            .runtime
            .lock()
            .map_err(|_| "状态锁已损坏".to_string())?;
        runtime.connection_state = "connecting".to_string();
        runtime.connected_peripheral = None;
    }

    let connect_result = async {
        let peripheral = ensure_protocol_ready(&device_id).await?;
        if write_attempts(&peripheral).is_empty() {
            return Err("设备未提供可写入特征，无法对接协议命令".to_string());
        }

        if let Some(response_char) = pick_response_characteristic(&peripheral) {
            if response_char.properties.contains(CharPropFlags::NOTIFY)
                || response_char.properties.contains(CharPropFlags::INDICATE)
            {
                let _ = peripheral.subscribe(&response_char).await;
            }
        }

        Ok::<btleplug::platform::Peripheral, String>(peripheral)
    }
    .await;

    let connected_peripheral = match connect_result {
        Ok(peripheral) => peripheral,
        Err(error) => {
            let mut runtime = state
                .runtime
                .lock()
                .map_err(|_| "状态锁已损坏".to_string())?;
            runtime.connection_state = "disconnected".to_string();
            runtime.connected_device_id = None;
            runtime.connected_peripheral = None;
            return Err(error);
        }
    };

    {
        let mut runtime = state
            .runtime
            .lock()
            .map_err(|_| "状态锁已损坏".to_string())?;
        runtime.connection_state = "connected".to_string();
        runtime.connected_device_id = Some(device_id.clone());
        runtime.connected_peripheral = Some(connected_peripheral);
    }

    let mut settings = read_settings();
    settings.last_device_id = Some(device_id);
    write_settings(&settings)?;

    Ok("connected".to_string())
}

#[tauri::command]
async fn disconnect_device(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let (connected_id, connected_peripheral) = {
        let runtime = state
            .runtime
            .lock()
            .map_err(|_| "状态锁已损坏".to_string())?;
        (
            runtime.connected_device_id.clone(),
            runtime.connected_peripheral.clone(),
        )
    };

    if let Some(peripheral) = connected_peripheral {
        if let Ok(true) = peripheral.is_connected().await {
            let _ = peripheral.disconnect().await;
        }
    } else if let Some(device_id) = connected_id {
        if let Ok(peripheral) = resolve_peripheral_by_id(&device_id).await {
            if let Ok(true) = peripheral.is_connected().await {
                let _ = peripheral.disconnect().await;
            }
        }
    }

    let mut runtime = state
        .runtime
        .lock()
        .map_err(|_| "状态锁已损坏".to_string())?;

    runtime.connection_state = "disconnected".to_string();
    runtime.connected_device_id = None;
    runtime.connected_peripheral = None;

    Ok("disconnected".to_string())
}

#[tauri::command]
fn get_connection_state(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let runtime = state
        .runtime
        .lock()
        .map_err(|_| "状态锁已损坏".to_string())?;

    Ok(runtime.connection_state.clone())
}

#[tauri::command]
fn save_bluetooth_settings(settings: BluetoothSettings) -> Result<(), String> {
    write_settings(&settings)
}

#[tauri::command]
fn load_bluetooth_settings() -> Result<BluetoothSettings, String> {
    Ok(read_settings())
}

#[tauri::command]
async fn valve_start(
    push: u32,
    release: u32,
    count: u32,
    interval: u32,
    duty: u8,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if !(10..=10_000).contains(&push) {
        return Err("push 超出范围（10~10000）".to_string());
    }
    if !(10..=10_000).contains(&release) {
        return Err("release 超出范围（10~10000）".to_string());
    }
    if !(1..=10_000).contains(&count) {
        return Err("count 超出范围（1~10000）".to_string());
    }
    if interval > 10_000 {
        return Err("interval 超出范围（0~10000）".to_string());
    }
    if !(1..=100).contains(&duty) {
        return Err("duty 超出范围（1~100）".to_string());
    }

    let command = format!("START {push} {release} {count} {interval} {duty}");
    let _ = send_protocol_command(state.inner(), &command, false).await?;
    Ok(())
}

#[tauri::command]
async fn valve_stop(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let _ = send_protocol_command(state.inner(), "STOP", false).await?;
    Ok(())
}

#[tauri::command]
async fn valve_set_led(
    mode: u8,
    r: u8,
    g: u8,
    b: u8,
    speed: u32,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    if mode > 2 {
        return Err("mode 超出范围（0~2）".to_string());
    }
    if mode == 2 && !(100..=60_000).contains(&speed) {
        return Err("呼吸灯 speed 超出范围（100~60000）".to_string());
    }

    let command = format!("LED {mode} {r} {g} {b} {speed}");
    let _ = send_protocol_command(state.inner(), &command, false).await?;
    Ok(())
}

#[tauri::command]
async fn valve_query_status(state: tauri::State<'_, AppState>) -> Result<ValveStatus, String> {
    let response = send_protocol_command(state.inner(), "QUERY", true)
        .await?
        .ok_or_else(|| "设备未返回 QUERY 响应".to_string())?;
    parse_valve_status(&response)
}

#[tauri::command]
fn get_hook_install_status(state: tauri::State<'_, AppState>) -> Result<HookInstallStatus, String> {
    let paths = hook_paths_buf();
    let settings_exists = paths.settings_path.exists();

    let hooks_exists = if settings_exists {
        match fs::read_to_string(&paths.settings_path) {
            Ok(content) => serde_json::from_str::<Value>(&content)
                .ok()
                .and_then(|value| value.get("hooks").cloned())
                .is_some(),
            Err(_) => false,
        }
    } else {
        false
    };

    let script_files_ready = paths.forwarder_path.exists() && paths.listener_path.exists();
    let listener_running = listener_status_internal(state.inner())?.running;

    Ok(HookInstallStatus {
        settings_exists,
        hooks_exists,
        script_files_ready,
        listener_running,
        paths: hook_paths(),
    })
}

#[tauri::command]
fn install_claude_hooks(
    overwrite_hooks: bool,
    state: tauri::State<'_, AppState>,
) -> Result<InstallClaudeHooksResult, String> {
    write_hook_scripts()?;

    let paths = hook_paths_buf();
    let settings_exists = paths.settings_path.exists();

    let mut settings_value = if settings_exists {
        let raw = fs::read_to_string(&paths.settings_path)
            .map_err(|error| format!("读取 settings.json 失败: {error}"))?;

        if raw.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str::<Value>(&raw)
                .map_err(|error| format!("settings.json 不是合法 JSON: {error}"))?
        }
    } else {
        json!({})
    };

    if !settings_value.is_object() {
        settings_value = json!({});
    }

    let hooks_exists = settings_value.get("hooks").is_some();
    if hooks_exists && !overwrite_hooks {
        return Err("检测到 settings.json 已存在 hooks，请确认覆盖后重试".to_string());
    }

    if let Some(parent) = paths.settings_path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建 .claude 目录失败: {error}"))?;
    }

    let backup_path = if settings_exists {
        let backup = paths
            .settings_path
            .with_file_name(format!("settings.json.bak.{}", current_unix_timestamp()));
        fs::copy(&paths.settings_path, &backup)
            .map_err(|error| format!("备份 settings.json 失败: {error}"))?;
        Some(path_to_string(&backup))
    } else {
        None
    };

    if let Some(object) = settings_value.as_object_mut() {
        object.insert("hooks".to_string(), hook_template_value(&paths));
    }

    let formatted = serde_json::to_string_pretty(&settings_value)
        .map_err(|error| format!("序列化 settings 失败: {error}"))?;

    fs::write(&paths.settings_path, format!("{}\n", formatted))
        .map_err(|error| format!("写入 settings.json 失败: {error}"))?;

    let mut warnings = external_script_warnings();
    let listener_started = match start_listener_internal(state.inner()) {
        Ok(status) => status.running,
        Err(error) => {
            warnings.push(error);
            false
        }
    };

    Ok(InstallClaudeHooksResult {
        ok: true,
        backup_path,
        settings_path: path_to_string(&paths.settings_path),
        script_dir: path_to_string(&paths.script_dir),
        overwritten_hooks: hooks_exists,
        listener_started,
        warnings,
    })
}

#[tauri::command]
fn start_listener(state: tauri::State<'_, AppState>) -> Result<ListenerStatus, String> {
    start_listener_internal(state.inner())
}

#[tauri::command]
fn stop_listener(state: tauri::State<'_, AppState>) -> Result<ListenerStatus, String> {
    stop_listener_internal(state.inner())
}

#[tauri::command]
fn get_listener_status(state: tauri::State<'_, AppState>) -> Result<ListenerStatus, String> {
    listener_status_internal(state.inner())
}

#[tauri::command]
fn get_recent_hook_tips(limit: Option<usize>) -> Result<Vec<HookTip>, String> {
    let take_count = limit.unwrap_or(5).clamp(1, 50);
    let log_path = hook_paths_buf().log_path;

    if !log_path.exists() {
        return Ok(vec![]);
    }

    let raw =
        fs::read_to_string(&log_path).map_err(|error| format!("读取 hook 日志失败: {error}"))?;
    let normalized = raw.replace("}\\n{", "}\n{").replace("}\\n", "}\n");

    let tips = normalized
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .filter_map(parse_hook_tip)
        .take(take_count)
        .collect::<Vec<_>>();

    Ok(tips)
}

#[tauri::command]
fn open_hook_log_folder() -> Result<String, String> {
    let script_dir = hook_paths_buf().script_dir;
    if !script_dir.exists() {
        return Err(format!(
            "日志目录不存在，请先安装 hooks: {}",
            path_to_string(&script_dir)
        ));
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(&script_dir)
            .spawn()
            .map_err(|error| format!("打开日志目录失败: {error}"))?;
    }

    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(&script_dir)
            .spawn()
            .map_err(|error| format!("打开日志目录失败: {error}"))?;
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(&script_dir)
            .spawn()
            .map_err(|error| format!("打开日志目录失败: {error}"))?;
    }

    Ok(path_to_string(&script_dir))
}

pub fn run() {
    tauri::Builder::default()
        .manage(AppState::default())
        .setup(|app| {
            let state = app.state::<AppState>();
            maybe_auto_start_listener(state.inner());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_device_scan,
            stop_device_scan,
            get_scanned_devices,
            scan_devices,
            connect_device,
            disconnect_device,
            get_connection_state,
            valve_start,
            valve_stop,
            valve_set_led,
            valve_query_status,
            save_bluetooth_settings,
            load_bluetooth_settings,
            get_hook_install_status,
            install_claude_hooks,
            start_listener,
            stop_listener,
            get_listener_status,
            get_recent_hook_tips,
            open_hook_log_folder
        ])
        .run(tauri::generate_context!())
        .expect("failed to run app");
}
