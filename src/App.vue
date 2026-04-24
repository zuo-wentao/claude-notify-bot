<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { House, Workflow, CircleHelp } from "lucide-vue-next";
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import UiSwitch from "@/components/ui/Switch.vue";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { Sheet, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarRail,
  SidebarSeparator,
  SidebarTrigger,
} from "@/components/ui/sidebar";
import { useBluetoothStore } from "@/stores/bluetooth";
import heroDeviceIcon from "@/assets/hero-device-placeholder.svg";
import {
  getHookInstallStatus,
  getRecentHookTips,
  getListenerStatus,
  installClaudeHooks,
  openHookLogFolder,
  startListener,
  stopListener,
} from "@/services/hook-installer-api";
import type { HookInstallStatus, HookTip } from "@/types/hooks";
import { valveQueryStatus, valveSetLed, valveStart, valveStop } from "@/services/valve-protocol-api";
import type { ValveStatus } from "@/types/valve";

type PageKey = "home" | "mapping" | "about";

const store = useBluetoothStore();
const currentPage = ref<PageKey>("home");

const navItems: { key: PageKey; label: string; icon: typeof House }[] = [
  { key: "home", label: "首页", icon: House },
  { key: "mapping", label: "事件联动映射", icon: Workflow },
  { key: "about", label: "关于", icon: CircleHelp },
];

const pageTitle = computed(() => navItems.find((item) => item.key === currentPage.value)?.label ?? "首页");

const mappingRows = [
  { event: "START", action: "设备闪蓝灯 + 震动 1 次", note: "任务启动反馈" },
  { event: "APPROVAL", action: "设备黄灯慢闪 + 声音提醒", note: "等待人工确认" },
  { event: "DONE", action: "设备绿灯常亮 2 秒", note: "任务完成提示" },
];

const hookStatus = ref<HookInstallStatus | null>(null);
const hookLoading = ref(false);
const listenerLoading = ref(false);
const awaitingOverwriteConfirm = ref(false);
const hookMessage = ref("");
const hookError = ref("");
const hookWarnings = ref<string[]>([]);
const homeDeviceHint = ref("");
const showDevicePicker = ref(false);
const deviceKeyword = ref("");
const selectedDeviceId = ref<string | null>(null);
const pickerLoading = ref(false);
const homeHookTips = ref<HookTip[]>([]);
const valveLoading = ref(false);
const valveMessage = ref("");
const valveError = ref("");
const valveStatus = ref<ValveStatus | null>(null);
const startPush = ref("100");
const startRelease = ref("100");
const startCount = ref("2");
const startInterval = ref("200");
const startDuty = ref("100");
const ledMode = ref("2");
const ledR = ref("255");
const ledG = ref("85");
const ledB = ref("0");
const ledSpeed = ref("2000");
const autoPulseEnabled = ref(true);
const autoPulseBusy = ref(false);
const autoPulseLastEvent = ref("");
const latestHookTipKey = ref<string | null>(null);
let hookTipTimer: ReturnType<typeof setInterval> | null = null;

const stateUi = computed(() => {
  switch (store.connectionState) {
    case "connected":
      return { label: "已连接", variant: "success" as const };
    case "connecting":
      return { label: "连接中", variant: "warning" as const };
    case "scanning":
      return { label: "扫描中", variant: "secondary" as const };
    case "error":
      return { label: "异常", variant: "destructive" as const };
    default:
      return { label: "未连接", variant: "outline" as const };
  }
});

const heroIconClass = computed(() => {
  switch (store.connectionState) {
    case "connected":
      return "opacity-100 grayscale-0 drop-shadow-[0_0_96px_rgba(249,115,22,0.9)]";
    case "connecting":
      return "opacity-95 grayscale-0 animate-pulse drop-shadow-[0_0_110px_rgba(251,146,60,0.95)]";
    case "scanning":
      return "opacity-95 grayscale-0 animate-pulse drop-shadow-[0_0_100px_rgba(251,146,60,0.78)]";
    case "error":
      return "opacity-75 grayscale drop-shadow-[0_0_64px_rgba(244,63,94,0.62)]";
    default:
      return "opacity-60 grayscale drop-shadow-[0_0_42px_rgba(212,212,216,0.4)]";
  }
});

