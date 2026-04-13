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

const connectableDevices = computed(() => store.devices.filter((device) => device.connectable));
const filteredConnectableDevices = computed(() => {
  const keyword = deviceKeyword.value.trim().toLowerCase();
  if (!keyword) return connectableDevices.value;

  return connectableDevices.value.filter((device) => {
    const name = device.name.toLowerCase();
    const id = device.id.toLowerCase();
    return name.includes(keyword) || id.includes(keyword);
  });
});
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
  const inFiltered = filteredConnectableDevices.value.some((device) => device.id === current);
  if (inFiltered) return;
  selectedDeviceId.value = filteredConnectableDevices.value[0]?.id ?? null;
}

function formatTipTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleTimeString("zh-CN", { hour12: false });
}

async function refreshHookTips() {
  try {
    homeHookTips.value = await getRecentHookTips(5);
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
  homeDeviceHint.value = "正在扫描可连接设备...";

  try {
    await store.refreshDevices();
    normalizeSelectedDevice();
    homeDeviceHint.value = connectableDevices.value.length > 0 ? "请选择设备并连接" : "未发现可连接设备";
  } finally {
    pickerLoading.value = false;
  }
}

function openDevicePicker() {
  showDevicePicker.value = true;
  deviceKeyword.value = "";
  normalizeSelectedDevice();

  if (connectableDevices.value.length === 0) {
    void refreshDevicePicker();
  } else {
    homeDeviceHint.value = "请选择设备并连接";
  }
}

function closeDevicePicker() {
  showDevicePicker.value = false;
  deviceKeyword.value = "";
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
  const target = store.devices.find((device) => device.id === selectedDeviceId.value && device.connectable) ?? null;

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
                <SheetDescription>先搜索设备，选中后点击连接。</SheetDescription>
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
                  v-for="device in filteredConnectableDevices"
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
                  <span class="truncate">{{ device.name }}</span>
                  <span class="ml-2 shrink-0 text-xs opacity-80">RSSI {{ device.rssi ?? "-" }}</span>
                </button>
                <p v-if="filteredConnectableDevices.length === 0" class="py-8 text-center text-sm text-muted-foreground">
                  暂无可连接设备
                </p>
              </div>

              <SheetFooter class="mt-4">
                <Button size="sm" variant="ghost" @click="closeDevicePicker">取消</Button>
                <Button
                  size="sm"
                  :disabled="!selectedDeviceId || store.loadingConnect"
                  @click="connectSelectedDevice"
                >
                  {{ store.loadingConnect ? "连接中..." : "连接选中设备" }}
                </Button>
              </SheetFooter>
            </div>
          </SheetContent>
        </Sheet>
      </div>
    </SidebarInset>
  </SidebarProvider>
</template>
