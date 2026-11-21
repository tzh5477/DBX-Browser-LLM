# 官网大模型聚合插件改造方案（基于 com.dbx.webview）

> 状态：技术方案，未实现。调研基准：`chenjie1024/DBX-Browser` v0.7.10（MIT）、dbx 主仓库当前版本。
> 日期：2026-11-21

## 1. 目标与范围

### 1.1 可验证目标

在 dbx 内交付一个插件，做到：

1. 自定义新增/编辑/删除"官网大模型"条目（名称 + 对话页 URL），配置持久化，重启后保留。
2. 同时打开多个大模型官网对话页，**并排可见**，各自保持独立登录态（一处登录，下次免登录）。
3. 在统一界面一键把同一段提示词发给选中的多个模型，回收各自主回答。
4. 网页内的图片/文件上传、下载可用。
5. 协议 MIT，可合法二次开发分发。

### 1.2 明确不在范围

- RDP/Telnet/串口/游戏等我方需求无关能力；原插件的阅读模式/本地小说保留但不投入。
- macOS/Linux 支持（实时页面依赖 WebView2 + CDP，仅 Windows）。
- 通过官方商店审核（可先本地安装未签名包）。

## 2. 关键架构事实（决定方案的硬约束）

以下均为源码核实结论，是方案设计的依据：

| # | 事实 | 证据 |
|---|---|---|
| F1 | dbx 插件 UI 是 `sandbox="allow-scripts"` 的 opaque-origin iframe，CSP `default-src 'none'`、无 `frame-src` | `apps/desktop/src/components/plugins/PluginWorkbenchHost.vue:954`、`apps/desktop/src/lib/plugins/pluginHostBridge.ts:1208-1213` |
| F2 | 因此插件 UI **无法直接 iframe 嵌入**外部官网；无 DOM、无 Cookie | 同上 + `docs/content/docs/plugin-development.mdx:168` |
| F3 | 同一插件的所有 workbench tab **共享一个 sidecar 进程** | `crates/dbx-plugin-runtime/src/plugins/host.rs:125,172-214`：`sessions: HashMap<plugin_id, Arc<Session>>` |
| F4 | com.dbx.webview 的实时页面是**单例**：`live: Mutex<Option<LiveSession>>`，多标签靠导航切换 | 插件 `backend/src/main.rs:43-46,438-483` |
| F5 | 每个 `LiveSession` = 独立 STA 线程 + 独立窗口 + 独立 WebView2 + 独立 profile，彼此天然隔离 | 插件 `backend/src/live.rs:103-113,224-304` |
| F6 | 帧事件 `webview/liveFrame` **不带会话标识**，UI 侧 `liveState` 也是单一对象 | `live.rs:196`；`ui/app.js:853-867` |
| F7 | `webview/liveEval` 已存在（CDP `Runtime.evaluate`，≤64KB，8s 超时） | `main.rs:357-372`；`live.rs:674-712` |
| F8 | 实时页面**无**文件上传下载（无 `SetDownloadBehavior`/`ChooseFile` 转发） | `live.rs` 全文检索无相关调用 |
| F9 | 登录态保存在 sidecar 数据目录的 WebView2 profile，下次免登录 | `main.rs:449`、README「隐私与权限」 |

**推论**：

- 由 F1/F2：必须走"sidecar 起真实 WebView2 + 帧流"路线，不能走 iframe 直嵌。
- 由 F3/F4：**开多个 dbx tab 无法绕过单例**——所有 tab 共用一个 sidecar，而 sidecar 里只有一个 `LiveSession`。多实例改造是必选项，不是可选项。
- 由 F5：多实例的技术底子好——每个会话本来就是独立线程/WebView2，改造是"把单槽改成 map + 事件带 ID"，不是重写引擎。

## 3. 方案选型

### 3.1 路线对比

| 路线 | 做法 | 评价 |
|---|---|---|
| A. fork 后单实例 + 多 tab 导航 | 原样使用，靠 dbx 多 tab，每 tab 显示不同模型 | ❌ 不成立（F3/F4），只有一个 WebView2 |
| B. fork 后后端改多实例 | `Mutex<Option<LiveSession>>` → `HashMap<sessionId, LiveSession>`，事件带 ID | ✅ 采用。改动集中在数据结构 + 事件路由，引擎复用 |
| C. 完全自研 wry 子窗口插件 | 在插件 sidecar 里重新集成 wry | ❌ 重复造轮子；dbx 未向插件暴露 wry 子窗口 API，可行性未知 |

### 3.2 分屏的两层含义（需明确）

- **dbx 原生分屏**：多个 plugin-workbench tab，用 dbx 的 `split right/down` 并排。优点：零改插件 UI；缺点：每个 tab 是独立 iframe，需要在各 tab 间同步会话列表。
- **插件内分屏**：一个 workbench tab 内画多个 pane，每个 pane 对应一个 `LiveSession`。优点：统一管理、群发 easiest；缺点：UI 改造量大（帧画布、输入命中、布局都要自己写）。

**建议**：第一期用 dbx 原生分屏（每个模型一个 workbench tab），插件内先做单 pane；第二期若需要"同屏对比"再引入插件内分屏。这符合简洁优先。