const hookInstalled = computed(() => {
  if (!hookStatus.value) return false;
  return hookStatus.value.hooksExists && hookStatus.value.scriptFilesReady;
});

const scannedDevices = computed(() => store.devices);
const filteredDevices = computed(() => {
  const keyword = deviceKeyword.value.trim().toLowerCase();
  if (!keyword) return scannedDevices.value;

  return scannedDevices.value.filter((device) => {
    const name = device.name.toLowerCase();
    const id = device.id.toLowerCase();
    return name.includes(keyword) || id.includes(keyword);
  });
});
const selectedDevice = computed(() => {
  return scannedDevices.value.find((device) => device.id === selectedDeviceId.value) ?? null;
});
const selectedDeviceConnectable = computed(() => selectedDevice.value?.connectable === true);
const homeDeviceButtonText = computed(() => {
  if (store.loadingConnect) return "处理中...";
  if (store.connectionState === "connected") return "断开设备";
  return "连接设备";
});
const homeDeviceButtonVariant = computed(() => {
  if (store.connectionState === "connected") return "outline";
  return "default";
});
const homeDeviceButtonDisabled = computed(() => {
  return store.loadingConnect || store.connectionState === "connecting";
});

const listenerRunning = computed(() => hookStatus.value?.listenerRunning ?? false);
const settingsPathText = computed(() => hookStatus.value?.paths.settingsPath ?? "~/.claude/settings.json");
const scriptDirText = computed(() => hookStatus.value?.paths.scriptDir ?? "~/.claude/claude-notify-bot");

function resetHookFeedback() {
  hookError.value = "";
  hookMessage.value = "";
  hookWarnings.value = [];
}

function normalizeSelectedDevice() {
  const current = selectedDeviceId.value;
  const inFiltered = filteredDevices.value.some((device) => device.id === current);
  if (inFiltered) return;
  selectedDeviceId.value = filteredDevices.value[0]?.id ?? null;
}

function formatTipTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleTimeString("zh-CN", { hour12: false });
}

async function refreshHookTips() {
  try {
    const tips = await getRecentHookTips(5);
    homeHookTips.value = tips;

    const latestTip = tips[0];
    if (!latestTip) return;

    const latestKey = `${latestTip.time}|${latestTip.event}|${latestTip.detail}`;
    if (latestHookTipKey.value === null) {
      latestHookTipKey.value = latestKey;
      return;
    }

    if (latestHookTipKey.value === latestKey) {
      return;
    }

    latestHookTipKey.value = latestKey;
    void triggerAutoPulseFromHookTip(latestTip);
  } catch {
    // ignore tips pull errors to avoid disturbing normal operations
  }
}

async function handleOpenHookLogFolder() {
  try {
    const dir = await openHookLogFolder();
    hookMessage.value = `已打开日志目录：${dir}`;
  } catch (error) {
    hookError.value = error instanceof Error ? error.message : "打开日志目录失败";
  }
}

async function refreshDevicePicker() {
  pickerLoading.value = true;
  homeDeviceHint.value = "正在持续扫描蓝牙设备...";

  try {
    await store.restartRealtimeScan();
    normalizeSelectedDevice();
    homeDeviceHint.value = scannedDevices.value.length > 0 ? "请选择设备并连接" : "扫描中，暂未发现设备";
  } catch (error) {
    homeDeviceHint.value = error instanceof Error ? error.message : "蓝牙扫描启动失败";
  } finally {
    pickerLoading.value = false;
  }
}

function openDevicePicker() {
  showDevicePicker.value = true;
  deviceKeyword.value = "";
  homeDeviceHint.value = "正在持续扫描蓝牙设备...";
  normalizeSelectedDevice();
  void refreshDevicePicker();
}

