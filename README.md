# Claude Notify Bot

Claude Notify Bot 是一个基于 `Tauri + Vue3 + shadcn 风格组件` 的桌面应用 MVP，当前版本聚焦蓝牙设备连接界面（Mock 驱动）。

## 已实现功能

- 设备扫描列表（Mock）
- 连接/断开设备
- 连接状态展示（未连接/扫描中/连接中/已连接/异常）
- 自动重连开关与上次设备记忆
- 操作日志面板
- 事件联动页面占位（后续接 Claude/Codex）

## 技术栈

- Frontend: Vue3 + TypeScript + Pinia + Tailwind + shadcn 风格组件
- Desktop: Tauri 2
- Backend(Mock): Rust commands

## 本地启动

```bash
pnpm install
pnpm tauri:dev
```

仅前端预览：

```bash
pnpm install
pnpm dev
```

## Rust 命令接口

- `scan_devices`
- `connect_device(deviceId)`
- `disconnect_device`
- `get_connection_state`
- `save_bluetooth_settings(settings)`
- `load_bluetooth_settings`

## 后续迭代建议

1. 替换 Rust `Mock Provider` 为真实蓝牙 Provider（如 `btleplug`）
2. 新增事件映射页面（START / APPROVAL / DONE -> 设备动作）
3. 增加异常重试、节流与离线告警
