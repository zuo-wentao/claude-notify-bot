import { invoke } from "@tauri-apps/api/core";
import type { BluetoothSettings, ConnectionState, DeviceSummary } from "@/types/bluetooth";

const tauriAvailable = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const mockDevices: DeviceSummary[] = [
  { id: "bot-jump-01", name: "Notify Motor A", rssi: -42, connectable: true },
  { id: "bot-jump-02", name: "Notify Motor B", rssi: -56, connectable: true },
  { id: "bot-jump-offline", name: "Notify Motor Offline", rssi: -88, connectable: false },
];

let mockState: ConnectionState = "disconnected";
let mockConnectedId: string | null = null;
let mockSettings: BluetoothSettings = {
  autoReconnect: true,
  lastDeviceId: null,
};

const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export async function scanDevices(): Promise<DeviceSummary[]> {
  if (tauriAvailable) {
    try {
      return await invoke<DeviceSummary[]>("scan_devices");
    } catch {
      // fallback to mock
    }
  }

  mockState = mockConnectedId ? "connected" : "scanning";
  await delay(500);
  mockState = mockConnectedId ? "connected" : "disconnected";
  return structuredClone(mockDevices);
}

export async function connectDevice(deviceId: string): Promise<ConnectionState> {
  if (tauriAvailable) {
    try {
      return await invoke<ConnectionState>("connect_device", { deviceId });
    } catch {
      // fallback to mock
    }
  }

  mockState = "connecting";
  await delay(700);

  const selected = mockDevices.find((device) => device.id === deviceId);
  if (!selected || !selected.connectable) {
    mockState = "error";
    await delay(250);
    mockState = "disconnected";
    throw new Error("设备不可连接或不存在");
  }

  mockConnectedId = selected.id;
  mockState = "connected";
  return mockState;
}

export async function disconnectDevice(): Promise<ConnectionState> {
  if (tauriAvailable) {
    try {
      return await invoke<ConnectionState>("disconnect_device");
    } catch {
      // fallback to mock
    }
  }

  await delay(200);
  mockConnectedId = null;
  mockState = "disconnected";
  return mockState;
}

export async function getConnectionState(): Promise<ConnectionState> {
  if (tauriAvailable) {
    try {
      return await invoke<ConnectionState>("get_connection_state");
    } catch {
      // fallback to mock
    }
  }

  return mockState;
}

export async function saveBluetoothSettings(settings: BluetoothSettings): Promise<void> {
  if (tauriAvailable) {
    try {
      await invoke<void>("save_bluetooth_settings", { settings });
      return;
    } catch {
      // fallback to mock
    }
  }

  mockSettings = { ...settings };
}

export async function loadBluetoothSettings(): Promise<BluetoothSettings> {
  if (tauriAvailable) {
    try {
      return await invoke<BluetoothSettings>("load_bluetooth_settings");
    } catch {
      // fallback to mock
    }
  }

  return { ...mockSettings };
}