function closeDevicePicker() {
  showDevicePicker.value = false;
  deviceKeyword.value = "";
  void store.stopRealtimeScan();
}

function handleDevicePickerOpenChange(open: boolean) {
  if (open) {
    showDevicePicker.value = true;
    return;
  }
  closeDevicePicker();
}

async function connectSelectedDevice() {
  normalizeSelectedDevice();
  const target = store.devices.find((device) => device.id === selectedDeviceId.value && device.connectable === true) ?? null;

  if (!target) {
    homeDeviceHint.value = "请先选择一个可连接设备";
    return;
  }

  homeDeviceHint.value = `正在连接 ${target.name}...`;
  await store.connectToDevice(target);

  if (store.connectedDevice) {
    homeDeviceHint.value = `已连接 ${target.name}`;
    closeDevicePicker();
  } else if (store.errorMessage) {
    homeDeviceHint.value = store.errorMessage;
  }
}

async function handleHomeDeviceAction() {
  if (homeDeviceButtonDisabled.value) return;

  if (store.connectionState === "connected") {
    homeDeviceHint.value = "正在断开设备...";
    await store.disconnectCurrentDevice();
    homeDeviceHint.value = "设备已断开";
    return;
  }

  openDevicePicker();
}

async function refreshHookStatus() {
  try {
    const status = await getHookInstallStatus();
    try {
      const listener = await getListenerStatus();
      status.listenerRunning = listener.running;
    } catch {
      // ignore listener status failure and keep status from get_hook_install_status
    }
    hookStatus.value = status;
  } catch (error) {
    const message = error instanceof Error ? error.message : "读取 Hook 状态失败";
    hookError.value = message;
  }
}

async function handleInstallHooks() {
  if (hookLoading.value) return;
  hookLoading.value = true;
  resetHookFeedback();

  try {
    if (!hookStatus.value) {
      await refreshHookStatus();
    }

    const hooksExists = hookStatus.value?.hooksExists ?? false;
    let overwriteHooks = false;

    if (hooksExists) {
      if (!awaitingOverwriteConfirm.value) {
        awaitingOverwriteConfirm.value = true;
        hookMessage.value =
          "检测到 settings.json 已存在 hooks。请点击“确认覆盖并安装”继续，或点击“取消覆盖”。";
        return;
      }
      overwriteHooks = true;
    }

    const result = await installClaudeHooks(overwriteHooks);
    hookWarnings.value = result.warnings ?? [];

    const backupText = result.backupPath ? `备份文件：${result.backupPath}` : "未生成备份（之前无 hooks）。";
    const overwriteText = result.overwrittenHooks ? "hooks 已覆盖。" : "hooks 已写入。";
    const listenerText = result.listenerStarted ? "监听器已启动。" : "监听器未启动。";
    hookMessage.value = `${overwriteText} ${backupText} ${listenerText}`;
    awaitingOverwriteConfirm.value = false;

    await refreshHookStatus();
  } catch (error) {
    hookError.value = error instanceof Error ? error.message : "安装 Claude Hooks 失败";
  } finally {
    hookLoading.value = false;
  }
}

function cancelOverwriteInstall() {
  awaitingOverwriteConfirm.value = false;
  hookMessage.value = "已取消覆盖，settings.json 未修改。";
}

function resetValveFeedback() {
  valveError.value = "";
  valveMessage.value = "";
}

function setAutoPulseEnabled(value: boolean) {
  autoPulseEnabled.value = value;
}

function shouldIgnoreAutoPulseEvent(eventName: string): boolean {
  const normalized = eventName.trim().toLowerCase();
  return normalized === "userpromptsubmit";
}

function parseIntField(value: string, label: string): number {
  const parsed = Number.parseInt(value, 10);
  if (Number.isNaN(parsed)) {
    throw new Error(`${label} 必须是整数`);
  }
  return parsed;
}

