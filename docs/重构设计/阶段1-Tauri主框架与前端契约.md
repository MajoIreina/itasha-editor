# 阶段 1：Tauri 主框架与前端契约

导航：[总览](./README.md) · [前置阶段](./阶段0-模型资产与渲染承载技术预研.md) · [公共契约](./附录A-跨阶段数据与并发契约.md) · [验收口径](./附录B-性能验收与需求追踪.md)

状态：Draft 0.1，待 Gate 0 路线冻结
估计：2–3 周
依赖：Gate 0 的 viewport 路线决议
目标：新空工程与工作台外壳可运行；所有应用行为通过 Tauri contract，无 localhost API

## 1. 范围

建立 Tauri 2 + Rust workspace + React + TypeScript + Vite 工程、运行时安全配置、命令与事件 schema、工作台静态外壳和开发/发行构建。提供可由设置打开的 viewport host 接口，但 3D 核心在阶段 2 实现。迁移工程项目列表最小 happy path，仅创建空工程、保存并重新打开；不接入当前模型资源、旧曲面算法和完整导出。

阶段 1 不重写渲染器，不搬迁业务 API URL，不将 `studio.js` 逻辑直接换语法放入单个 React 组件。

空工程是 `draft` 状态，vehicleRef/modelFingerprint 可以为空，图层放置和曲面工具暂不可用；选车并验证资产后进入 `configured/editable`。这也是资产暂缺时 shell 与项目功能可以先行的明确领域状态。

## 2. 工程拓扑

```text
src-tauri/
  src/main.rs                 Tauri 启动
  src/app.rs                  command 注册与应用状态容器
  src/commands/               参数解析/权限检查/调用 use case
  capabilities/               窗口 command allowlist
crates/
  contracts/                  Rust DTO、版本、TS 生成入口
  domain/                     项目和车辆类型；无 Tauri 依赖
  application/                session/document actor 与 use cases
  persistence/                工作目录、便携 v2、原子保存
src/
  app/                        React 装配与路由/阶段
  features/                   project、workspace、settings UI
  bridge/                     typed invoke、事件、channel DTO
  shared/                     tokens、按钮、输入控件、通用布局
  styles/                     外壳样式
```

Domain crate 不依赖 Tauri、React 概念或 GPU handle。Commands 只做边界校验和用例调度，不在 command 函数中执行大型解码或几何遍历。UI 的 CSS/DOM 完全由 React WebView 所有。viewport host 为 Gate 0 所选实现提供统一 mount/resize/show/close/health/diagnostics 抽象；若 native 视口无法透明叠层，React 不直接猜测平台窗口坐标。

开发构建允许 Vite dev URL；发行运行使用打包静态资源，不启动业务 localhost 服务或 Rust HTTP listener。source scan 排查旧 `/__api`、`/__archive`、Service Worker 注册和业务 `fetch()`，发行包运行时再检查实际监听与外网请求；不能仅用源码搜索替代运行验证。Tauri 内部协议/资源 URL 与开发 HMR 不属于旧业务 HTTP 链。

## 3. 状态所有权

| 状态 | 权威所有者 | UI 表示 | 持久化 |
| --- | --- | --- | --- |
| 项目文档、车辆 ID、图层、资产引用、surface anchor | Rust domain/application | React read model，以 revision 更新 | 工程 v2 |
| 选择、面板展开、搜索、弹窗、键盘焦点 | React UI store | UI store | 用户设置可持久化；临时选中不入工程 |
| viewport 相机和 hover | viewport runtime | React 通过事件显示摘要 | 相机偏好按项目设置决定；hover 不存 |
| 正在拖动中的 preview | viewport session | 只读临时 overlay | 不保存、不写入 history |
| 图片像素、mesh buffer、GPU texture | 资源/renderer owner | AssetRef/诊断值 | 仅原图/工程语义数据持久化；GPU handle 不持久化 |
| revision、contentId、dirty、pending task | Rust application | UI projection | task 不存；dirty 比较 currentContentId 和 savedContentId |

React 可以乐观更新控制项，但应用命令失败时必须回滚到最后一个确认 revision。高频 viewport preview 在 viewport 自身运行，commit 后才向 Rust 发 edit command。不能让 React store 与 Rust Project 同时成为可独立修改的权威工程副本。

## 4. Command 与事件 DTO

接口由 `crates/contracts/` 定义并提供生成的 TS 类型；公共规则见附录 A。enum 使用稳定字符串标签；ID 为 UUID 或具名稳定字符串；u64 revision 在 JSON 中用十进制字符串；浮点值在边界做 finite/range 校验；错误 message 可本地化，稳定 `code` 用于 UI 和自动化。

### 首批命令

| 命令 | 请求 | 返回/效果 |
| --- | --- | --- |
| `app_get_info` | 无 | app 版本、schema 支持范围、渲染能力、data root 的可读状态（不暴露秘密绝对路径） |
| `project_list` | pagination/排序 | 工程摘要，不读取整个素材包 |
| `project_create` | name、vehicleId? | SessionRef、revision="0" 的文档摘要；阶段 1 可使用无车空工程 |
| `project_open` | ProjectRef | 已校验和迁移的 ProjectSnapshot；错误时当前工程不被替换 |
| `project_save` | SessionRef、expectedRevision、requestId | taskId；Rust 捕获已提交 snapshot，完成返回 savedRevision/contentId；前端不传工程副本 |
| `vehicle_list` | 无 | 可用车型、能力 flags、资产状态/适配版本 |
| `asset_import_begin` | 文件选择选项/导入模式 | taskId；Rust 打开原生文件 picker |
| `task_cancel` | taskId | 尝试取消可取消的文件/解码/几何任务 |
| `task_get` | taskId | 状态、阶段、完成数和稳定错误码 |
| `settings_read` / `settings_write` | 版本化偏好 | 持久化用户设置；不混入工程数据 |

