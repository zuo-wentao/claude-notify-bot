export type ConnectionState =
  | "disconnected"
  | "scanning"
  | "connecting"
  | "connected"
  | "error";

export interface DeviceSummary {
  id: string;
  name: string;
  rssi?: number;
  connectable: boolean | null;
}

export interface BluetoothSettings {
  autoReconnect: boolean;
  lastDeviceId?: string | null;
}

export type ScanState = "idle" | "scanning" | "error";

export interface ScanStatePayload {
  state: ScanState;
  message?: string;
}
