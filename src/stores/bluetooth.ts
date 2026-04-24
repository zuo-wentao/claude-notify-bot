import { computed, ref } from "vue";
import { defineStore } from "pinia";
import type {
  BluetoothSettings,
  ConnectionState,
  DeviceSummary,
  ScanState,
  ScanStatePayload,
} from "@/types/bluetooth";
import {
  connectDevice,
  disconnectDevice,
  getConnectionState,
  getScannedDevices,
  listenBluetoothScanEvents,
  loadBluetoothSettings,
  saveBluetoothSettings,
  scanDevices,
  startDeviceScan,
  stopDeviceScan,
} from "@/services/bluetooth-api";

function nowLabel() {
  return new Date().toLocaleTimeString("zh-CN", { hour12: false });
}

export const useBluetoothStore = defineStore("bluetooth", () => {
  const connectionState = ref<ConnectionState>("disconnected");
  const scanState = ref<ScanState>("idle");
  const devices = ref<DeviceSummary[]>([]);
  const connectedDevice = ref<DeviceSummary | null>(null);
  const autoReconnect = ref<boolean>(true);
  const lastDeviceId = ref<string | null>(null);
  const loadingScan = ref(false);
  const loadingConnect = ref(false);
  const errorMessage = ref<string | null>(null);
  const logs = ref<string[]>([]);

  let realtimeUnlisten: (() => Promise<void>) | null = null;

  const canDisconnect = computed(() => connectionState.value === "connected");

  function appendLog(message: string) {
    logs.value = [`[${nowLabel()}] ${message}`, ...logs.value].slice(0, 100);
  }

  async function persistSettings() {
    const settings: BluetoothSettings = {
      autoReconnect: autoReconnect.value,
      lastDeviceId: lastDeviceId.value,
    };
    await saveBluetoothSettings(settings);
  }

  function normalizeConnectionStateFromScan() {
    if (connectionState.value === "connected" || connectionState.value === "connecting") {
      return;
    }

    connectionState.value = scanState.value === "scanning" ? "scanning" : "disconnected";
  }

  function applyDeviceSnapshot(snapshot: DeviceSummary[]) {
    devices.value = snapshot;

    if (connectedDevice.value) {
      const latest = snapshot.find((item) => item.id === connectedDevice.value?.id);
      if (latest) {
        connectedDevice.value = latest;
      }
    }
  }

  function handleScanState(payload: ScanStatePayload) {
    scanState.value = payload.state;

    if (payload.state === "error") {
      const message = payload.message ?? "蓝牙扫描失败";
      errorMessage.value = message;
      connectionState.value = "error";
      appendLog(`扫描失败：${message}`);
      setTimeout(() => {
        if (connectionState.value === "error") {
          normalizeConnectionStateFromScan();
        }
      }, 600);
      return;
    }

    errorMessage.value = null;
    normalizeConnectionStateFromScan();
  }

  async function refreshDevices() {
    loadingScan.value = true;
    errorMessage.value = null;

    if (connectionState.value !== "connected" && connectionState.value !== "connecting") {
      connectionState.value = "scanning";
    }

    try {
      const result = await scanDevices();
      applyDeviceSnapshot(result);
      appendLog(`设备快照更新：${result.length} 个`);
      normalizeConnectionStateFromScan();
    } catch (error) {
      const message = error instanceof Error ? error.message : "扫描设备失败";
      errorMessage.value = message;
      connectionState.value = "error";
      appendLog(`扫描失败：${message}`);
    } finally {
      loadingScan.value = false;
    }
  }

  async function startRealtimeScan() {
    if (realtimeUnlisten) return;

    loadingScan.value = true;
    errorMessage.value = null;

    try {
      realtimeUnlisten = await listenBluetoothScanEvents({
        onDevicesUpdated: (snapshot) => {
          applyDeviceSnapshot(snapshot);
        },
        onScanState: (payload) => {
          handleScanState(payload);
        },
      });

      await startDeviceScan();
      scanState.value = "scanning";
      const snapshot = await getScannedDevices();
      applyDeviceSnapshot(snapshot);
      normalizeConnectionStateFromScan();
      appendLog("已开始持续扫描蓝牙设备");
    } catch (error) {
      const message = error instanceof Error ? error.message : "启动蓝牙扫描失败";
      errorMessage.value = message;
      scanState.value = "error";
      connectionState.value = "error";

      if (realtimeUnlisten) {
        await realtimeUnlisten().catch(() => undefined);
        realtimeUnlisten = null;
      }

      appendLog(`扫描启动失败：${message}`);
      throw error;
    } finally {
      loadingScan.value = false;
    }
  }

  async function stopRealtimeScan() {
    const hadActiveScan = !!realtimeUnlisten || scanState.value === "scanning";
    const unlisten = realtimeUnlisten;
    realtimeUnlisten = null;

    try {
      await stopDeviceScan();
    } catch (error) {
      const message = error instanceof Error ? error.message : "停止蓝牙扫描失败";
      errorMessage.value = message;
      appendLog(`停止扫描失败：${message}`);
    }

    if (unlisten) {
      await unlisten().catch(() => undefined);
    }

    scanState.value = "idle";
    normalizeConnectionStateFromScan();
    if (hadActiveScan) {
      appendLog("已停止蓝牙扫描");
    }
  }

  async function restartRealtimeScan() {
    await stopRealtimeScan();
    await startRealtimeScan();
  }

  async function connectToDevice(device: DeviceSummary) {
    if (device.connectable !== true) {
      const message = "该设备当前不可连接";
      errorMessage.value = message;
      appendLog(`连接失败：${message}`);
      return;
    }

    loadingConnect.value = true;
    errorMessage.value = null;
    connectionState.value = "connecting";

    try {
      const state = await connectDevice(device.id);
      connectionState.value = state;
      connectedDevice.value = device;
      lastDeviceId.value = device.id;
      await persistSettings();
      appendLog(`已连接：${device.name}`);
    } catch (error) {
      const message = error instanceof Error ? error.message : "连接设备失败";
      errorMessage.value = message;
      connectionState.value = "error";
      appendLog(`连接失败：${message}`);
    } finally {
      loadingConnect.value = false;
      if (connectionState.value === "error") {
        setTimeout(() => {
          if (connectionState.value === "error") normalizeConnectionStateFromScan();
        }, 600);
      }
    }
  }

  async function disconnectCurrentDevice() {
    errorMessage.value = null;
    try {
      await disconnectDevice();
      if (connectedDevice.value) {
        appendLog(`已断开：${connectedDevice.value.name}`);
      }
      connectedDevice.value = null;
      normalizeConnectionStateFromScan();
    } catch (error) {
      const message = error instanceof Error ? error.message : "断开失败";
      errorMessage.value = message;
      appendLog(`断开失败：${message}`);
    }
  }

  async function setAutoReconnect(value: boolean) {
    autoReconnect.value = value;
    await persistSettings();
    appendLog(value ? "已开启自动重连" : "已关闭自动重连");
  }

  async function initialize() {
    try {
      const settings = await loadBluetoothSettings();
      autoReconnect.value = settings.autoReconnect;
      lastDeviceId.value = settings.lastDeviceId ?? null;
      appendLog("已加载蓝牙设置");
    } catch {
      appendLog("加载设置失败，已使用默认值");
    }

    try {
      connectionState.value = await getConnectionState();
    } catch {
      connectionState.value = "disconnected";
    }

    try {
      const snapshot = await getScannedDevices();
      applyDeviceSnapshot(snapshot);
    } catch {
      // ignore initial scan cache read errors
    }

    if (autoReconnect.value && lastDeviceId.value) {
      const target = devices.value.find((item) => item.id === lastDeviceId.value && item.connectable === true);
      if (target) {
        appendLog(`尝试自动重连：${target.name}`);
        await connectToDevice(target);
      } else {
        appendLog("未找到上次设备，自动重连跳过");
      }
    }
  }

  return {
    connectionState,
    scanState,
    devices,
    connectedDevice,
    autoReconnect,
    lastDeviceId,
    loadingScan,
    loadingConnect,
    errorMessage,
    logs,
    canDisconnect,
    initialize,
    refreshDevices,
    startRealtimeScan,
    stopRealtimeScan,
    restartRealtimeScan,
    connectToDevice,
    disconnectCurrentDevice,
    setAutoReconnect,
  };
});
