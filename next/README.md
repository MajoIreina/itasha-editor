# 痛车编辑器 Next

2026-11-17 起，本工作区为仓库唯一开发主线，版本保持 0.3.0-dev.6；架构为 Rust + Tauri 2 + React / TypeScript + wgpu。根 README、AGENTS 和 docs/开发与构建.md 为当前入口。
已实现欢迎页 → 车型选择 → 车漆/轮毂/内饰/方向盘选配 → 工作区流程，以及本机设计库加载、重命名、保存、重开、撤销/重做和关闭保护。早期无车型草稿继续兼容。
工作区支持上下/左右分屏、拖动比例；右上角设置管理布局、性能偏好、快捷键与目录路径。见 [设置增量说明](../docs/重构设计/工作区与设置增量-2026-11-09.md)。
右侧新增素材库、图层管理、插入图形和插入文字面板，支持高度调节与最小化并保存布局；已接入首批图形、文字、图片及图层编辑。见 [面板说明](../docs/重构设计/右侧工具面板-2026-11-10.md)。
新建流程、目录来源及保存契约见 [欢迎与选配说明](../docs/重构设计/欢迎与车型选配流程-2026-11-09.md)。页面使用软件自己的 UI 与车辆形态示意。
已接入 SU7 Max 的 Rust/wgpu 原生静态 PBR 预览，按选配加载车身、轮组、方向盘和内饰。漆面映射当前为 v24 mirrored（左前下翼子板镜像版）：18 个可编辑部件（含尾厢，左前下翼子板采用 v24 镜像近似），21 个原始候选与历史筛选规则见各映射包 README。2D/3D 均支持素材选择、移动、八方向缩放、旋转，以及单图层跨面投影。导出和旧工程迁移尚未接入；用户已指定以 NEXT 作为开发基线，此切换不代表完整发布验收。当前小素材跨面传播有 1 项回归失败，见 [主线切换记录](../docs/NEXT主线切换-2026-11-17.md)。资产路径、测量与原生宿主限制见 [3D 首批接入记录](../docs/重构设计/SU7原生渲染首批接入-2026-11-10.md)，本次缺面修复见 [漆面区域缺失修复](../docs/重构设计/NEXT漆面区域缺失修复-2026-11-12.md)。

画布映射、资源预算、验证与当前曲面限制见 [画布首批接入记录](../docs/重构设计/画布与曲面贴花首批接入-2026-11-10.md)。

本轮跨面映射、控制点、兼容规则及验证证据见 [跨面素材与双端编辑](../docs/重构设计/跨面素材与双端编辑-2026-11-10.md)。环绕/顶部投影不是无畸变施工展开。

查看器新增 FXAA 抗锯齿、有效的帧率/分辨率设置、相机地平线和距离限位、程序化天空盒及场景时间。入口为设置中的“性能”和“场景”。实现、测试和性能口径见 [查看器画质与场景控制](../docs/重构设计/查看器画质与场景控制-2026-11-10.md)。

## 开发与构建

在本目录执行。当前已验证 Windows x64、Rust 1.97.1、Node 24.22.1、pnpm 11.25.0、MSVC/Windows SDK 和 WebView2。

    pnpm install --frozen-lockfile
    pnpm desktop:dev

构建独立 exe：

    pnpm desktop:build

输出 target/release/itasha-desktop.exe。当前不生成安装包；Windows 需要已安装 WebView2。
单独执行 pnpm dev 只能预览界面，没有伪造的浏览器后端。

## 实现边界

| 模块 | 责任 |
| --- | --- |
| src/ | React 界面、只读投影、Tauri bridge |
| src-tauri/ | 窗口、权限、异步命令入口 |
| crates/contracts/ | 版本、会话、错误和读模型 |
| crates/domain/ | 工程文档、名称校验、修订和撤销历史 |
| crates/application/ | 串行应用服务、会话校验、重复请求保护 |
| crates/renderer/ | 通用静态 GLB 读取、PBR 材质、原生 wgpu 渲染与相机 |
| crates/persistence/ | 本地草稿、校验摘要、替换写入、工作目录独占锁 |
| tools/ | 归档、资源清点、静态边界检查、真实桌面 smoke |

前端工程入口为 read_view、execute、close_clean 与受限目录选择 choose_directory；3D 入口为 viewport_load、viewport_bounds、viewport_status、viewport_reset、viewport_surface、viewport_decal；素材入口为 import_image、asset_bytes。Rust 按当前工程会话选择资产，React 不传任意文件路径或几何数据。生产 UI 不读取旧引擎资源，也不创建 iframe 或业务 HTTP 请求。Vite 是开发服务器。
AppManifest 自动生成自定义命令权限，capabilities/main.json 只授予主窗口需要的命令和事件监听。

工程存储于 Tauri app_data_dir，标识 studio.itasha.editor.next，独立于旧版 projects/。
内部 JSON 保存草稿、车型 ID、四类基础选配及图层映射，8 MiB 上限，含内容 SHA-256；图片规范化为 PNG 后按内容摘要保存于 blobs/；不能当作已交付的便携工程 v2。
同目录临时文件、文件同步和替换写入保证正常运行下完整文件切换，不声明断电持久性。
同一工作目录仅允许一个持久化服务实例。

应用服务运行于 blocking worker 并通过 Mutex 串行执行。小草稿保存期间编辑排队。
模型准备和素材操作使用 blocking worker，原生渲染线程处理视口消息；遮罩和贴花使用二进制 IPC。不可变快照后台保存与完整任务调度属于后续增量。
撤销覆盖重命名和图层编辑，最多 100 条并限制历史元数据 16 MiB；重开工程会建立新 session epoch，历史不跨重开持久化。
成功请求保留最近 256 条 ID 与操作指纹，用于进程内重试去重；不是跨重启事务日志。
自动保存尚未实现，设置中保持关闭且拒绝启用。
列表遇到损坏工程时会显式报错，当前尚无逐文件隔离和修复 UI。

## 验证

    pnpm build
    pnpm check:boundary
    pnpm test:transforms
    cargo fmt --all --check
    cargo test --workspace --exclude itasha-desktop --locked
    cargo build -p itasha-desktop --features custom-protocol --locked
    pnpm test:desktop

桌面 smoke 使用本工作区自己的 devDependencies（playwright-core 与 pngjs），不读取旧 Electron 目录。ITASHA_MODEL_ROOT 可指定模型目录，缺省为相对 NEXT 的 ../../3D。
它启动真实调试版 Tauri，用独立 .reports/ 数据目录测试 Rust IPC、保存、重开和 Windows 关闭事件。
ITASHA_TEST_DATA_DIR 用于验收进程隔离工程目录。WebView2 调试端口由测试进程临时开启，应用源码不会启用它。
报告、截图、WebView 数据和编译产物均不纳入 Git。

    pnpm inventory

资源清点只读旧车型清单，流式计算哈希，不执行旧引擎、不联网、不复制模型，输出 .reports/legacy-assets.json。
资源文件较大，扫描会占用磁盘带宽。

    node tools/audit-model-package.mjs

只读清点用户处理后的 9 个模块 GLB 与 119 张 PNG，输出 .reports/model-package-audit.json。默认直接使用工作区旁 3D/Blender/software，不复制原资源。移动程序后可在设置中选择模型包目录。

    pnpm catalog:build

从本地捕获数据生成 next/catalog/vehicles.json。该目录作为静态数据编入 Rust，不执行旧代码；重新生成前应评估已保存工程的 catalogVersion / ID 兼容性。

归档与下一步参见 [实施记录](../docs/重构设计/实施记录-2026-11-09.md)。
