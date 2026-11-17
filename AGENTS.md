# NEXT 开发主线

用户于 2026-11-17 指定：以重构后的 NEXT 替换原 GitHub 项目，今后开发统一基于 NEXT。

- 唯一应用工作区为 `next/`：Rust + Tauri 2 + React/TypeScript + wgpu。功能、修复和测试均进入对应的 `next/src`、`next/src-tauri`、`next/crates`、`next/tools`。
- 当前物理目录与开发入口见 `docs/项目结构.md`。React 按 `app`、`features`、`bridge`、`shared` 分层；功能层不反向依赖 app，共用层不导入功能组件。业务权威状态留在 Rust，前端不另建一套工程历史。
- Tauri 启动与状态在 `host.rs`，IPC 在 `commands/`，车型专用组装在 `adapters/`，原生窗口和线程在 `platform/`；通用业务/算法继续放入已有 6 个 crates。新增 IPC 须同步注册、权限、契约与调用。
- 工具按 `checks/tests/smoke/catalog/assets/legacy` 分类。优先使用 package scripts；直接调用脚本时使用新目录并按脚本自身位置解析工作区根，避免依赖本机旧文件或调用者 cwd。
- 根目录可能仍有本机旧版 `app/`、`backend/`、`desktop/` 和旧可执行文件；这些已退出版本管理，只作历史恢复依据，不继续开发、不重新纳入主线，也不作为 NEXT 的运行或测试依赖。
- 车型来源数据 `config/vehicles.json` 与 `vehicles/*/backend-adapter.json` 仅供 `next/catalog` 生成工具使用，不代表恢复旧版应用链路。
- 当前运行映射为 `next/assets/paint/su7-v24-mirrored/paint-map.json`。v21 是兼容回归样本，v15 只保留整车布局测试所需的尺寸报告。历史试验、未封口映射、Blender、原始车辆模型和用户数据保留本地，不强制加入 Git。
- 修改车型资产时优先读取 `C:/Users/zhout/.codex/skills/xiaomi-ev-model-adaptation/SKILL.md`。本机 SU7 基线为 `L:/SU7/3D`，不得把 SU7 参数直接复用于其他车型。
- 车漆、轮组、内饰、方向盘、黑化套装、碳纤维后视镜/迎宾踏板按车型能力独立配置；黑化套装不联动 21 寸运动轮毂。未实现的配置轴须明确说明。
- 提交前在 `next/` 执行相关前端检查及 Rust 测试；涉及原生桌面/3D 时运行真实桌面 smoke。当前已知跨面传播失败见 `docs/NEXT主线切换-2026-11-17.md`，保留回归断言，不以跳过测试掩盖。
- 主线切换不代表旧工程迁移、施工导出或完整发布验收已完成。有日期的历史设计资料只反映对应阶段，以根 README、当前源码和最新验收记录为准。
