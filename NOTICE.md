# NOTICE

本仓库是 [SuperJJ007/CSSwitch](https://github.com/SuperJJ007/CSSwitch) 的 **Windows x64 移植分支**（非官方）。
上游以 MIT 许可证发布，原始版权声明保留在 [LICENSE](./LICENSE)：

```
MIT License
Copyright (c) 2026 shanjunjie
```

## 分发时必须保留

- `LICENSE` 原文（含上述版权声明）；
- 本 `NOTICE`（或等价的归属说明），说明这是 **非官方 Windows 移植**，并指向上游仓库。

如果修改了代码再分发，建议在 README 或发布说明里注明改动范围（本分支的改动见 `CHANGELOG.md` 中
`0.8.4-windows` 与后续提交）。

## 第三方组件

| 组件 | 位置 | 说明 |
| --- | --- | --- |
| WebView2Loader.dll | `desktop/src-tauri/resources/WebView2Loader.dll`，随安装包分发 | 来自 Microsoft Edge WebView2 SDK，按 WebView2 SDK 的可再分发条款随应用分发；不做修改。 |
| Rust 依赖 | `Cargo.lock` | 各 crate 按其自身许可证（MIT / Apache-2.0 等）分发，详见各 crate 元数据。 |
| Claude Science | **不随本仓库分发** | 本仓库不包含、不下载、不修改 Claude Science。用户需自行从官方渠道获取，并遵守其条款。 |
| 图标与截图 | `desktop/src-tauri/icons/`、`docs/assets/` | 沿用上游仓库资源。 |

## 无关联声明

本项目与 Anthropic 无隶属或背书关系。"Claude"、"Claude Science" 等名称归其权利人所有。
本工具通过本地回环代理把自己的模型 API 接到 Science 上，不提供 Anthropic 账号权限，
也不绕过任何付费或授权机制；请自行确认你的使用方式符合上游产品条款。
