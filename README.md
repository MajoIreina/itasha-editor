# 痛车编辑器 NEXT · Itasha Editor

本仓库的开发主线已于 **2026-11-17** 切换为重构后的 NEXT（`0.3.0-dev.6`）：Rust + Tauri 2 + React / TypeScript + 原生 wgpu。后续功能、修复和构建均在 [`next/`](next/README.md) 进行。

当前实现包含欢迎与车型选配、SU7 Max 原生 3D、二维/三维素材编辑、图层变换、保存/重开及撤销/重做。生产漆面映射为 **v24 mirrored**，保留 v21 用于兼容回归。

这是开发基线：施工导出、旧版工程迁移、其他车型完整适配和发布验收尚未完成。2026-11-17 验证发现一项现有跨面传播回归失败，详见 [主线切换与验证记录](docs/NEXT主线切换-2026-11-17.md)。

## 开发与构建

在 Windows x64 安装 Rust/MSVC、Windows SDK、Node.js、pnpm 和 WebView2。环境与锁文件说明见 [开发与构建](docs/开发与构建.md)。

```powershell
cd next
pnpm install --frozen-lockfile
pnpm desktop:dev
```

```powershell
# 独立 Windows exe，不生成安装器
pnpm desktop:build
```

输出为 `next/target/release/itasha-desktop.exe`。根目录的 `启动工作室.cmd` 仅启动此 NEXT 发行构建。模型包需单独恢复，见 [离线资源](docs/离线资源.md)。

## 项目目录

源码已按职责实际重排，完整目录、调用流程、资源与运行数据位置见 [项目整体结构](docs/项目结构.md)。

| 路径 | 内容 |
| --- | --- |
| `next/src/app/` | 应用装配、导航与动作队列 |
| `next/src/features/` | 欢迎、工程、设置、工作区、2D 编辑和 3D 容器 |
| `next/src/bridge/`、`next/src/shared/` | Tauri 契约、共用颜色/主题/输入工具 |
| `next/src-tauri/src/` | `host` 启动、`commands` 命令、`adapters` 车型适配、`platform` 原生视口 |
| `next/crates/` | 契约、领域、应用服务、存储、曲面与渲染器 |
| `next/catalog/` | 编入 Rust 的车型选配数据 |
| `next/assets/paint/` | 当前运行映射与必要回归样本 |
| `next/tools/` | 按 checks、tests、smoke、catalog、assets、legacy 分类的工具 |
| `config/vehicles.json`、`vehicles/*/backend-adapter.json` | 车型目录生成所需的来源数据 |
| `docs/` | 当前开发约定、验收记录与有日期的重构设计资料 |

旧版源码、本机运行时、用户工程、临时漆面、模型 GLB/贴图和构建产物退出当前 Git 树，仍保留于本地或历史备份。恢复范围与历史保全方式见 [仓库管理](docs/仓库管理.md)。

## 验证

在 `next/` 执行：

```powershell
pnpm build
pnpm check:boundary
pnpm test:transforms
pnpm test:editor-properties
pnpm test:whole-layout
cargo fmt --all --check
cargo test --workspace --exclude itasha-desktop --locked
pnpm test:desktop
```

桌面 smoke 需要构建后的 NEXT exe 和独立恢复的 SU7 模型；测试依赖安装于 NEXT 自己的工作区。当前状态及失败回归见 [性能与验收](docs/性能与验收.md)。

本次目录重排后的构建、真实桌面 35 项和既有失败复现见 [结构整理与验证](docs/结构整理-2026-11-17.md)。

开发入口：[NEXT 说明](next/README.md) · [项目结构](docs/项目结构.md) · [模块架构](docs/模块架构.md) · [接口与车型适配](docs/接口与车型适配.md) · [开发规则](AGENTS.md)。车辆资产来源说明见 [NOTICE.txt](NOTICE.txt)。
