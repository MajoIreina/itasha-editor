# SU7 v24 mirrored

当前 NEXT 运行映射。完整字节哈希与大小见上级 manifest.json；paint fingerprint 为 `109ba0aa146625f99b35050b2682edb6c0ad15b72e180c515c58867fa7d275d7`。

本版基于 v23，以右前下翼子板的源 UV0 与三角片镜像重建左侧。模型不完全对称，采用近邻对应；近似误差、替换数量与方法保留于 build-report.json。该报告的 awaiting-seal 状态是生成阶段记录，后续已封口文件为当前 paint-map.json，不能把它理解为完整发布验收。

保留 v21 回归样本验证未替换部件的三角身份。现有 small artwork must not jump across the vehicle 回归仍失败，主线切换没有修改传播算法。见仓库根 docs/NEXT主线切换-2026-11-17.md。
