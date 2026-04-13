import { invoke } from "@tauri-apps/api/core";
import type { HookInstallStatus, InstallClaudeHooksResult, ListenerStatus } from "@/types/hooks";

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

  const hadHooks = mockStatus.hooksExists;
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
    backupPath: hadHooks ? "~/.claude/settings.json.bak.mock" : null,
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
