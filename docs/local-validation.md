# 本地验证操作手册（LLM Workbench v0.8.0）

面向开发者：从零编译 → 打到 DBX 里 → 逐项真机验收。全流程已在 Windows x64 本机实测通过。

---

## ⚠️ 先读这个：进插件后看到的不是大模型工作台？

这个插件是从上游 Browser 插件 fork 的，**默认先显示一个「伪装界面」**——它故意长得像普通的 SQL
查询工作台（有连接、query-1.sql、结果表格），这是「老板键」功能，不是插件坏了，也不是 DBX 的查询页。

| 你看到的现象 | 原因 | 怎么办 |
|---|---|---|
| 打开插件是 SQL 查询界面 | 伪装层默认开启 | 按 **Escape** 切到真实界面；或点工具栏 **设置 → 设置页签 → 取消勾选「打开工作台时先显示这个界面」**（即时保存） |
| 点模型行的「打开」，上方地址栏没变化 | 阅读器地址栏和大模型工作台是**两个视图**：模型行按钮进的是独立 WebView2 分屏，不是把阅读器切到官网 | 现在按钮文字已改成 **「工作台打开」**，点击后会同时把官网地址填入上方地址栏、切到工作台栅格并显示提示；想用系统浏览器则点同一行的 **「浏览器打开」** |
| 地址栏显示 DeepSeek，但报错卡片/实际页面还是上一次的豆包 | 阅读标签的 `url` 没跟着工作台切换；工具栏「实时页面」按旧地址启动了单实例会话 | 已修复：切工作台时同步更新当前标签地址并清空旧抓取内容；工作台可见时「实时页面」按钮显示为按下，点它表示退出工作台回到阅读器 |
| 工作台 grid 被顶到可视区外，只看到旧阅读器内容 | 阅读视图的 `display:flex` 覆盖了 `[hidden]`，工作台显示时阅读视图没真正隐藏 | 已补 `.ct__view[hidden]{display:none}`；现在进入工作台只显示分屏画面 |
| 地址栏手工输入 `https://www.doubao.com/chat/` 后点「打开」，报"这个页面没有抓到可读内容" | 这是**阅读模式**抓正文（LLM 官网纯 JS 渲染，服务端 HTML 没有正文） | 现在输入已配置的模型官网会自动转到工作台；未配置的 AI 官网会自动登记模型并打开工作台。旧页面残留的空正文卡片也提供 **「在大模型工作台打开」** / **「用系统浏览器打开」** 两个按钮 |

**正确进入工作台的三种方式**（本次 UX 修正沿用 0.8.0 版本号，覆盖安装即可）：

1. 工具栏点 **「大模型工作台」**（▦ 图标）→ 直接切到工作台；若还没有 pane，会自动打开「大模型」设置页。
2. 设置 → **大模型** 页签 → 模型行点 **「工作台打开」** → 该模型开一个独立 pane，地址栏同步填入官网地址（首次启动 WebView2 约需几秒）。
3. 在上方地址栏输入任意已配置模型的官网地址（或豆包 / ChatGPT / Kimi / DeepSeek 等常见 AI 官网），点 **「打开」/回车** → 自动转到工作台，不再走阅读器抓取。

模型行另有 **「浏览器打开」**：拉起系统浏览器访问官网，完整渲染、可正常登录；适合只在一个网站聊天、不需要分屏的场景。

进工作台后：每个 pane 顶部可登录（登录态按会话保存，下次免登录），工具栏「批量群发」多模型群发，pane 内 ⬆/⬇ 上传下载。

> 如果点了「打开」后某个 pane 一直停在"启动中"或报"实时页面启动失败"，说明真实 WebView2 会话没起来：
> 确认已装 *Microsoft Edge WebView2 Runtime*（本机已装 157 版），然后看 DBX 的 sidecar 日志。

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
