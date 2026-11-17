# 痛车编辑器 NEXT

2026-11-17 起，`next/` 为仓库唯一应用工作区，版本保持 `0.3.0-dev.6`。技术栈为 Rust + Tauri 2 + React / TypeScript + 原生 wgpu。目录已按功能和宿主职责实际重排，后续开发沿用此结构。

入口：[整体结构](../docs/项目结构.md) · [模块架构](../docs/模块架构.md) · [开发与构建](../docs/开发与构建.md) · [离线资源](../docs/离线资源.md) · [开发规则](../AGENTS.md)。

## 当前能力

已实现欢迎、新建和车型基础选配，SU7 Max 模块化原生 3D，2D/3D 素材编辑、图层变换、工程保存/重开、撤销/重做与关闭保护。工作区支持分屏、工程库和右侧面板布局；设置包括主题、快捷键、场景、性能偏好和目录。

当前接入车漆、轮毂、内饰、方向盘四轴。生产漆面为 v24 mirrored；左前下翼子板使用镜像近似。跨面投影不等于无畸变施工展开。其他车型完整适配、黑化/碳纤维独立轴、施工导出、旧工程迁移、自动保存与发布验收尚未完成。

完整 Rust 回归有 1 项已知跨面传播失败，见 [主线切换记录](../docs/NEXT主线切换-2026-11-17.md) 和 [本次结构验证](../docs/结构整理-2026-11-17.md)。

## 目录与职责

| 路径 | 内容 |
| --- | --- |
| `src/main.tsx` | React 挂载入口 |
| `src/app/` | 应用装配、导航、动作队列与关闭保护 |
| `src/features/` | onboarding、projects、settings、workspace、editor、viewport |
| `src/bridge/`、`src/shared/` | 基础 Tauri 契约与共用颜色、主题、输入工具 |
| `src-tauri/src/host.rs` | Tauri 服务与窗口装配 |
| `src-tauri/src/commands/` | editor、assets、viewport IPC |
| `src-tauri/src/adapters/`、`platform/` | SU7 资源适配与 Windows 原生视口 |
| `crates/` | contracts、domain、application、persistence、surface-map、renderer |
| `catalog/`、`assets/paint/` | 车型目录与当前/回归漆面数据 |
| `tools/` | checks、tests、smoke、catalog、assets、legacy 工具 |

详细树与“改哪个功能看哪个文件”见 [项目结构](../docs/项目结构.md)，工具参数与资源前提见 [工具说明](tools/README.md)。

## 开发与构建

在本目录执行，环境与版本以锁文件及 [构建说明](../docs/开发与构建.md) 为准：

```powershell
pnpm install --frozen-lockfile
pnpm desktop:dev
```

```powershell
pnpm desktop:build
```

发行输出为 `target/release/itasha-desktop.exe`，当前不生成安装器，Windows 需要 WebView2。单独 `pnpm dev` 只预览界面。SU7 GLB/贴图包独立恢复，不存入源码仓库。

## 状态与数据

React 使用 Rust 的只读 View，通过应用命令更新工程。Application 在 blocking worker 中以 Mutex 串行运行；模型准备与素材操作使用 blocking worker，原生 wgpu 线程处理渲染和视口消息。

正常工程位于 Tauri `app_data_dir`，标识 `studio.itasha.editor.next`，与旧版 `projects/` 分离。工程内部 JSON 含车型、四轴选配、图层和内容摘要；图片规范化为 PNG 后保存到 `blobs/`。损坏或不支持的工程逐文件隔离并在列表显示警告，当前没有修复 UI。

撤销历史最多 100 条、元数据上限 16 MiB，重开时建立新 session，历史不跨重开持久化。成功请求保留最近 256 个 ID 用于进程内重试去重。保存使用临时文件、同步和替换写入；自动保存尚未实现。部分缓存/性能偏好仅保存，不能据设置项存在推断全部生效。

## 验证与辅助工具

```powershell
pnpm build
pnpm check:boundary
pnpm test:transforms
pnpm test:editor-properties
pnpm test:whole-layout
cargo fmt --all --check
cargo test --workspace --exclude itasha-desktop --locked
cargo build -p itasha-desktop --features custom-protocol --locked
$env:ITASHA_SMOKE_EXE = 'target/debug/itasha-desktop.exe'
$env:ITASHA_MODEL_ROOT = 'L:/SU7/3D'
pnpm test:desktop
```

桌面 smoke 启动真实 Tauri，使用 NEXT 自身的 Playwright/pngjs 和独立 `.reports/` 数据，验证 IPC、原生视口、编辑与保存重开。报告和构建产物不纳入 Git；GPU readback 不代替操作系统合成及人工车漆核对。

`pnpm catalog:build` 从根车型来源生成静态目录，重新生成前核对已保存工程 ID/catalogVersion 兼容性。`pnpm inventory` 和 `tools/legacy/` 只用于本机历史资源。`tools/assets/` 的模型审计与漆面脚本需要独立恢复输入，不是日常构建前提。

有日期的设计资料位于 [重构设计](../docs/重构设计/README.md)，只反映对应阶段；当前事实以源码、本文和最新验证记录为准。