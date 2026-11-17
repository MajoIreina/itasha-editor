# 附录 B：性能验收与需求追踪

导航：[总览](./README.md) · [跨阶段契约](./附录A-跨阶段数据与并发契约.md)

状态：Draft 0.1。所有 E/G 编号为待实施验收项，当前结果均为未执行。

## 1. 需求到设计与测试

| 需求 ID | 用户要求 | 设计位置 | 实施任务 | 退出证据 |
| --- | --- | --- | --- | --- |
| R-01 | Rust 主框架、Tauri、React/TS | 阶段 1、附录 A | T1-01～T1-04 | release 离线启动、Rust authoritative state、UI 对照 |
| R-02 | 去掉小米 iframe/引擎，仅保留模型材质 | 阶段 0/2/4 | T0-01、T2-01～T2-04、T4-04 | 独立资源载入、无旧脚本、车型材质/覆盖对照 |
| R-03 | HTTP 操作迁入主框架 | 阶段 1/4、附录 A | T1-02、T4-04 | command/IO 契约、实际无业务监听、断网全流程 |
| R-04 | 外壳尽量还原现有设计 | 阶段 0/1/3 | T0-02、T1-03、T3-03 | 页面/弹窗/分栏/快捷键/缩放截图与交互矩阵 |
| R-05 | 通用车模支持 | 阶段 2/3/4 | T2-02、T3-02、T4-02 | 两种不同拓扑模型，renderer 无品牌分支 |
| R-06 | 性能时间范围与分阶段验收 | 本附录及上位方案 | 各阶段 telemetry/bench | 版本化原始数据、统计与失败说明 |
| C-01 | 既有工程和制作输出兼容 | 阶段 3/4 | T3-04、T4-01～T4-03 | v1→v2 remap、尺寸/像素/材料回归 |

R-01～R-05 来自用户明确要求；R-06 延续上位性能方案；C-01 是迁移所需的工程兼容约束。额外格式/新车型商业适配不自动纳入一期交付。

## 2. 可独立审阅的任务包

| ID | 任务包 | 输入依赖 | 输出 | 主要角色 |
| --- | --- | --- | --- | --- |
| T0-01 | 资产盘点、独立读取、remap、材质缺口 | 现有存档 | 资源报告/转换原型/对照样本 | 资源/渲染 |
| T0-02 | 同窗 host、DPI、输入、modal spike | 自制模型/目标机 | 承载 ADR、窗口矩阵 | 桌面 |
| T0-03 | 旧基线重测与 workload 固定 | 可运行旧版 | manifest/原始测量/fixture hash | 性能/QA |
| T1-01 | Tauri/React workspace、CI、发行资源 | 无车 fixture | 空 shell debug/release | 桌面/前端 |
| T1-02 | actor、DTO、commands、permissions | 附录 A | 契约测试、空工程保存 | Rust |
| T1-03 | 现有 UI 分屏/弹窗/快捷键迁移 | 旧截图/DOM | React 组件与视觉记录 | 前端 |
| T1-04 | host 生命周期/关闭/任务 | T0-02、T1-01/02 | 窗口 E2E、取消/恢复 | 桌面/Rust |
| T2-01 | 导入器与 PBR 基础 renderer | G0 route | scene/纹理/设备恢复 | 渲染 |
| T2-02 | 语义 surface、variants、第二模型 | T0-01、T2-01 | 通用 adapter、能力矩阵 | 资源/Rust |
| T2-03 | camera/BVH/pick/LOD | canonical geometry | 坐标/拾取 fixtures | 渲染/geometry |
| T2-04 | 材质等价与加载性能 | 模型/纹理 profiles | 颜色/环境对照和 trace | 渲染/QA |
| T3-01 | gesture/session/history | T1-02、T2-03 | 幂等事务与竞态用例 | Rust |
| T3-02 | 曲面投影/接缝与后台计算 | T2-02/03 | golden geometry 与预算 | geometry |
| T3-03 | 2D/3D 联动、UI read model | T3-01/02 | 真实手势/属性/性能验收 | 前端/渲染 |
| T3-04 | 旧图层语义迁移与输出 fixture | T0-01、T3-02 | v1 锚点/排序/尺寸对照 | Rust/QA |
| T4-01 | v2、recovery、原子保存 | T1-02、T3-04 | 故障注入与重开报告 | Rust |
| T4-02 | 全格式素材/模型资源管理 | T2-01、原格式清单 | decoder 与资源生命周期矩阵 | Rust/资源 |
| T4-03 | PNG/SVG/PDF/ZIP/离屏输出 | exact projection、color profile | 成品尺寸/视觉/取消结果 | 输出/QA |
| T4-04 | 旧 runtime 移除、打包、升级 | 全功能闭环 | 离线候选包/发布记录 | 桌面/QA |

