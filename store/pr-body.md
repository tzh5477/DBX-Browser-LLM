首次提交 DBX 官方商店（版本 **0.8.0**）。

## 插件

- id: `com.xingxi.llm-workbench`
- name: LLM Workbench（大模型工作台）
- publisher: `xingxi`
- version: `0.8.0`
- 源码: https://github.com/tzh5477/DBX-Browser-LLM （tag `v0.8.0`）
- 许可证: MIT
- 平台: **仅 windows-x64**（sidecar 依赖 WebView2 + CDP；CI 的构建矩阵已限定为 windows-x64，Release 里只有 Windows 产物）

## 能力、权限与数据访问

- 能力：一个 workbench（大模型工作台，保留原网页阅读/实时页面）、一个命令（打开工作台）、命令面板与工具栏菜单入口。
- 权限：`host.storage`（保存模型配置、访问记录、站点设置、书签）、`host.events`、`host.binary`（实时画面二进制帧与文件上传下载分块）。不申请额外系统权限。
- 网络：由原生 sidecar 通过 WebView2 加载用户添加的官网大模型站点；抓取正文时访问用户输入的网址。无遥测、无自建后端、不向第三方上传数据。
- 原生 sidecar：Rust。Windows 上通过 WebView2 渲染实时网页并用 CDP 转发鼠标/键盘、挂载上传文件、接收下载；每个会话使用独立 profile 保留登录态。仅在用户打开对应功能时启动。
- 本地文件：用户上传的图片/文件、下载的模型回复文件写入插件自己的数据目录（`DBX_PLUGIN_DATA_DIR`）；保存到用户指定路径需通过宿主 `saveFile` 由用户确认。不上传。
- 批量群发：按外置 selector 表在已登录的多个模型页面填入文本并触发发送，串行执行、逐模型返回结果；不调用任何模型 API，不绕过登录。

## 校验

- 候选包未签名；`sha256` / `size` 以首次 Release 的 `release-candidates.json` 为准（当前模板为占位值，发布前必须替换）。
- 包内 `manifest.json` 的 id/version/publisher 与候选一致；`engines.dbx` = `>=0.6.0`。
- 无 `signingKeyId`。

如果维护者方便，也请把 `tzh5477/DBX-Browser-LLM` 登记进 `automation/plugin-sources.json`（`autoUpdate: true`、`metadataPath: .dbx-store.json`），后续版本即可自动同步。