async function handleValveStart() {
  if (valveLoading.value) return;
  valveLoading.value = true;
  resetValveFeedback();

  try {
    await valveStart({
      push: parseIntField(startPush.value, "push"),
      release: parseIntField(startRelease.value, "release"),
      count: parseIntField(startCount.value, "count"),
      interval: parseIntField(startInterval.value, "interval"),
      duty: parseIntField(startDuty.value, "duty"),
    });
    valveMessage.value = "START 命令已发送";
  } catch (error) {
    valveError.value = error instanceof Error ? error.message : "发送 START 失败";
  } finally {
    valveLoading.value = false;
  }
}

async function handleValveStop() {
  if (valveLoading.value) return;
  valveLoading.value = true;
  resetValveFeedback();

  try {
    await valveStop();
    valveMessage.value = "STOP 命令已发送";
  } catch (error) {
    valveError.value = error instanceof Error ? error.message : "发送 STOP 失败";
  } finally {
    valveLoading.value = false;
  }
}

async function handleValveLed() {
  if (valveLoading.value) return;
  valveLoading.value = true;
  resetValveFeedback();

  try {
    await valveSetLed({
      mode: parseIntField(ledMode.value, "mode") as 0 | 1 | 2,
      r: parseIntField(ledR.value, "r"),
      g: parseIntField(ledG.value, "g"),
      b: parseIntField(ledB.value, "b"),
      speed: parseIntField(ledSpeed.value, "speed"),
    });
    valveMessage.value = "LED 命令已发送";
  } catch (error) {
    valveError.value = error instanceof Error ? error.message : "发送 LED 失败";
  } finally {
    valveLoading.value = false;
  }
}

async function handleValveQuery() {
  if (valveLoading.value) return;
  valveLoading.value = true;
  resetValveFeedback();

  try {
    valveStatus.value = await valveQueryStatus();
    valveMessage.value = "QUERY 成功";
  } catch (error) {
    valveError.value = error instanceof Error ? error.message : "发送 QUERY 失败";
  } finally {
    valveLoading.value = false;
  }
}

async function triggerAutoPulseFromHookTip(tip: HookTip) {
  if (!autoPulseEnabled.value || autoPulseBusy.value) return;
  if (store.connectionState !== "connected") return;
  if (valveLoading.value) return;
  if (shouldIgnoreAutoPulseEvent(tip.event)) return;

  autoPulseBusy.value = true;
  try {
    await valveStart({
      push: parseIntField(startPush.value, "push"),
      release: parseIntField(startRelease.value, "release"),
      count: parseIntField(startCount.value, "count"),
      interval: parseIntField(startInterval.value, "interval"),
      duty: parseIntField(startDuty.value, "duty"),
    });
    autoPulseLastEvent.value = `${tip.event} @ ${formatTipTime(tip.time)}`;
    valveMessage.value = `自动触发成功：${tip.event} -> START ${startPush.value} ${startRelease.value} ${startCount.value} ${startInterval.value} ${startDuty.value}`;
    valveError.value = "";
  } catch (error) {
    valveError.value = error instanceof Error ? error.message : "自动触发 START 失败";
  } finally {
    autoPulseBusy.value = false;
  }
}

async function handleRestartListener() {
  if (listenerLoading.value) return;
  listenerLoading.value = true;
  resetHookFeedback();

  try {
    await stopListener().catch(() => null);
    const status = await startListener();
    hookMessage.value = status.running ? "监听已重启并运行中。" : "监听重启失败。";
    await refreshHookStatus();
  } catch (error) {
    hookError.value = error instanceof Error ? error.message : "重启监听失败";
  } finally {
    listenerLoading.value = false;
  }
}

async function handleStopListener() {
  if (listenerLoading.value) return;
  listenerLoading.value = true;
  resetHookFeedback();

  try {
    const status = await stopListener();
    hookMessage.value = status.running ? "监听停止失败。请重试。" : "监听已停止。";
    await refreshHookStatus();
  } catch (error) {
    hookError.value = error instanceof Error ? error.message : "停止监听失败";
  } finally {
    listenerLoading.value = false;
  }
}

