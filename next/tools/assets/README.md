# 资产工具

这里存放 NEXT 的离线模型和车漆映射工具。应用运行时读取 `next/assets/` 中已封存的资产，不在启动时调用这些脚本。原始车辆模型、Blender 工程和本地审阅证据继续留在仓库外部或被忽略的 `.reports/` 中。

## 当前审阅流程

| 工具 | 用途 |
| --- | --- |
| `audit-paint-integrity.py` | 读取原始 GLB、OEM/SDK 基线，按原 source face 审计覆盖、部件和可见性采样；GEOS 结果用于发现问题。 |
| `render-paint-omission-evidence.py` | 生成遗漏区域的上下文与细节图；单独的 source face 高亮仅用于定位，不证明其正常外露。 |
| `repair-reviewed-paint-gaps.py` | 在明确审阅的 selection 内生成候选；左前下翼子板使用真实左侧 source geometry 和自身 UV0。 |
| `conform-paint-fragments.py` | 在同一 source face 内整理片段，按真实 source index edge 细分；`--repack-only` 仅调整 UV locator 并移除证明为零的 source domain。 |
| `connect-reviewed-source-hinges.py` | 对明确审阅的原 GLB 共边重新验明身份及双方支持区，只增加传播关系，不改变已有几何或遮罩。 |
| `audit-source-union-exact.py` | 按实际 f32 值、完整 homogeneous barycentric 行和有理数垂直积分独立计算 source union、重复面积及双向差。超薄片以此结果复核浮点叠加。 |

每次生成都使用新的输出目录；脚本拒绝覆盖已有结果。模型、输入映射和审阅清单必须通过指纹核对。先在 `.reports/` 生成候选和证据，通过封存、运行时传播、物理尺寸、微片与兼容性检查后，再替换运行时资产。

Python 工具需要 NumPy 和 Shapely。具体输入路径、哈希、命令和验证结论应随被采用资产的 `build-report.json` 保存。本地审阅输入不随 GitHub 提交，不能仅凭精简报告重新生成全部 OEM 资源。

封存由 NEXT 的 Rust 实现完成，在 `next/` 内运行：

```powershell
cargo run -p itasha-surface-map --example seal_map -- <候选目录>/paint-map-unsealed.json <候选目录>/paint-map.json
```

## 审计口径

完整原面 source domain、存储后的 f32 source domain、存储后的 world 三角形和 UV locator 分别记录。正 source 面积不能因一个任意面积阈值而丢弃；world 零面积判断将 f32 端点提升为 f64 后，只接受 cross 恰好为零。f32 编码引起的正向、反向 source 差异必须单独报告，不能合称为 world 不可表示。

UV microchart 为很小的正面积片提供可表示的 locator。它的最小宽高会明确记录；贴图的物理尺寸仍由 world/source metric 决定。

原 GLB 共边上的 source hinge 只在双方已审阅遮罩的共同近边支持区允许传播，输出始终裁剪到已有遮罩。支持区测量带不增加涂装域，也不填补真实缝隙。可见性射线只证明采样点可见，不能据此把包含该点的整个 polygon 或内板加入外露面。

## 历史工具

其余生成器保留用于历史重现。尤其 `mirror-lower-fender.py` 是旧 v24 最近中心匹配方案，不能用于建立新的权威 source face 对应关系。不要通过重新运行历史生成器覆盖当前已审阅资产。