角色是职责划分，不表示已安排人员或启动子代理。按上位方案 3–4 位工程人员估算约 12–20 周；原型失败、材质重建和资产转换难度可改变周期。阶段 1 shell、契约和测试可与 G0 调研并行，依赖未满足的具体功能不能标记完成。

## 3. 工作负载与硬件

每次测量记录：源码 commit/dirty 状态、构建 hash、Rust/Tauri/wgpu/WebView 版本、CPU、实际 GPU adapter/backend、驱动、电源模式、RAM、存储、窗口物理像素、DPI、刷新率、质量 profile、模型/工程/素材 hash。Rust viewport 与 WebView 可能选用不同 GPU，两者都记录。

| workload | 定义 | 比较用途 |
| --- | --- | --- |
| B0 旧基线复现 | 与现有 benchmark 相同素材、约 9,834 面跨面层、1600×1000 中画质 | 旧→新相同功能比较；模型转换后用 remap 确认等价 |
| B1 常规目标 | 100 层/其中 10 曲面层、规定 4K 素材集 | 验证新产品常用容量；不能直接和旧单层结果算加速比 |
| B2 压力 | 200 层、单层近 48,000 面、8K 素材批量 | 有界内存、取消/退化、尾部延迟 |
| B3 通用模型 | 自制第二车辆，异拓扑/尺度/UV | adapter 通用性与无品牌特判 |
| B4 输出 | 固定尺寸/DPI/透明/字体/颜色 fixture | 保存、导出、一致性 |

预估硬件 A 为 Core Ultra 7 265K、64 GB、RTX 5080 级独显与 NVMe；现有报告未证明实际 active GPU，因此旧数据不能冠以该 GPU 实测成绩。低配档 B 暂定义为 4～8 核 CPU、16 GB、集显、NVMe，最终必须用一台具名设备固定基线，未固定前区间可信度较低。

0.5～2 百万可见三角面、100～300 draw calls 是上位方案的暂定压力范围，不是已测 SU7 模型统计。G0 实际盘点后更新 B0/B1 的精确计数。首次标准化转换时间单独记录，不混入后续模型加载时间。

## 4. 指标定义

| 指标 | 起点→终点 | 限制 |
| --- | --- | --- |
| launch-to-home | OS 启动进程→React 首页可接受输入 | 包含 WebView/runtime 初始化；冷进程与热进程分开 |
| vehicle-ready | 受理打开车型→首帧提交且可正确拾取 | 若可测实际 present 再单列；需包括几何/BVH/纹理准备 |
| import-preview | picker/drop 返回文件 grant→layer preview 可显示 | 排除用户选文件停留时间；含读取/解码/预览 |
| preview CPU | 最新合并输入→geometry 结果/renderer CPU submit | 不是硬件输入到像素 |
| active frame interval | 连续真实拖动期间相邻视口帧 | 禁止混入大量空闲帧降低中位数 |
| input-to-present | 被处理事件时间→能关联到该 seq 的呈现 | 仅在平台 trace 可关联时报告；否则 unavailable |
| save-working-copy | 已提交 snapshot 受理→HEAD 已持久化 | 记录新增素材字节与是否 first save |
| export-portable | snapshot 受理→完整便携文件替换 | 不能套用仅 metadata 的工作目录增量保存预算 |
| export-raster | exact projection snapshot→文件写入完成 | 分 GPU/readback/编码/IO，含内存峰值 |

首次打开区分：新进程新 GPU 资源、OS 文件缓存热；真正冷盘样本仅在控制过缓存条件时标注。重新打开需写清模型是否仍在 CPU/GPU cache，不能把进程存活缓存结果当作应用重启加载。

## 5. 预估区间与 Gate

以下沿用上位方案给出预算方向，但修正计时定义。它们全部是待原型校准的预测；Gate 值为拟定验收上限，G0 后冻结，不在结果产生后移动阈值。