## 4. 改造清单（按文件/函数/行号）

基准：`com.dbx.webview` v0.7.10 源码。

### 阶段 0：PoC 验证（先做，失败即止损）

**目标**：把单例改成两个实例，验证两个 WebView2 能否同时推帧、互不串扰。

改动：

1. `backend/src/main.rs:41-56`
   `live: Mutex<Option<live::LiveSession>>` → `live: Mutex<HashMap<String, live::LiveSession>>`。
2. `backend/src/main.rs:438-483` `live_open`
   增加 `sessionId` 参数；按 ID 查/建会话；新会话的 profile 改为 `data_dir()/live-browser/<sessionId>`（避免多实例抢同一 profile 锁）。
3. `backend/src/live.rs:196` 帧事件
   `emitter.binary("webview/liveFrame", &frame)` → 帧前 4 字节写 sessionId（或事件名带 ID）。**注意**：二进制通道是共享 broadcast，必须带 ID 才能路由。
4. `backend/src/main.rs:486-527` `live_command`/`live_input`
   所有命令增加 `sessionId`，从 map 取对应 session。涉及的 RPC：`liveNavigate/liveBack/liveForward/liveReload/livePause/liveResume/liveInput/liveQuality/liveResize/liveEval/liveStatus/liveClose`。
5. `ui/app.js:853-867` `liveState`
   由单对象改为 `liveStates: Map<sessionId, state>`；`applyLiveFrame` 按帧里的 ID 分派到对应 `<canvas>`。

**验证步骤**：

- 起两个会话（sessionId = a/b），分别打开 `https://www.example.com` 与 `https://example.org`。
- 断言：两个 canvas 持续出帧、内容不同；对 a 的鼠标输入不影响 b；关闭 a 后 b 继续正常。
- 通过后再进入阶段 1。若两个 WebView2 同进程冲突（COM/STA/窗口消息泵），需评估改为独立 sidecar 进程（见 §6 风险）。

### 阶段 1：模型配置管理

**目标**：自定义增删模型，配置持久化。

改动：

1. `backend/src/main.rs:1221-1230` `Store`
   新增 `models: Vec<LlmModel>`，字段 `id/name/url/enabled/createdAt`。`#[serde(default)]` 保证老配置兼容。
2. `backend/src/main.rs:415-435` `save_settings`
   增加 `models` 的读写分支（复用现有 `persist()`，落 `web-reader-settings.json`）。
3. `backend/src/main.rs` 命令分发新增：`models/list`、`models/upsert`、`models/remove`。
4. `ui/app.js` 新增"模型管理"面板：列表、新增（名称 + URL）、删除、启用开关。URL 仅校验 http/https 前缀（与 t_all 一致）。
5. `manifest.json` 无需新增权限（用现有 `host.storage`）。

**验证**：新增 3 个模型 → 重启插件 → 列表仍在；删除 1 个 → 重启 → 不再出现。

### 阶段 2：免登录 + 分屏访问

**目标**：每个模型独立 profile 登录态持久；并排可见。

改动：

1. 复用阶段 0 的多实例 + 每会话独立 profile（已在 `live_open` 完成）。
2. `ui/app.js:6329-6410` `startLive` 改为接收 `sessionId` + `modelId`；每个模型 tab 一个 canvas。
3. 分屏：先用 dbx 原生能力——模型列表项提供"在新标签打开"按钮，走 `host.openWorkbench`（参考 manifest 现有 `command` 贡献点写法，`forceNew: true` 开多个实例）。
4. 登录态验证：在模型 A 的 WebView2 里登录 → 关闭 tab → 重开 → 仍登录。

**验证**：两个模型同时在线且各自登录态独立；刷新 dbx 后登录态保留。

### 阶段 3：一键批量群发

**目标**：选中多个模型，一次发送同一段提示词，回收回答。

改动：

1. 复用现有 `webview/liveEval`（F7），注入脚本按站点 selector 定位输入框并填入/点击发送。
2. 新增 `backend/src/main.rs` 命令 `batch/send`：
   - 入参：`{ text, sessionIds[] }`。
   - 串行对每个 session 发 `Command::Eval`（多实例并行发送会触发站点风控，且 `liveEval` 的 reply channel 是同步等待，串行更安全）。
   - 每个站点一段 JS 模板（selector 表）：
     - ChatGPT (`chatgpt.com`)：`textarea[data-id="root-composer-input"]` + 发送按钮
     - Claude (`claude.ai`)：`div[contenteditable="true"]` + 发送按钮
     - 豆包 / Kimi / DeepSeek / 元宝 / 通义：各自 selector
     - 注入方式参考 t_all `batch_prompt.rs` 的 `setNativeValue` + `execCommand('insertText')` + 派发 input/click 事件（**仅借鉴思路，不复制代码**——t_all 有补充协议限制）。
   - 返回每个 session 的发送结果（成功/失败/超时）。
3. `ui/app.js` 新增群发栏：多选模型 + 文本框 + 发送按钮 + 结果状态列表。
4. 回答回收：不做自动抓取（站点 DOM 不稳定），用户在各 pane 里直接看。

