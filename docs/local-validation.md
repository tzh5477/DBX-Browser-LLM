# 本地验证操作手册（LLM Workbench v0.8.0）

面向开发者：从零编译 → 打到 DBX 里 → 逐项真机验收。全流程已在 Windows x64 本机实测通过。

---

## 0. 前置要求

| 项 | 要求 | 怎么确认 |
|---|---|---|
| Node.js | 22+ | `node --version` |
| Rust 工具链 | stable（含 MSVC target，`rustup-init` 默认即可） | `cargo --version` |
| plugin-cli | 任意能跑的版本（本机 0.1.10） | `dbx-plugin --version`，没有就 `npm install -g @dbx-app/plugin-cli` |
| WebView2 运行时 | Windows 11 自带；Win10 随 Edge 装 | 看「设置 → 应用」里有没有 *Microsoft Edge WebView2 Runtime* |
| DBX 桌面端 | ≥ 0.6.0（`host_api` 1） | DBX 关于页 |

> SDK（`dbx-plugin-sdk`）**不在 crates.io**，只随 `@dbx-app/plugin-cli` 分发。所以**必须用 `dbx-plugin package` / `dbx-plugin dev` 构建**，它会自动注入 patch；直接裸跑 `cargo build` 会报找不到 `dbx-plugin-sdk`。

---

## 1. 编译打包（约 5 分钟，首次更久）

在仓库根目录打开终端（PowerShell 或 Git Bash 都行）：

```bash
cd F:/personal/project/github/DBX-Browser-LLM
dbx-plugin package .
```

产物在 `dist/`：

```
dist/com.xingxi.llm-workbench-0.8.0-windows-x64.dbxp          ← 安装包
dist/com.xingxi.llm-workbench-0.8.0-windows-x64.artifact.json  ← 元数据
```

本机实测：包约 2.9 MB，未签名（review candidate）。包内 `manifest.json` 的 id/version/name 已核实为
`com.xingxi.llm-workbench` / `0.8.0` / `LLM Workbench`，后端 exe 也在包里。

想指定别的输出目录：`dbx-plugin package . --output-dir <目录>`。

### 备选：不装 DBX，用 dev host 看 UI

```bash
dbx-plugin dev --path . --port 5190
```

浏览器打开它给的 `http://127.0.0.1:5190` 即可看 UI。注意：
- 首次会全量编译 **debug** sidecar（几分钟），和 `package` 的 release 产物不共用缓存；
- dev host 是模拟环境，**WebView2 实时画面、CDP 文件挂载、登录态持久这些原生能力测不了**，
  真机验收还是得走第 2 步装进 DBX。

### 只跑测试（不打包，验证代码健康）

```bash
# 后端 46 个单测（需要 SDK patch）
cd backend
cargo test --bin dbx-plugin-webview --config "patch.crates-io.dbx-plugin-sdk.path='<SDK路径>'"
```

SDK 路径 = `<npm root -g>/@dbx-app/plugin-cli/sdk-root/plugins/sdk/rust/dbx-plugin-sdk`，
本机是 `D:/ProgramFiles/nvm/node_global/node_modules/@dbx-app/plugin-cli/sdk-root/plugins/sdk/rust/dbx-plugin-sdk`。
（用 `dbx-plugin package` 就不用管这个 patch，CLI 自己会注入。）

前端语法检查：

```bash
node --check ui/app.js
```

---

## 2. 导入本地 DBX

1. 打开 DBX → **插件中心 → 设置**。
2. 勾选 **「允许安装未签名开发包」**。
   > 未签名的包默认拦装；这个开关只影响本地开发安装，不放宽官方商店校验。
3. 回到插件中心，点 **「安装本地插件 / 从文件安装」**，选第 1 步生成的
   `dist/com.xingxi.llm-workbench-0.8.0-windows-x64.dbxp`。
4. 装好后，工具栏出现 **LLM Workbench** 按钮；也可在命令面板搜 `LLM Workbench` 打开。
   - 因为是新 ID，它和原来的 Browser 插件**可以共存**，互不覆盖。
5. 更新：重新打包后覆盖安装即可（同 ID 替换）。

> 如果装完打不开：先确认 WebView2 运行时已装（Win11 自带）；再看 DBX 版本 ≥ 0.6.0。

---

## 3. 找到插件数据目录（校准要用）

Sidecar 的数据目录由 DBX 注入，规则是：

```
<DBX 数据根>/plugin-data/com.xingxi.llm-workbench/
```

DBX 数据根默认是应用的 app data 目录（若你配过自定义数据目录则以那个为准）。不确定时，
在工作台里打开任一模型，然后用 Everything / 资源管理器搜目录名
`com.xingxi.llm-workbench`，进到里面会看到：

