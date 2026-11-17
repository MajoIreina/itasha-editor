# 车型目录来源数据

当前主线为仓库 `next/`。此处只版本化各车型的 `backend-adapter.json`，供 `next/tools/build-vehicle-catalog.mjs` 读取来源选配并生成 NEXT 静态目录。文件名沿用来源记录，不代表应用依赖旧后端。

原始捕获、manifest 与下载数据保留本地，由忽略规则排除。NEXT 当前仅接入 SU7 Max 原生模型，其他车型的目录选项不代表完成适配。