| 操作 | 硬件 A 预估 | 硬件 B 预估 | 进入验收时的处理 |
| --- | ---: | ---: | --- |
| launch-to-home | 0.25–0.70 s | 0.50–1.30 s | 记录至少 30 次，P95 对照上限；冷/热分别报告 |
| vehicle-ready，已规范化资产新进程 | 1.2–3.0 s | 3.0–7.0 s | 同模型/质量；排除一次性转换并单独列出 |
| vehicle-ready，缓存可复用 | 0.4–1.2 s | 0.8–2.5 s | 清楚标记 CPU/GPU cache 条件 |
| 4K PNG/JPEG import-preview | 0.25–0.80 s | 0.8–2.0 s | 16-bit/ICC/特殊格式分开，不保证同区间 |
| geometry CPU P95，B0 | 2–8 ms | 5–16 ms | 分搜索/UV/分配，精确与 preview 分开 |
| 跨面 preview 处理至 submit P95 | 10–20 ms | 18–33 ms | 独立于帧间隔，禁止等同 FPS 或 input-to-present |
| 工作目录保存，约 6 MB 初始工程 | 0.08–0.35 s | 0.15–0.70 s | 标注第一次写 blobs / metadata-only；便携 ZIP 另测 |
| exact 曲面面板合成，固定尺寸 | 0.05–0.20 s | 0.15–0.60 s | 与旧 createPanel 测量尺寸一致才可对比 |

交互发布档位单独定义：目标 60 Hz 下 active frame interval P95 ≤20 ms（16.67 ms 理想帧周期，留计时抖动），超过 33.4 ms 的帧比例 ≤1%；低配 30 Hz 档 P95 ≤33.4 ms，超过 50 ms 的比例 ≤1%。若原型只能达到 30 Hz，记录能力档位并调整明确的质量设置，不能把 10–20 ms CPU 时间直接宣称稳定 60 FPS。120 Hz 是另一个 8.33 ms 预算，不自动包含在本期指标中。

上位方案把旧 event→rAF 的 37.3 ms 与新处理预算算成倍数，口径不足；本轮不继续引用那个“1.9–3.7 倍”作验收结论。只有同定义、同场景重测才能计算加速比。GPU 优化也不自动保证 Tauri 启动更快或总内存更小。

## 6. 采样与统计

- 单次加载/保存/导入至少 30 次有效操作，报告所有单次值、p50/p95/max；若只有 5 次，写为探索样本的范围，不能宣称稳定 P95。
- 连续交互每种 workload 至少 5 轮，每轮至少 30 秒；每轮分别统计活跃帧，报告各轮分布和最差轮，避免把不同硬件/档位混成一个 P95。
- 软件注入输入和真人输入各保留一组，测旧 benchmark 未覆盖的真实 3D 位置拖动；变换柄缩放另命名。
- 时间用单调时钟；CPU/GPU/OS trace 时钟分别注明、关联 request/gesture seq。跨时钟未校准时只报告各阶段，不能相减制造端到端结果。
- 渲染/GPU timestamp 可用性、溢出、后台节流、最小化和失焦时段写入数据；没有 GPU duration 不用 CPU submit 假代。
- JS heap、Rust resident/allocated、WebView/GPU 进程 working/private、应用 texture/mesh 估算字节分别记录；共享内存不可简单求和当物理占用或显存。
- 生命周期 20 次，初始 warmup 后比较平台与斜率；缓存驻留应有可解释上限。一次大 working set 不足以证明内存泄漏。

## 7. 功能/视觉对照

UI 对照包含首页、新建/选车/配置/编辑、仅编辑/仅3D/双视图、素材/图层/属性、相机、设置、导出、打开/错误/恢复弹窗及快捷键。记录 1600×1000、1280×800、720×520，100/150/200% DPI 的基线截图；差异标记 expected/unexpected，不能只比较首页。

几何 fixture 检查 bbox、单位、semantic selection、表面锚点、接缝 UV（初设 ≤1e-5）、参考姿态变换和 canonical↔LOD remap。数值容差在 fixture 中版本化。渲染外观先固定环境、曝光、色调映射、相机和材质；材质视觉阈值由 G0 样片评审冻结，不用单一像素误差替代所有 PBR 效果。

制作输出尺寸/像素/DPI/bleed 为精确断言；文字与透明边缘的抗锯齿差异单独登记。色彩转换、font fallback、PDF raster/矢量语义清楚记录；没有模型时可恢复数据不代表可重新计算 exact 投影。

## 8. Gate 记录格式

每个阶段提交一份记录，建议字段：

```text
gateId / status: pending | passed | conditional | failed
sourceCommit / dirtyPatchHash / buildHash
contractVersion / modelFingerprint / fixtureHashes
hardwareProfiles / measuredAt / runtimeVersions
testCommands / results / artifacts
requirementsCovered / unmetRequirements
decisions / limitations / nextActions / responsibleRole
```

现在 G0～G4 全部 pending。文档齐全只表示可开始设计评审，不代表技术门已通过。测试和原型产物可以在上述任务包实施后生成；本次不创建伪造的通过记录。
