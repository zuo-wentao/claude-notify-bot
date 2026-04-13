export interface HookPaths {
  claudeDir: string;
  settingsPath: string;
  scriptDir: string;
  forwarderPath: string;
  listenerPath: string;
  logPath: string;
}

export interface HookInstallStatus {
  settingsExists: boolean;
  hooksExists: boolean;
  scriptFilesReady: boolean;
  listenerRunning: boolean;
  paths: HookPaths;
}

export interface InstallClaudeHooksResult {
  ok: boolean;
  backupPath?: string | null;
  settingsPath: string;
  scriptDir: string;
  overwrittenHooks: boolean;
  listenerStarted: boolean;
  warnings: string[];
}

export interface ListenerStatus {
  running: boolean;
  pid?: number | null;
  port: number;
}

export interface HookTip {
  time: string;
  event: string;
  detail: string;
}
