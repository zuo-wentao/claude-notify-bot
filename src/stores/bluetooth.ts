import { computed, ref } from "vue";
import { defineStore } from "pinia";
import type { BluetoothSettings, ConnectionState, DeviceSummary } from "@/types/bluetooth";
import {
  connectDevice,
  disconnectDevice,
  getConnectionState,
  loadBluetoothSettings,
  saveBluetoothSettings,
  scanDevices,
} from "@/services/bluetooth-api";

function nowLabel() {
  return new Date().toLocaleTimeString("zh-CN", { hour12: false });
}

export const useBluetoothStore = defineStore("bluetooth", () => {
  const connectionState = ref<ConnectionState>("disconnected");
  const devices = ref<DeviceSummary[]>([]);
  const connectedDevice = ref<DeviceSummary | null>(null);
  const autoReconnect = ref<boolean>(true);
  const lastDeviceId = ref<string | null>(null);
  const loadingScan = ref(false);
  const loadingConnect = ref(false);
  const errorMessage = ref<string | null>(null);
  const logs = ref<string[]>([]);

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

  async function refreshDevices() {
    loadingScan.value = true;
    errorMessage.value = null;
    connectionState.value = connectedDevice.value ? "connected" : "scanning";

    try {
      const result = await scanDevices();
      devices.value = result;
      appendLog(`扫描到 ${result.length} 个设备`);

      if (connectedDevice.value) {
        const latestConnected = result.find((item) => item.id === connectedDevice.value?.id);
        if (latestConnected) {
          connectedDevice.value = latestConnected;
        }
      }

      const state = await getConnectionState();
      connectionState.value = state;
      if (state !== "connected" && !connectedDevice.value) {
        connectionState.value = "disconnected";
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : "扫描设备失败";
      errorMessage.value = message;
      connectionState.value = "error";
      appendLog(`扫描失败：${message}`);
    } finally {
      loadingScan.value = false;
    }
  }

  async function connectToDevice(device: DeviceSummary) {
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
          if (connectionState.value === "error") connectionState.value = "disconnected";
        }, 600);
      }
    }
  }

  async function disconnectCurrentDevice() {
    errorMessage.value = null;
    try {
      connectionState.value = await disconnectDevice();
      if (connectedDevice.value) {
        appendLog(`已断开：${connectedDevice.value.name}`);
      }
      connectedDevice.value = null;
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

    await refreshDevices();

    if (autoReconnect.value && lastDeviceId.value) {
      const target = devices.value.find((item) => item.id === lastDeviceId.value && item.connectable);
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
    connectToDevice,
    disconnectCurrentDevice,
    setAutoReconnect,
  };
});