阶段 1 可先实现前四类工程命令和设置；其他命令 DTO 在阶段 2/3 扩充。不要用一个 `execute(operation: String, payload: JSON)` 的万能 RPC 绕开类型边界。

### 事件与异步任务

- `project-changed`: `{projectId, revision, dirty, changeSummary}`。
- `task-progress`: `{taskId, phase, completed, total?, cancellable}`。
- `task-finished`: `{taskId, outcome: success|cancelled|error, resultRef?, error?}`。
- `renderer-state`: 初始化/ready/degraded/device-lost/restoring，包含 capability id 和 adapter 字符串。
- `viewport-event`: 仅语义事件，例如 selection/hover summary/viewport health；不逐个转发 pointermove。

每项长任务带 taskId、projectId、generation；切换工程时旧 task cancel，无法取消的结果必须由 generation 拒绝。进度 channel 有界或合并，避免每帧 event 堆积。

### 错误契约

首版稳定错误码：`invalid-input`、`unsupported-format`、`asset-not-found`、`asset-too-large`、`project-corrupt`、`schema-unsupported`、`revision-conflict`、`cancelled`、`renderer-unavailable`、`permission-denied`、`io-error`、`internal-error`。内部日志可含诊断上下文，UI 展示不泄露原始内部路径或令牌。

## 5. Tauri 安全和资源访问

- 显式配置 capability 列表与 windows/webviews 范围；应用自定义 command 纳入构建时 `AppManifest::commands` 和生成权限配置。仅创建 capabilities 文件并不会自动限制所有已注册的自定义 command，需逐项验证非授权窗口调用失败。依据见附录 A 技术参考。
- 内部 AssetRef 限于应用资源根；用户导入/导出由 native picker/drop 产生 FileGrant，允许访问该已选外部路径。Rust 归一化/重检 grant，UI 不传可自行拼接的绝对路径。
- 原生 dialog、剪贴板、拖放和文件拖入分别测权限与文件句柄生命周期。避免前端 bridge 暴露通用路径读写函数。
- UI 静态资源用发行资源映射；车型/贴图访问使用 opaque `AssetRef` 或 asset provider API。Rust 端重新检查资源归属和引用，不依赖 renderer 提供的 path。
- 日志只保留任务、错误和性能摘要；不记录素材字节或认证秘密，用户导出诊断时使用脱敏副本。
- schema 输入设置长度/集合上限；前端类型不等同 Rust 端校验。Tauri IPC 是信任边界。

## 6. React 外壳结构

根据当前编辑器截屏/DOM 清单创建工作台：顶部工程栏、左/主编辑区域、右侧分组面板、底部状态区域和 dialog layer。保留 viewport 区域为可切换状态的 host，而不是继续嵌入旧 iframe。

组件按功能边界拆分：`ProjectHeader`、`WorkspaceShell`、`LayerPanel`、`InspectorPanel`、`AssetPanel`、`CameraToolbar`、`StatusBar`、`SettingsDialog`、`ExportDialog`。共享控件只抽象重复的错误/交互方式，不把页面所有样式写成一个全局覆盖表。

维持当前首页、信息层级、分栏、主题、尺寸、弹窗和快捷键；截图差异逐项登记，用户未要求的视觉改版不混入框架迁移。UI snapshot 至少覆盖 1600×1000、1280×800、720×520 及 150% DPI。每屏评估焦点顺序、键盘操作、中文文字换行和面板 splitter resize。

viewport host 由 React 报告有布局版本的逻辑矩形，平台层一次转换为物理像素并处理 DPI/origin；resize 和指针命中使用同一版本。Native surface 的 DOM 菜单覆盖、dialog/tooltip、跨窗拖放与 IME 必须通过阶段 0，不以普通 HTML z-index 能覆盖原生视图为前提。

## 7. 构建、测试与阶段 1 验收

- debug build 启动静态 React 首页；release build 可离线启动且无 Vite/HTTP 本地服务。
- command 单元测试覆盖参数上限、错误码、revision 冲突、工程替换的原子性；IPC contract 测试检查 Rust/TS DTO 序列化一致。
- 真实 Tauri Windows E2E：新建→命名→保存→关闭→重开→列出；坏文件不会覆盖当前项目；取消 dialog 无副作用。
- capability 自动检查不允许 UI 调用未授权 filesystem/shell/window command；全局检查没有 business fetch/旧 API 路由/Service Worker。
- Shell 对照截图通过，布局无重叠，高 DPI 和 resize 不导致 viewport host 漂移。
- 在 UI 操作 save 的期间打字，保存的 revision 明确；dirty 按 contentId 判断，undo 回到已保存内容时也可 clean。
- 每项长任务支持 progress 结束态和可取消状态；取消后不会误应用迟到结果。

## 8. 阶段 1 交付物

提交 Tauri workspace、React shell、contracts、capability allowlist、创建/保存/打开空工程 use case、CI 构建脚本、contract tests、窗口 E2E、`phase1-acceptance.md`。保持旧应用可构建，以便后续迁移做差异比较。
