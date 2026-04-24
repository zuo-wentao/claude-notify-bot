import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  BluetoothSettings,
  ConnectionState,
  DeviceSummary,
  ScanStatePayload,
} from "@/types/bluetooth";

const tauriAvailable = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const mockDevices: DeviceSummary[] = [
  { id: "bot-jump-01", name: "Notify Motor A", rssi: -42, connectable: true },
  { id: "bot-jump-02", name: "Notify Motor B", rssi: -56, connectable: true },
  { id: "bot-jump-offline", name: "Notify Motor Offline", rssi: -88, connectable: false },
  { id: "bot-jump-unknown", name: "Notify Motor Unknown", rssi: -70, connectable: null },
];

let mockState: ConnectionState = "disconnected";
let mockConnectedId: string | null = null;
let mockSettings: BluetoothSettings = {
  autoReconnect: true,
  lastDeviceId: null,
};

let mockScanTimer: ReturnType<typeof setInterval> | null = null;
const mockDeviceListeners = new Set<(devices: DeviceSummary[]) => void>();
const mockScanStateListeners = new Set<(payload: ScanStatePayload) => void>();

const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

function emitMockDevices() {
  const snapshot = structuredClone(mockDevices);
  for (const listener of mockDeviceListeners) {
    listener(snapshot);
  }
}

function emitMockScanState(payload: ScanStatePayload) {
  for (const listener of mockScanStateListeners) {
    listener(payload);
  }
}

function jitterRssi(value?: number): number | undefined {
  if (typeof value !== "number") return value;
  const delta = Math.floor(Math.random() * 5) - 2;
  return Math.max(-95, Math.min(-30, value + delta));
}

function ensureMockScanning() {
  if (mockScanTimer) return;

  if (!mockConnectedId) {
    mockState = "scanning";
  }
  emitMockScanState({ state: "scanning" });
  emitMockDevices();

  mockScanTimer = setInterval(() => {
    for (const device of mockDevices) {
      device.rssi = jitterRssi(device.rssi);
    }
    emitMockDevices();
  }, 700);
}

function stopMockScanning() {
  if (mockScanTimer) {
    clearInterval(mockScanTimer);
    mockScanTimer = null;
  }

  if (!mockConnectedId) {
    mockState = "disconnected";
  }
  emitMockScanState({ state: "idle" });
}

export interface BluetoothScanEventHandlers {
  onDevicesUpdated?: (devices: DeviceSummary[]) => void;
  onScanState?: (payload: ScanStatePayload) => void;
}

export async function listenBluetoothScanEvents(
  handlers: BluetoothScanEventHandlers,
): Promise<() => Promise<void>> {
  if (tauriAvailable) {
    const unlistenDevices = await listen<DeviceSummary[]>("bluetooth://devices-updated", (event) => {
      handlers.onDevicesUpdated?.(event.payload);
    });

    const unlistenScanState = await listen<ScanStatePayload>("bluetooth://scan-state", (event) => {
      handlers.onScanState?.(event.payload);
    });

    return async () => {
      const tasks: Promise<void>[] = [Promise.resolve(unlistenDevices())];
      tasks.push(Promise.resolve(unlistenScanState()));
      await Promise.all(tasks);
    };
  }

  if (handlers.onDevicesUpdated) {
    mockDeviceListeners.add(handlers.onDevicesUpdated);
  }
  if (handlers.onScanState) {
    mockScanStateListeners.add(handlers.onScanState);
  }

  return async () => {
    if (handlers.onDevicesUpdated) {
      mockDeviceListeners.delete(handlers.onDevicesUpdated);
    }
    if (handlers.onScanState) {
      mockScanStateListeners.delete(handlers.onScanState);
    }
  };
}

export async function startDeviceScan(): Promise<void> {
  if (tauriAvailable) {
    await invoke<void>("start_device_scan");
    return;
  }

  ensureMockScanning();
}

export async function stopDeviceScan(): Promise<void> {
  if (tauriAvailable) {
    await invoke<void>("stop_device_scan");
    return;
  }

  stopMockScanning();
}

export async function getScannedDevices(): Promise<DeviceSummary[]> {
  if (tauriAvailable) {
    return invoke<DeviceSummary[]>("get_scanned_devices");
  }

  return structuredClone(mockDevices);
}

export async function scanDevices(): Promise<DeviceSummary[]> {
  if (tauriAvailable) {
    return invoke<DeviceSummary[]>("scan_devices");
  }

  mockState = mockConnectedId ? "connected" : "scanning";
  await delay(500);
  if (!mockConnectedId) {
    mockState = "disconnected";
  }
  return structuredClone(mockDevices);
}

export async function connectDevice(deviceId: string): Promise<ConnectionState> {
  if (tauriAvailable) {
    return invoke<ConnectionState>("connect_device", { deviceId });
  }

  mockState = "connecting";
  await delay(700);

  const selected = mockDevices.find((device) => device.id === deviceId);
  if (!selected || selected.connectable !== true) {
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
    return invoke<ConnectionState>("disconnect_device");
  }

  await delay(200);
  mockConnectedId = null;
  mockState = "disconnected";
  return mockState;
}

export async function getConnectionState(): Promise<ConnectionState> {
  if (tauriAvailable) {
    return invoke<ConnectionState>("get_connection_state");
  }

  return mockState;
}

export async function saveBluetoothSettings(settings: BluetoothSettings): Promise<void> {
  if (tauriAvailable) {
    await invoke<void>("save_bluetooth_settings", { settings });
    return;
  }

  mockSettings = { ...settings };
}

export async function loadBluetoothSettings(): Promise<BluetoothSettings> {
  if (tauriAvailable) {
    return invoke<BluetoothSettings>("load_bluetooth_settings");
  }

  return { ...mockSettings };
}