| 文件/目录 | 用途 |
|---|---|
| `web-reader-settings.json` | 模型配置、访问记录、书签、站点预设 |
| `selectors.json` | 群发选择器表（首次运行自动生成，改完重启 DBX 生效） |
| `live-browser/<sid>/` | 每个会话的浏览器 profile（登录态在这里） |
| `live-uploads/<sid>/` | 上传文件暂存 |
| `live-downloads/<sid>/` | 下载文件落地点 |

> `<sid>` 就是模型 ID：旧实时页用 `default`，每个大模型 pane 用它自己的 ID。

---

## 4. 逐项真机验收（按依赖顺序做）

### 4.1 模型管理
1. 工作台右上角「管理模型」→ 抽屉「大模型」页。
2. 应看到 7 个预置模型（ChatGPT / Claude / 豆包 / Kimi / DeepSeek / 元宝 / 通义）。
3. `＋ 新建` 加一个自定义模型（填名称 + 官网对话页网址），确认出现在列表。
4. 编辑改名、删除各试一次；删完确认对应窗口也关了。
5. 打开几个模型后回到这页，看「访问记录」是否按时间倒序记下来了。

### 4.2 登录态持久化（免登录）
1. 打开一个模型 → 在网页里正常登录。
2. 关闭这个 pane，再重新打开同一个模型。
3. **预期**：仍是登录状态（profile 落盘在 `live-browser/<sid>/`）。
4. 再关掉整个 DBX、重开、打开模型，确认登录态还在。

### 4.3 多窗口分屏
1. 打开 3 个不同模型 → 工作台里出现 3 个 pane，各画各的网页。
2. 每个 pane 都能独立点击/输入（中文输入法直接打字）、滚动。
3. 只有一个是「活动」的（持续推帧）；点另一个 pane 应切换活动态，旧的暂停。
4. 关掉一个 pane，其余两个画面正常、ID 不乱。

### 4.4 一键批量群发（需先登录）
1. 先确认要群发的模型都已打开并登录。
2. 顶部群发栏：勾选 2-3 个模型，输入「你好」，点发送。
3. **预期**：每个模型的输入框都被填入并发出，结果区逐个显示成功/失败。
4. **如果某个模型失败**：多半是它的选择器过时了。打开数据目录的 `selectors.json`，
   对照该站点当前 DOM 改 `inputSelector` / `sendSelector` / `trigger`，重启 DBX 再试。
   - 输入框选择器：textarea 一般用 `textarea`；contenteditable 的用 `div[contenteditable="true"]`。
   - `trigger` 填 `click` 或 `enter`。
   - 建议先只勾一个模型验证选择器，再全选群发。
5. 注意：群发是**串行**的，模型多会逐个进行；未打开的窗口要先打开。

### 4.5 图片 / 文件上传
1. 在某个 pane 标题栏点 ⬆，选一张图片或一个文件。
2. **预期**：文件被传到该会话，并被挂载到网页的文件输入框（网页里应能看到文件名/预览）。
3. 在网页里像平常一样发送。
4. 若挂载失败：确认该站点的输入框确实是 `input[type=file]`（CDP 挂载走这个）；
   个别站点用自定义上传组件，可能挂不上，属已知限制。

### 4.6 下载与保存
1. 让模型返回一个可下载的文件（或随便触发一个下载）。
2. 点该 pane 标题栏 ⬇，看下载列表里有没有刚下的文件。
3. 点文件的保存按钮 → 系统文件对话框选路径 → 确认。
4. 去目标路径打开，确认文件完整。
5. 原始下载落在 `live-downloads/<sid>/`，只有点保存才写到用户指定位置。

---

## 5. 出问题时的排查顺序

| 现象 | 先查什么 |
|---|---|
| 装不上 / 提示签名 | 插件中心设置里「允许安装未签名开发包」是否勾上 |
| 工具栏没有按钮 | 命令面板搜 `LLM Workbench`；确认装的是新 ID 的包 |
| 实时/模型窗口黑屏 | WebView2 运行时；首次启动等几秒；退出实时再进 |
| 群发某个模型不生效 | `selectors.json` 对应站点的选择器；先单模型验证 |
| 登录态没留住 | `live-browser/<sid>/` 目录是否生成；是否被清理软件删了 |
| 上传/下载不工作 | 文件是否落在 `live-uploads` / `live-downloads`；权限弹窗是否确认 |

---

## 6. 一句话流程（熟手速查）

```bash
cd F:/personal/project/github/DBX-Browser-LLM
dbx-plugin package .
# DBX → 插件中心 → 设置 → 允许未签名 → 从文件安装 dist/*.dbxp
# 打开 LLM Workbench → 管理模型 → 开 3 个 → 登录 → 群发 → 上传 → 下载
```

真机验收通过后可提 Issues/反馈；发布商店流程见 [publish.md](publish.md)。
验证脚本与说明见 [dev-spec.md](llm-web-workbench-dev-spec.md) §11。
