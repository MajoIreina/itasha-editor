# NEXT 工具目录

在 `next/` 工作区执行命令。工具按使用场景分组，脚本用自身位置定位工作区，不依赖调用者当前目录。

| 目录 | 内容 | 常用入口 |
| --- | --- | --- |
| `checks/` | 应用源码静态边界检查 | `pnpm check:boundary` |
| `tests/` | 前端变换、属性与整车布局回归 | `pnpm test:transforms`、`pnpm test:editor-properties`、`pnpm test:whole-layout` |
| `smoke/` | 真实 Tauri/原生视口验收和辅助断言 | `pnpm test:desktop`、`pnpm test:whole-car-seams` |
| `catalog/` | 从来源 registry/adapters 生成车型目录 | `pnpm catalog:build` |
| `assets/` | 模型清点、漆面分类/展开/修复/镜像、几何工具 | `node tools/assets/audit-model-package.mjs <软件模型包>` |
| `legacy/` | 旧布局的本地资源审计与归档 | `pnpm inventory` |

## 常规检查

```powershell
pnpm check:boundary
pnpm test:transforms
pnpm test:editor-properties
pnpm test:whole-layout
python tools/assets/test-paint-uv-geometry.py
```

Python 几何用例放在资产工具旁，直接导入同目录 `paint_uv_geometry.py`。其他资产脚本按需要使用 NumPy、Pillow、OpenCV、PSD 或 Blender，未纳入常规应用运行依赖。

## 真实桌面

```powershell
$env:ITASHA_MODEL_ROOT = 'L:/SU7/3D'
$env:ITASHA_SMOKE_EXE = 'target/debug/itasha-desktop.exe'
pnpm test:desktop
```

smoke 需要先构建 NEXT 的前端和原生 exe。Playwright/pngjs 来自本工作区 devDependencies；模型目录缺省为相对 NEXT 的 `../../3D`。工程、设置、WebView2、报告和截图均写入 `.reports/` 隔离目录。

`smoke/` 的方向回归使用 manifest 指定的当前地图。全车专项另需同指纹的 `.reports/seam-metric-audit.json`，核对 18 个实际遮罩及 24 对接缝双向共 48 组素材 GPU 见证；先按 [复查记录](../../docs/整车跨面复查-2026-11-17.md) 生成审计。专项 `smoke-paint-v2.mjs`、`smoke-paint-parts.mjs`、`smoke-whole-layout.mjs` 保留于同目录，由具体改动选择运行。

## 可选资产与历史工具

```powershell
node tools/assets/audit-model-package.mjs 'L:/SU7/3D/Blender/software'
node tools/assets/inspect-paint-node.mjs 'L:/SU7/3D/Blender/software/body.glb'
```

资产制作脚本包含前代地图、分类图、PSD/AO 和 SU7 默认路径。使用前从本地基线/历史归档恢复输入，并用 CLI 参数指定新输出；不能作为其他车型的通用参数，也不是克隆源码后必跑的构建步骤。当前源码保留运行 v25、迁移/回归 v24/v21 和 v15 compact-report。

本轮工具：`audit-paint-integrity.py` 核对原始 GLB 的车漆材质、源重心和源域覆盖；`repair-reviewed-paint-gaps.py` 按评审范围修复真实缺口及左下翼子板；`conform-paint-fragments.py` 按真实 source 共边拆片和处理紧凑 UV；`render-paint-omission-evidence.py` 生成保留装配遮挡的缺口见证。`mirror-lower-fender.py` 仅用于复现旧近邻近似，不能作为可靠生成链。`connect-reviewed-source-hinges.py` 只连接已证明共同支持区的原始 GLB 边；`audit-source-union-exact.py` 以实际 f32 源重心的有理数并集积分核对量化差。精确输入、命令与排除边界见 [资产工具说明](assets/README.md)、复查记录和运行图 build-report。

`catalog/build-vehicle-catalog.mjs` 读取仓库根 `config/vehicles.json` 和各车型 adapter，输出 `next/catalog/vehicles.json`。更新目录前核对已保存工程的 ID/catalogVersion 兼容性。

`legacy/inventory-legacy.mjs` 依赖本地原始捕获；`legacy/archive-legacy.mjs` 描述旧布局归档。它们不参与 NEXT 正常构建和发布，使用前须核对原料和归档目标。恢复方式见仓库 `docs/仓库管理.md`。