onMounted(async () => {
  await store.initialize();
  await refreshHookStatus();
  await refreshHookTips();
  hookTipTimer = setInterval(() => {
    void refreshHookTips();
  }, 2500);
});

onUnmounted(() => {
  void store.stopRealtimeScan();
  if (hookTipTimer) {
    clearInterval(hookTipTimer);
    hookTipTimer = null;
  }
});
</script>

<template>
  <SidebarProvider :default-open="false" class="bg-background">
    <Sidebar collapsible="icon">
      <SidebarHeader>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton size="lg">
              <span class="flex size-4 shrink-0 items-center justify-center">
                <img :src="heroDeviceIcon" alt="应用图标" class="block h-4 w-4 object-contain object-center" />
              </span>
              <div class="grid flex-1 text-left text-sm leading-tight group-data-[collapsible=icon]:hidden">
                <span class="truncate font-semibold">Claude Notify Bot</span>
                <span class="truncate text-xs">Desktop Console</span>
              </div>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>

      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>Navigation</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              <SidebarMenuItem v-for="item in navItems" :key="item.key">
                <SidebarMenuButton
                  :is-active="currentPage === item.key"
                  :tooltip="item.label"
                  @click="currentPage = item.key"
                >
                  <component :is="item.icon" class="block size-4 shrink-0" />
                  <span>{{ item.label }}</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>

      <SidebarFooter>
        <div class="px-2 group-data-[collapsible=icon]:hidden">
          <Badge :variant="stateUi.variant" class="w-full justify-center">{{ stateUi.label }}</Badge>
        </div>
        <div
          class="mx-2 mt-1 flex items-center justify-between rounded-md bg-sidebar-accent/60 px-2 py-2 group-data-[collapsible=icon]:hidden"
        >
          <span class="text-xs text-sidebar-foreground/80 group-data-[collapsible=icon]:hidden">自动重连</span>
          <UiSwitch :model-value="store.autoReconnect" @update:model-value="store.setAutoReconnect" />
        </div>
      </SidebarFooter>

      <SidebarRail />
    </Sidebar>

    <SidebarInset>
      <header class="flex h-14 items-center gap-2 border-b border-border bg-background/80 px-4 backdrop-blur">
        <SidebarTrigger />
        <Separator orientation="vertical" class="mr-2 h-4" />
        <h1 class="text-sm font-medium">{{ pageTitle }}</h1>
      </header>

      <div class="flex-1 overflow-auto p-6 md:p-8">
        <section
          v-if="currentPage === 'home'"
          class="relative flex min-h-[calc(100svh-9rem)] flex-col items-center justify-center gap-10"
        >
          <div class="relative flex items-center justify-center">
            <img
              :src="heroDeviceIcon"
              alt="设备图标占位图"
              class="h-32 w-32 object-contain transition-all duration-300 md:h-36 md:w-36"
              :class="heroIconClass"
            />
          </div>

          <div class="relative flex w-full max-w-md flex-col items-center gap-2">
            <Button :variant="homeDeviceButtonVariant" :disabled="homeDeviceButtonDisabled" @click="handleHomeDeviceAction">
              {{ homeDeviceButtonText }}
            </Button>
            <p v-if="homeDeviceHint" class="text-sm text-muted-foreground">{{ homeDeviceHint }}</p>
          </div>

          <div class="w-full max-w-2xl">
            <div class="flex items-center justify-between gap-3">
              <p class="text-[10px] uppercase tracking-[0.28em] text-muted-foreground/70">Hook Tips（最新 5 条）</p>
              <Button variant="ghost" size="sm" @click="handleOpenHookLogFolder">查看全部日志</Button>
            </div>
            <div v-if="homeHookTips.length === 0" class="mt-3 text-center text-sm text-muted-foreground">
              暂无 Hook 触发
            </div>
            <div v-else class="mt-3 space-y-2">
              <div
                v-for="(tip, index) in homeHookTips"
                :key="`${tip.time}-${tip.event}-${index}`"
                class="flex items-center justify-between border-b border-border/60 py-2"
              >
                <div class="flex items-center gap-2">
                  <Badge variant="outline">{{ tip.event }}</Badge>
                  <span class="text-sm text-foreground/90">{{ tip.detail }}</span>
                </div>
                <span class="text-xs text-muted-foreground">{{ formatTipTime(tip.time) }}</span>
              </div>
            </div>
          </div>
        </section>

        <section v-else-if="currentPage === 'mapping'" class="mx-auto w-full max-w-5xl space-y-8">
          <div class="space-y-2">
            <h2 class="text-3xl font-semibold tracking-tight">事件联动映射</h2>
            <p class="text-muted-foreground">在这里维护 Claude/Codex 事件到设备动作的映射与 Hook 安装状态。</p>
          </div>

          <div class="rounded-2xl border border-border/70 bg-card/30 p-6 md:p-7">
            <div class="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
              <div>
                <h3 class="text-lg font-semibold">Hook 安装面板</h3>
                <p class="mt-1 text-sm text-muted-foreground">
                  一键生成脚本并写入 `~/.claude/settings.json`，安装后自动尝试拉起 listener。
                </p>
              </div>
              <div class="flex flex-wrap items-center gap-2">
                <Badge :variant="hookInstalled ? 'success' : 'outline'">
                  {{ hookInstalled ? "Hooks 已安装" : "Hooks 未安装" }}
                </Badge>
                <Badge :variant="listenerRunning ? 'success' : 'secondary'">
                  {{ listenerRunning ? "监听运行中" : "监听已停止" }}
                </Badge>
              </div>
            </div>

            <div class="mt-5 grid gap-3 rounded-lg border border-border/70 bg-background/40 p-4 text-xs md:grid-cols-2">
              <div class="space-y-1">
                <p class="text-muted-foreground">settings.json</p>
                <p class="break-all font-mono">{{ settingsPathText }}</p>
              </div>
              <div class="space-y-1">
                <p class="text-muted-foreground">脚本目录</p>
                <p class="break-all font-mono">{{ scriptDirText }}</p>
              </div>
            </div>

            <div class="mt-5 flex flex-wrap gap-3">
              <Button :disabled="hookLoading || listenerLoading" @click="handleInstallHooks">
                {{ hookLoading ? "安装中..." : awaitingOverwriteConfirm ? "确认覆盖并安装" : "一键安装 Claude Hooks" }}
              </Button>
              <Button
                v-if="awaitingOverwriteConfirm"
                variant="outline"
                :disabled="hookLoading || listenerLoading"
                @click="cancelOverwriteInstall"
              >
                取消覆盖
              </Button>
              <Button variant="secondary" :disabled="hookLoading || listenerLoading" @click="handleRestartListener">
                {{ listenerLoading ? "处理中..." : "重启监听" }}
              </Button>
              <Button variant="outline" :disabled="hookLoading || listenerLoading" @click="handleStopListener">
                停止监听
              </Button>
              <Button variant="ghost" :disabled="hookLoading || listenerLoading" @click="refreshHookStatus">
                刷新状态
              </Button>
            </div>

            <p v-if="hookMessage" class="mt-4 text-sm text-foreground/90">{{ hookMessage }}</p>
            <p v-if="hookError" class="mt-2 text-sm text-destructive">{{ hookError }}</p>

            <div v-if="hookWarnings.length > 0" class="mt-3 rounded-lg border border-amber-500/40 bg-amber-500/10 p-3">
              <p class="text-sm font-medium text-amber-300">Warnings</p>
              <p v-for="warning in hookWarnings" :key="warning" class="mt-1 text-xs text-amber-200/90">
                - {{ warning }}
              </p>
            </div>
          </div>

          <div class="overflow-hidden rounded-2xl border border-border/70 bg-background/35">
            <table class="w-full text-sm">
              <thead class="border-b border-border/70 bg-muted/35 text-left text-muted-foreground">
                <tr>
                  <th class="px-4 py-3 font-medium">事件</th>
                  <th class="px-4 py-3 font-medium">设备动作</th>
                  <th class="px-4 py-3 font-medium">说明</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="row in mappingRows" :key="row.event" class="border-b border-border/60 last:border-none">
                  <td class="px-4 py-3">
                    <Badge variant="outline">{{ row.event }}</Badge>
                  </td>
                  <td class="px-4 py-3">{{ row.action }}</td>
                  <td class="px-4 py-3 text-muted-foreground">{{ row.note }}</td>
                </tr>
              </tbody>
            </table>
          </div>

          <Card class="p-6">
            <h3 class="text-lg font-semibold">阀门推杆协议调试</h3>
            <p class="mt-1 text-sm text-muted-foreground">
              已对接 `START/STOP/LED/QUERY` 文本协议，命令会通过当前蓝牙连接下发。
            </p>
            <div class="mt-3 flex items-center justify-between rounded-lg border border-border/70 bg-background/35 px-3 py-2">
              <div>
                <p class="text-sm font-medium">Claude 事件自动推两下</p>
                <p class="text-xs text-muted-foreground">
                  参数：START {{ startPush }} {{ startRelease }} {{ startCount }} {{ startInterval }} {{ startDuty }}
                </p>
                <p v-if="autoPulseLastEvent" class="text-xs text-muted-foreground">最近触发：{{ autoPulseLastEvent }}</p>
              </div>
              <UiSwitch :model-value="autoPulseEnabled" @update:model-value="setAutoPulseEnabled" />
            </div>

            <div class="mt-4 grid gap-3 md:grid-cols-5">
              <Input v-model="startPush" placeholder="push(ms)" />
              <Input v-model="startRelease" placeholder="release(ms)" />
              <Input v-model="startCount" placeholder="count" />
              <Input v-model="startInterval" placeholder="interval(ms)" />
              <Input v-model="startDuty" placeholder="duty(%)" />
            </div>
            <div class="mt-3 flex flex-wrap gap-2">
              <Button :disabled="valveLoading || store.connectionState !== 'connected'" @click="handleValveStart">
                {{ valveLoading ? "发送中..." : "发送 START" }}
              </Button>
              <Button
                variant="outline"
                :disabled="valveLoading || store.connectionState !== 'connected'"
                @click="handleValveStop"
              >
                发送 STOP
              </Button>
            </div>

            <div class="mt-5 grid gap-3 md:grid-cols-5">
              <Input v-model="ledMode" placeholder="mode(0~2)" />
              <Input v-model="ledR" placeholder="r(0~255)" />
              <Input v-model="ledG" placeholder="g(0~255)" />
              <Input v-model="ledB" placeholder="b(0~255)" />
              <Input v-model="ledSpeed" placeholder="speed(ms)" />
            </div>
            <div class="mt-3 flex flex-wrap gap-2">
              <Button
                variant="secondary"
                :disabled="valveLoading || store.connectionState !== 'connected'"
                @click="handleValveLed"
              >
                发送 LED
              </Button>
              <Button
                variant="ghost"
                :disabled="valveLoading || store.connectionState !== 'connected'"
                @click="handleValveQuery"
              >
                发送 QUERY
              </Button>
            </div>

            <p v-if="valveMessage" class="mt-4 text-sm text-foreground/90">{{ valveMessage }}</p>
            <p v-if="valveError" class="mt-2 text-sm text-destructive">{{ valveError }}</p>
            <div v-if="valveStatus" class="mt-4 rounded-lg border border-border/70 bg-background/35 p-3 text-sm">
              <p>running: {{ valveStatus.running ? 1 : 0 }}</p>
              <p>count: {{ valveStatus.count }}</p>
              <p>state: {{ valveStatus.state }} ({{ valveStatus.stateLabel }})</p>
              <p class="mt-1 text-xs text-muted-foreground">raw: {{ valveStatus.raw }}</p>
            </div>
          </Card>
        </section>

        <section v-else class="max-w-4xl">
          <h2 class="text-3xl font-semibold tracking-tight">关于</h2>
          <p class="mt-3 text-muted-foreground">
            Claude Notify Bot 是基于 Tauri + Vue3 的桌面应用，用于把 AI 工作流事件转化为设备通知动作。
          </p>

          <Card class="mt-8 p-6">
            <h3 class="text-lg font-semibold">版本与规划</h3>
            <div class="mt-3 space-y-2 text-sm text-muted-foreground">
              <p>当前版本：0.1.0（MVP）</p>
              <p>已支持：官方 shadcn Sidebar、首页 Hero、事件映射页面结构</p>
              <p>下一步：真实蓝牙 Provider、规则编辑器、模板导入导出</p>
            </div>
          </Card>
        </section>

        <Sheet :open="showDevicePicker" @update:open="handleDevicePickerOpenChange">
          <SheetContent side="bottom" class="mx-auto w-full max-w-2xl rounded-t-2xl border-x border-t p-0">
            <div class="p-5">
              <SheetHeader>
                <SheetTitle>选择蓝牙设备</SheetTitle>
                <SheetDescription>打开面板后会持续扫描，选中可连接设备后点击连接。</SheetDescription>
              </SheetHeader>

              <div class="mt-4 flex items-center gap-2">
                <Input
                  v-model="deviceKeyword"
                  placeholder="搜索设备名称或 ID"
                  @update:model-value="normalizeSelectedDevice"
                />
                <Button size="sm" variant="outline" :disabled="pickerLoading || store.loadingScan" @click="refreshDevicePicker">
                  {{ pickerLoading || store.loadingScan ? "刷新中..." : "刷新" }}
                </Button>
              </div>

              <div class="mt-4 max-h-64 space-y-2 overflow-auto pr-1">
                <button
                  v-for="device in filteredDevices"
                  :key="device.id"
                  type="button"
                  class="flex w-full items-center justify-between rounded-md border px-3 py-2 text-left text-sm transition-colors"
                  :class="
                    selectedDeviceId === device.id
                      ? 'border-primary bg-primary/15 text-foreground'
                      : 'border-border bg-background/40 text-muted-foreground hover:border-border/90 hover:bg-accent/40 hover:text-foreground'
                  "
                  @click="selectedDeviceId = device.id"
                >
                  <span class="min-w-0">
                    <span class="block truncate">{{ device.name }}</span>
                    <span class="block truncate text-[11px] opacity-70">{{ device.id }}</span>
                  </span>
                  <span class="ml-2 flex shrink-0 items-center gap-2 text-xs">
                    <Badge :variant="device.connectable === true ? 'success' : device.connectable === false ? 'outline' : 'secondary'">
                      {{ device.connectable === true ? "可连接" : device.connectable === false ? "不可连接" : "连接能力未知" }}
                    </Badge>
                    <span class="opacity-80">RSSI {{ device.rssi ?? "-" }}</span>
                  </span>
                </button>
                <p v-if="filteredDevices.length === 0" class="py-8 text-center text-sm text-muted-foreground">
                  {{ store.loadingScan ? "扫描中，暂未发现设备" : "暂无扫描设备" }}
                </p>
              </div>

              <SheetFooter class="mt-4">
                <Button size="sm" variant="ghost" @click="closeDevicePicker">取消</Button>
                <Button
                  size="sm"
                  :disabled="!selectedDeviceConnectable || store.loadingConnect"
                  @click="connectSelectedDevice"
                >
                  {{ store.loadingConnect ? "连接中..." : selectedDeviceConnectable ? "连接选中设备" : "设备不可连接" }}
                </Button>
              </SheetFooter>
            </div>
          </SheetContent>
        </Sheet>
      </div>
    </SidebarInset>
  </SidebarProvider>
</template>