**验证**：选中 3 个已登录模型，群发"你好" → 3 个 pane 均出现输入并发送成功；未登录的模型返回明确错误。

**取舍说明**：群发脚本依赖站点 DOM selector，站点改版即失效。这是该类产品的固有脆弱性（t_all 亦然），需建可维护的 selector 配置文件，而非硬编码。

### 阶段 4：文件上传下载（最难，可延后）

**目标**：网页内 `<input type="file">` 可选本地文件上传；下载落到本地。

改动（**不需要改 dbx 主程序**——已核实 dbx Host API 提供完整文件能力）：

1. `manifest.json` **无需**增加 `host.filesystem` 权限：核实 `pluginHostBridge.ts:979-984`，`host.pickFiles` 与 `host.saveFile` 明确标注 "no manifest permission gate"（字节只在用户于原生对话框选定后才流转）。只有 `host.openFilesystem`（导航文件系统面板）才要求 `host.filesystem` 权限，本方案用不到。
2. `backend/src/live.rs` `configure()`（约 338 行）增加 CDP：
   - 下载：`Browser.setDownloadBehavior` 或 `Page.setDownloadBehavior`，behavior = `allowAndName`，downloadPath = sidecar 可写目录。
   - 上传：CDP `DOM.setFileInputFiles`。需要先 `DOM.getDocument` + `DOM.querySelector` 定位 input；或拦截 `Page.fileChooserOpened` 事件。
3. 文件选择对话框：**复用 dbx 现成 Host API，无需自研、无需改宿主**：
   - `host.pickFiles`（`pluginHostBridge.ts:302-303`）——原生打开文件对话框，返回只读句柄 `PluginFileHandleMeta[]`；
   - `host.readFileChunk`（:304-305, :987-990）——按 offset 分块读字节，配合 `host.binary` 通道把字节送进 iframe；
   - `host.saveFile` / `host.downloadFile`（:276-277, :923-934）——原生保存对话框 + 落盘。
   - 这四组 API 在 dbx 桌面端已实现并已接进插件桥接层，插件直接调用即可。
4. UI 增加上传按钮/下载列表。

**验证**：在任一模型网页上传一张图片并发送；模型返回的文件可下载到本地并打开。

**风险（已缓解）**：文件 API 已确认存在且免权限门控，原先"API 缺失 → 降级/放弃"的风险解除。剩余风险只在 WebView2 CDP 侧：screencast 模式下的 `fileChooserOpened` 拦截与 `setFileInputFiles` 句柄传递需实测，属插件内部调试，不涉及 dbx。

## 5. 交付物与目录

建议独立仓库（不污染 dbx 主仓库）：

```
dbx-plugin-llm-web/
├── manifest.json              # id: com.example.llm-web，fork 自 com.dbx.webview
├── backend/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs            # RPC 分发、Store、多实例 map
│       ├── live.rs            # WebView2/CDP 引擎（基本复用）
│       ├── models.rs          # 新增：模型配置 CRUD
│       └── batch.rs           # 新增：群发脚本 selector 表 + 注入
├── ui/
│   ├── index.html
│   ├── app.js                 # fork 自原插件，改造帧路由/模型面板/群发栏
│   └── style.css
└── LICENSE                    # MIT，保留原作者版权声明
```

**合规**：MIT 允许修改和再分发，但须保留原版权声明（LICENSE）。新代码可用自己的版权行。

## 6. 风险与取舍

| 风险 | 影响 | 应对 |
|---|---|---|
| 两个 WebView2 同进程 COM/STA 冲突 | 阶段 0 失败，多实例不成立 | 插件内先试多线程 STA；若仍冲突，再评估多 sidecar 进程（**这是唯一可能触及 dbx 主程序的点**：当前 `sessions` 按 plugin_id 单例，需扩展为多实例——超出插件范围，须单独评估） |
| 帧流带宽/CPU | 多实例各 16ms 帧，可能卡顿 | 非活动 pane 调 `livePause`；降低 quality；限制并发数（建议 ≤4） |
| 站点改版导致群发 selector 失效 | 群发功能间歇失效 | selector 外置配置，用户可自定义；提供失败提示 |
| 站点风控/验证码 | 登录、群发被拦 | 依赖真 WebView2 与用户手动验证，插件不做绕过 |
| `host.filesystem` 文件选择 API 缺失 | 阶段 4 阻塞 | ~~降级为路径输入~~ 已核实 dbx 提供 `host.pickFiles`/`readFileChunk`/`saveFile`/`downloadFile`，风险解除 |
| 官方商店签名 | 无法一键安装 | 本地安装 + 开启"允许未签名开发包" |

## 7. 建议的执行顺序

```
阶段 0 PoC（多实例双帧验证）
   └─ 验证通过 → 阶段 1（模型管理）
        └─ 阶段 2（免登录 + 分屏）
             └─ 阶段 3（群发）
                  └─ 阶段 4（文件传输，视 host.filesystem 能力）
```

每一阶段的验证标准见对应章节；**阶段 0 不通过则整体方案需重新设计**，不应继续往下做。
