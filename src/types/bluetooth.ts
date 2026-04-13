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
  connectable: boolean;
}

export interface BluetoothSettings {
  autoReconnect: boolean;
  lastDeviceId?: string | null;
}
