# NEXT 版本化漆面资源

唯一运行入口为 `manifest.json` 的 `runtimePaint`，当前是 `su7-v25-verified/paint-map.json`。指纹、文件大小和 SHA-256 由 manifest 记录。模型指纹为 `99ab99adcfddfb681e966a0309849eb534d13aacb480d34b3f27f69e29b07193`，必须与软件包 `body.glb` 对应。

- v25 verified：真实左下翼子板源面、外板缺口修复、源域并集/共边/UV 定位复查后的运行图。
- v24 mirrored、v21 classified-only：保留旧源面锚点迁移及兼容回归；v24 不再作为运行图。
- v15 compact-report：仅为整车布局测试提供历史尺寸，不保留旧大地图。

换图按真实 source face 和 source bary 重投影，保持世界位置、素材毫米尺寸及图像轴；文档和全部撤销/重做状态原子迁移。旧错误背面、超出源边界、病态微片图片轴误差超过 1°或对应旧图缺失时明确拒绝，不吸附近邻或只改指纹。v22/v23 需本地另行恢复旧图。

v24 的最近质心镜像无法保证一对一源面完整性，还含 UV 顶点双重置换；它的 build-report 保留原生成证据，不作为 v25 的验收结论。`tools/assets/mirror-lower-fender.py` 仅用于历史复现。

本轮发现、逐层验收和精度边界见仓库根目录 [整车跨面复查](../../../docs/整车跨面复查-2026-11-17.md)。运行图目录内 build-report 为精简摘要；完整逐面差集、微片清单、可见性见证及 GPU 报告保留在本机 `.reports/`。原始模型、Blender、贴图和中间候选不随源码分发。
