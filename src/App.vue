<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { House, Workflow, CircleHelp, ExternalLink } from "lucide-vue-next";
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import UiSwitch from "@/components/ui/Switch.vue";
import { Separator } from "@/components/ui/separator";
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

type PageKey = "home" | "mapping" | "about";

const store = useBluetoothStore();
const currentPage = ref<PageKey>("home");

const navItems: { key: PageKey; label: string; icon: typeof House }[] = [
  { key: "home", label: "首页", icon: House },
  { key: "mapping", label: "事件联动映射", icon: Workflow },
  { key: "about", label: "关于", icon: CircleHelp },
];

const pageTitle = computed(() => navItems.find((item) => item.key === currentPage.value)?.label ?? "首页");

const productLinks = [
  { name: "Claude", href: "https://claude.ai" },
  { name: "OpenAI Codex", href: "https://openai.com/codex" },
  { name: "Tauri", href: "https://tauri.app" },
];

const mappingRows = [
  { event: "START", action: "设备闪蓝灯 + 震动 1 次", note: "任务启动反馈" },
  { event: "APPROVAL", action: "设备黄灯慢闪 + 声音提醒", note: "等待人工确认" },
  { event: "DONE", action: "设备绿灯常亮 2 秒", note: "任务完成提示" },
];

onMounted(async () => {
  await store.initialize();
});

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
      return "opacity-100 grayscale-0 drop-shadow-[0_0_72px_rgba(249,115,22,0.82)]";
    case "connecting":
      return "opacity-95 grayscale-0 animate-pulse drop-shadow-[0_0_96px_rgba(251,146,60,0.9)]";
    case "scanning":
      return "opacity-95 grayscale-0 animate-pulse drop-shadow-[0_0_68px_rgba(251,146,60,0.72)]";
    case "error":
      return "opacity-75 grayscale drop-shadow-[0_0_56px_rgba(244,63,94,0.58)]";
    default:
      return "opacity-60 grayscale drop-shadow-[0_0_26px_rgba(212,212,216,0.28)]";
  }
});

const heroStatusHint = computed(() => {
  switch (store.connectionState) {
    case "connected":
      return "设备已就绪";
    case "connecting":
      return "正在建立连接";
    case "scanning":
      return "正在扫描设备";
    case "error":
      return "连接异常，请重试";
    default:
      return "尚未连接设备";
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
              <img
                :src="heroDeviceIcon"
                alt="应用图标"
                class="size-4 object-contain"
              />
              <div class="grid flex-1 text-left text-sm leading-tight group-data-[collapsible=icon]:hidden">
                <span class="truncate font-semibold">Claude Notify Bot</span>
                <span class="truncate text-xs">Desktop Console</span>
              </div>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>

      <SidebarSeparator />

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
                  <component :is="item.icon" />
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
        <div class="mx-2 mt-1 flex items-center justify-between rounded-md bg-sidebar-accent/60 px-2 py-2 group-data-[collapsible=icon]:hidden">
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
              class="h-64 w-64 object-contain transition-all duration-300 md:h-72 md:w-72"
              :class="heroIconClass"
            />
            <p class="absolute -bottom-9 text-sm font-medium text-muted-foreground">{{ heroStatusHint }}</p>
          </div>

          <Badge :variant="stateUi.variant">{{ stateUi.label }}</Badge>

          <div class="flex flex-wrap items-center justify-center gap-3">
            <a
              v-for="link in productLinks"
              :key="link.name"
              :href="link.href"
              target="_blank"
              rel="noreferrer"
              class="inline-flex items-center gap-2 rounded-md border border-border bg-card px-4 py-2 text-sm text-card-foreground transition-colors hover:bg-accent hover:text-accent-foreground"
            >
              {{ link.name }}
              <ExternalLink class="size-4" />
            </a>
          </div>
        </section>

        <section v-else-if="currentPage === 'mapping'" class="max-w-5xl">
          <h2 class="text-3xl font-semibold tracking-tight">事件联动映射</h2>
          <p class="mt-2 text-muted-foreground">在这里维护 Claude/Codex 事件到设备动作的映射。</p>

          <div class="mt-8 space-y-3">
            <Card v-for="row in mappingRows" :key="row.event" class="p-5">
              <div class="flex flex-col gap-2 md:flex-row md:items-center md:justify-between">
                <div class="flex items-center gap-3">
                  <Badge variant="outline">{{ row.event }}</Badge>
                  <span class="text-sm">{{ row.action }}</span>
                </div>
                <span class="text-xs text-muted-foreground">{{ row.note }}</span>
              </div>
            </Card>
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
      </div>
    </SidebarInset>
  </SidebarProvider>
</template>
