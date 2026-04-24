import { invoke } from "@tauri-apps/api/core";
import type { ValveLedParams, ValveStartParams, ValveStatus } from "@/types/valve";

const tauriAvailable = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

let mockRunning = false;
let mockCount = 0;
let mockState = 0;

function assertRange(value: number, min: number, max: number, name: string) {
  if (value < min || value > max) {
    throw new Error(`${name} 超出范围（${min}~${max}）`);
  }
}

function normalizeInvokeError(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  if (error && typeof error === "object") {
    const maybeMessage = (error as { message?: unknown }).message;
    if (typeof maybeMessage === "string" && maybeMessage.trim()) {
      return maybeMessage;
    }

    try {
      return JSON.stringify(error);
    } catch {
      return "调用蓝牙协议命令失败（无法解析错误详情）";
    }
  }

  return "调用蓝牙协议命令失败";
}

export async function valveStart(params: ValveStartParams): Promise<void> {
  if (tauriAvailable) {
    try {
      await invoke("valve_start", { ...params });
    } catch (error) {
      throw new Error(normalizeInvokeError(error));
    }
    return;
  }

  assertRange(params.push, 10, 10000, "push");
  assertRange(params.release, 10, 10000, "release");
  assertRange(params.count, 1, 10000, "count");
  assertRange(params.interval, 0, 10000, "interval");
  assertRange(params.duty, 1, 100, "duty");

  mockRunning = true;
  mockCount = 0;
  mockState = 1;
}

export async function valveStop(): Promise<void> {
  if (tauriAvailable) {
    try {
      await invoke("valve_stop");
    } catch (error) {
      throw new Error(normalizeInvokeError(error));
    }
    return;
  }

  mockRunning = false;
  mockState = 0;
}

export async function valveSetLed(params: ValveLedParams): Promise<void> {
  if (tauriAvailable) {
    try {
      await invoke("valve_set_led", { ...params });
    } catch (error) {
      throw new Error(normalizeInvokeError(error));
    }
    return;
  }

  assertRange(params.mode, 0, 2, "mode");
  if (params.mode === 2) {
    assertRange(params.speed, 100, 60000, "speed");
  }
}

export async function valveQueryStatus(): Promise<ValveStatus> {
  if (tauriAvailable) {
    try {
      return await invoke<ValveStatus>("valve_query_status");
    } catch (error) {
      throw new Error(normalizeInvokeError(error));
    }
  }

  if (mockRunning) {
    mockCount += 1;
  }

  const stateLabel = ["IDLE", "PUSHING", "RELEASING", "WAITING"][mockState] ?? "UNKNOWN";

  return {
    running: mockRunning,
    count: mockCount,
    state: mockState,
    stateLabel,
    raw: `STS:${mockRunning ? 1 : 0},${mockCount},${mockState}`,
  };
}
