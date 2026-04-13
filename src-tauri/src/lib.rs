use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

const LISTENER_PORT: u16 = 17373;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceSummary {
    id: String,
    name: String,
    rssi: Option<i32>,
    connectable: bool,
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
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            connection_state: "disconnected".to_string(),
            connected_device_id: None,
        }
    }
}

#[derive(Default)]
struct AppState {
    runtime: Mutex<RuntimeState>,
    listener: Mutex<Option<Child>>,
}

#[derive(Debug, Clone)]
struct HookPathsBuf {
    claude_dir: PathBuf,
    settings_path: PathBuf,
    script_dir: PathBuf,
    forwarder_path: PathBuf,
    listener_path: PathBuf,
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

fn mock_devices() -> Vec<DeviceSummary> {
    vec![
        DeviceSummary {
            id: "bot-jump-01".to_string(),
            name: "Notify Motor A".to_string(),
            rssi: Some(-42),
            connectable: true,
        },
        DeviceSummary {
            id: "bot-jump-02".to_string(),
            name: "Notify Motor B".to_string(),
            rssi: Some(-56),
            connectable: true,
        },
        DeviceSummary {
            id: "bot-jump-offline".to_string(),
            name: "Notify Motor Offline".to_string(),
            rssi: Some(-88),
            connectable: false,
        },
    ]
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

    let data = serde_json::to_string_pretty(settings).map_err(|error| format!("序列化失败: {error}"))?;
    fs::write(path, data).map_err(|error| format!("写入配置失败: {error}"))
}

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(std::env::temp_dir)
}

fn hook_paths_buf() -> HookPathsBuf {
    let claude_dir = home_dir().join(".claude");
    let script_dir = claude_dir.join("claude-notify-bot");

    HookPathsBuf {
        settings_path: claude_dir.join("settings.json"),
        forwarder_path: script_dir.join("hook-forwarder.mjs"),
        listener_path: script_dir.join("listener.mjs"),
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

fn write_hook_scripts() -> Result<(), String> {
    let paths = hook_paths_buf();

    fs::create_dir_all(&paths.script_dir).map_err(|error| format!("创建脚本目录失败: {error}"))?;
    fs::write(&paths.forwarder_path, forwarder_script_content())
        .map_err(|error| format!("写入 hook-forwarder.mjs 失败: {error}"))?;
    fs::write(&paths.listener_path, listener_script_content())
        .map_err(|error| format!("写入 listener.mjs 失败: {error}"))?;

    ensure_log_file(&paths.log_path)?;

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
        home.join(".claudeCodeChange").join("tools").join("claudeChangePreToolUse.js"),
        home.join(".claudeCodeChange").join("tools").join("claudeChangeStop.js"),
    )
}

fn hook_template_value(paths: &HookPathsBuf) -> Value {
    let (pre_tool_script, stop_script) = external_script_paths();

    let pre_tool_cmd = format!("node {}", quote_path(&pre_tool_script));
    let stop_cmd = format!("node {}", quote_path(&stop_script));

    let forwarder_with_log_cmd = format!(
        "node {} --port={} --log={}",
        quote_path(&paths.forwarder_path),
        LISTENER_PORT,
        quote_path(&paths.log_path)
    );

    let forwarder_cmd = format!(
        "node {} --port={}",
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
        warnings.push(format!(
            "未找到外部脚本: {}",
            path_to_string(&stop_script)
        ));
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
    let mut listener = state
        .listener
        .lock()
        .map_err(|_| "监听器状态锁已损坏".to_string())?;

    if let Some(child) = listener.as_mut() {
        match child.try_wait() {
            Ok(Some(_)) => {
                *listener = None;
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
                Ok(ListenerStatus {
                    running: false,
                    pid: None,
                    port: LISTENER_PORT,
                })
            }
        }
    } else {
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

    let child = Command::new("node")
        .arg(&paths.listener_path)
        .arg(format!("--port={LISTENER_PORT}"))
        .current_dir(&paths.script_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("启动监听器失败，请确认已安装 node: {error}"))?;

    let pid = child.id();

    let mut listener = state
        .listener
        .lock()
        .map_err(|_| "监听器状态锁已损坏".to_string())?;
    *listener = Some(child);

    Ok(ListenerStatus {
        running: true,
        pid: Some(pid),
        port: LISTENER_PORT,
    })
}

fn stop_listener_internal(state: &AppState) -> Result<ListenerStatus, String> {
    let mut listener = state
        .listener
        .lock()
        .map_err(|_| "监听器状态锁已损坏".to_string())?;

    if let Some(child) = listener.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }

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

#[tauri::command]
fn scan_devices(state: tauri::State<'_, AppState>) -> Result<Vec<DeviceSummary>, String> {
    let mut runtime = state
        .runtime
        .lock()
        .map_err(|_| "状态锁已损坏".to_string())?;

    runtime.connection_state = if runtime.connected_device_id.is_some() {
        "connected".to_string()
    } else {
        "disconnected".to_string()
    };

    Ok(mock_devices())
}

#[tauri::command]
fn connect_device(device_id: String, state: tauri::State<'_, AppState>) -> Result<String, String> {
    let matched = mock_devices()
        .into_iter()
        .find(|device| device.id == device_id)
        .ok_or_else(|| "设备不存在".to_string())?;

    if !matched.connectable {
        return Err("设备暂不可连接".to_string());
    }

    {
        let mut runtime = state
            .runtime
            .lock()
            .map_err(|_| "状态锁已损坏".to_string())?;
        runtime.connection_state = "connecting".to_string();
    }

    thread::sleep(Duration::from_millis(350));

    {
        let mut runtime = state
            .runtime
            .lock()
            .map_err(|_| "状态锁已损坏".to_string())?;
        runtime.connection_state = "connected".to_string();
        runtime.connected_device_id = Some(device_id.clone());
    }

    let mut settings = read_settings();
    settings.last_device_id = Some(device_id);
    write_settings(&settings)?;

    Ok("connected".to_string())
}

#[tauri::command]
fn disconnect_device(state: tauri::State<'_, AppState>) -> Result<String, String> {
    let mut runtime = state
        .runtime
        .lock()
        .map_err(|_| "状态锁已损坏".to_string())?;

    runtime.connection_state = "disconnected".to_string();
    runtime.connected_device_id = None;

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

    let formatted =
        serde_json::to_string_pretty(&settings_value).map_err(|error| format!("序列化 settings 失败: {error}"))?;

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

    let raw = fs::read_to_string(&log_path).map_err(|error| format!("读取 hook 日志失败: {error}"))?;
    let normalized = raw
        .replace("}\\n{", "}\n{")
        .replace("}\\n", "}\n");

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
            scan_devices,
            connect_device,
            disconnect_device,
            get_connection_state,
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
