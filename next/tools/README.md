# NEXT 工具入口

所有日常命令在 `next/` 执行。前端与依赖安装见上级 README；桌面 smoke 的 Playwright/pngjs 由 NEXT 自己的 devDependencies 提供。

| 类型 | 工具 |
| --- | --- |
| 日常检查 | check-boundary、test-transforms、test-editor-properties、test-whole-layout |
| 真实桌面 | smoke-desktop；另有 paint/whole-layout 专项 smoke |
| 当前模型清点 | audit-model-package；renderer 的 verify_paint_package 示例 |
| 目录生成 | build-vehicle-catalog，读取根 config/vehicles 来源数据 |
| 离线历史审计 | inventory-legacy、archive-legacy |
| 资产制作 | Python 漆面分类、修复、展开、镜像、封口前处理等 |

smoke 可设置 `ITASHA_MODEL_ROOT`（缺省相对 NEXT 为 ../../3D）和 `ITASHA_SMOKE_EXE`（相对 NEXT 或绝对 exe 路径）；报告均写入忽略的 .reports。

资产制作/诊断脚本包含前代地图、分类图、PSD/AO 与本机 SU7 默认路径。它们需要从本地基线/历史归档恢复输入并按 CLI 参数指定输出，不能作为克隆源码后的必跑构建步骤，也不适用于其他车型的通用参数。仅 map v24、回归 v21 和 v15 compact-report 随当前源码版本化。

archive-legacy 描述旧布局的本地归档操作；使用前必须核对目标与已有归档，不属于 NEXT 发布流程。历史原料恢复见仓库 docs/仓库管理.md。
