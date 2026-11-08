# 痛车编辑器 · Itasha Editor 0.2

Windows 本地涂装编辑器，使用 Rust 后端与 Electron 前端。支持二维图层编辑、真实车身曲面贴合、跨部件素材移动、独立 FOV 镜头和 PNG / SVG / PDF 输出。目前完整编辑 Su7，其他车型与用户模型导入仍在适配。

本仓库管理源码、配置、测试、示例与文档。Electron、ImageMagick、离线车型存档及用户数据保留在本地，不进入 Git。克隆源码后需要恢复离线资源并构建，不能直接作为便携版运行。

## 项目结构

| 目录 | 内容 |
| --- | --- |
| `app/` | 编辑界面、相机、曲面算法、3D 引擎适配与输出 |
| `backend/src/` | Rust HTTP 服务、图片解码调度、工程存储、导出和离线重放 |
| `desktop/src/` | Electron 窗口、后端进程与生命周期 |
| `config/` | 车型注册表、图片处理策略与色彩配置 |
| `vehicles/<id>/` | 按车型维护的后端适配数据；模型与捕获清单另行恢复 |
| `resources/` | 离线引擎的源码定制；大型存档留在本地 |
| `examples/` | 可分发的示例工程 |
| `tools/` | 构建、资源检查和真实应用验收脚本 |
| `tools/fixtures/` | 自动化测试素材 |
| `docs/` | 架构、构建、验收、资源恢复与仓库约定 |

## 开发启动

Windows x64 需要 Rust/Cargo 1.97.1、MSVC C++ Build Tools、Windows SDK、Node.js 24.22.1 和 pnpm 11.25.0。JS 与 Rust 依赖由锁文件固定。

```powershell
# 从已有完整本地目录恢复离线车型资源，不读取用户工程。
node tools/local-assets.cjs --from 'D:\已有完整的痛车编辑器'
node tools/local-assets.cjs --check

# 指向已安装 pnpm 的 JS 入口，安装并构建宿主与后端。
$env:ITASHA_PNPM_CLI = 'C:\实际路径\pnpm\bin\pnpm.cjs'
node tools/setup-desktop.cjs
node tools/build-desktop.cjs
.\痛车编辑器.exe
```

已有完整便携目录可以直接双击根目录 `痛车编辑器.exe`。运行不需要系统 Python、Node.js 或 WebView2。工程保存在 `projects/`，导出在 `exports/`，浏览器设置在 `app-data-electron/`。原 `.su7design` v1 工程继续兼容。

## 检查

```powershell
cargo test --locked --manifest-path backend/Cargo.toml
node --test desktop/test/*.test.cjs tools/test/*.test.cjs
node tools/surface-curvature-acceptance.cjs
node tools/camera-navigation-acceptance.cjs
node tools/acceptance-full.cjs
node tools/paint-projection-acceptance.cjs
node tools/benchmark-performance.cjs --enforce
```

前三项不需要离线车型资源；真实 Electron 验收需要已构建应用及模型。图形和性能验收应逐项运行，测试工程和配置使用临时目录。当前实测与限制见 `docs/性能与验收.md`。

## 文档

- [使用说明](使用说明.md)
- [模块架构](docs/模块架构.md)
- [开发与构建](docs/开发与构建.md)
- [离线资源恢复](docs/离线资源.md)
- [仓库与目录约定](docs/仓库管理.md)
- [接口与车型适配](docs/接口与车型适配.md)
- [性能与验收](docs/性能与验收.md)

车辆模型、原厂材质和配置器存档来源见 `NOTICE.txt`。仓库没有为这些第三方资源授予新的许可。
