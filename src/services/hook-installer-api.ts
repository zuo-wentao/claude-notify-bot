import { invoke } from "@tauri-apps/api/core";
import type { HookInstallStatus, HookTip, InstallClaudeHooksResult, ListenerStatus } from "@/types/hooks";

const tauriAvailable = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const defaultPaths = {
  claudeDir: "~/.claude",
  settingsPath: "~/.claude/settings.json",
  scriptDir: "~/.claude/claude-notify-bot",
  forwarderPath: "~/.claude/claude-notify-bot/hook-forwarder.mjs",
  listenerPath: "~/.claude/claude-notify-bot/listener.mjs",
  logPath: "~/.claude/claude-notify-bot/forwarder.log",
};

let mockStatus: HookInstallStatus = {
  settingsExists: false,
  hooksExists: false,
  scriptFilesReady: false,
  listenerRunning: false,
  paths: { ...defaultPaths },
};

let mockTips: HookTip[] = [];

export async function getHookInstallStatus(): Promise<HookInstallStatus> {
  if (tauriAvailable) {
    return invoke<HookInstallStatus>("get_hook_install_status");
  }
  return structuredClone(mockStatus);
}

export async function installClaudeHooks(overwriteHooks: boolean): Promise<InstallClaudeHooksResult> {
  if (tauriAvailable) {
    return invoke<InstallClaudeHooksResult>("install_claude_hooks", { overwriteHooks });
  }

  const hadSettings = mockStatus.settingsExists;
  if (mockStatus.hooksExists && !overwriteHooks) {
    throw new Error("检测到 settings.json 已存在 hooks，请确认覆盖后重试");
  }

  mockStatus = {
    ...mockStatus,
    settingsExists: true,
    hooksExists: true,
    scriptFilesReady: true,
    listenerRunning: true,
  };

  return {
    ok: true,
    backupPath: hadSettings ? "~/.claude/settings.json.bak.mock" : null,
    settingsPath: mockStatus.paths.settingsPath,
    scriptDir: mockStatus.paths.scriptDir,
    overwrittenHooks: overwriteHooks,
    listenerStarted: true,
    warnings: ["当前是浏览器 Mock 模式，未真正写入本机 ~/.claude 文件。"],
  };
}

export async function startListener(): Promise<ListenerStatus> {
  if (tauriAvailable) {
    return invoke<ListenerStatus>("start_listener");
  }
  mockStatus.listenerRunning = true;
  return { running: true, pid: 9527, port: 17373 };
}

export async function stopListener(): Promise<ListenerStatus> {
  if (tauriAvailable) {
    return invoke<ListenerStatus>("stop_listener");
  }
  mockStatus.listenerRunning = false;
  return { running: false, pid: null, port: 17373 };
}

export async function getListenerStatus(): Promise<ListenerStatus> {
  if (tauriAvailable) {
    return invoke<ListenerStatus>("get_listener_status");
  }
  return { running: mockStatus.listenerRunning, pid: mockStatus.listenerRunning ? 9527 : null, port: 17373 };
}

export async function getRecentHookTips(limit = 5): Promise<HookTip[]> {
  if (tauriAvailable) {
    return invoke<HookTip[]>("get_recent_hook_tips", { limit });
  }

  if (mockStatus.listenerRunning && mockStatus.hooksExists && mockTips.length === 0) {
    mockTips = [
      {
        time: new Date().toISOString(),
        event: "UserPromptSubmit",
        detail: "用户提交了新请求",
      },
    ];
  }

  return mockTips.slice(Math.max(0, mockTips.length - limit));
}

export async function openHookLogFolder(): Promise<string> {
  if (tauriAvailable) {
    return invoke<string>("open_hook_log_folder");
  }
  return "~/.claude/claude-notify-bot";
}
