# 离线资源目录

Git 仅管理 `custom.js`、`custom.css` 和本说明。`data/`、`ui/`、`manifest.json`、`extras.json` 来自本地车型存档，由 `node tools/local-assets.cjs --from <完整目录>` 恢复。

详见 `docs/离线资源.md`。本地资源不会因 Git 提交而移动或删除。
