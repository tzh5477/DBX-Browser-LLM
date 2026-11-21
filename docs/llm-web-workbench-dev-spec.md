# 官网大模型聚合工作台 · 细化开发规格

> 状态：阶段 0–4 全部实施完成并通过自动化验证；真机（DBX 宿主内）验收待用户执行。
> 前置调研：`docs/llm-web-workbench-plugin-plan.md`（已转存到本项目）。
> 基准：`com.dbx.webview` v0.7.10（本仓库）+ dbx 主仓库当前版本。
> 日期：2026-11-21

---

## 0. 本次核实结论摘要

| 项 | 结论 |
|---|---|
| DBX 插件中心是否已有同类插件 | **没有**。官方 catalog 共 **112** 个插件，逐一按 `LLM/AI/chat/gpt/claude/model/webview/browser/分屏/大模型` 检索，无任何"官网大模型聚合/分屏/群发"产品。最接近的是本项目的 `com.dbx.webview`（Browser，单例实时页面）和 `cc-switch`（仅导入 AI provider 配置）。→ 自研成立。 |
| 调研文档的技术事实 | 核心架构判断（单例、共享 sidecar、独立 WebView2 profile、无文件 CDP）**成立**；但 RPC 名称、分屏路线、文件 API 链路、函数定位有 **9 处错误**（见 §2），须按本规格修正后再开发。 |
| 是否可零改动宿主 | **可以**。`host.pickFiles`/`saveFile` 免权限门控（bridge:979-985、1546-1551），`host.workbench` 只需 manifest 加权限。不需要改 dbx 主程序。 |

---

## 1. 核实方法（可复现）

```
curl -sk https://raw.githubusercontent.com/t8y2/dbx-store/main/catalog/index.json \
  | python -c "import json,sys; d=json.load(sys.stdin); print(len(d['plugins']))"
# → 112
```
源码核实对象（均为本机 realpath，行号已逐条复核）：

- 插件本体：`backend/src/main.rs`(1535 行)、`backend/src/live.rs`(1019 行)、`ui/app.js`(8898 行)、`ui/index.html`(592 行)、`manifest.json`(101 行)
- dbx 宿主：`apps/desktop/src/lib/plugins/pluginHostBridge.ts`、`apps/desktop/src/components/plugins/PluginWorkbenchHost.vue`、`crates/dbx-plugin-runtime/src/plugins/host.rs`

---

## 2. 对调研文档的修正（开发前必读）

| # | 调研文档说法 | 源码事实 | 对方案的影响 |
|---|---|---|---|
| C1 | 命令叫 `live_open`/`live_command`/`liveNavigate`/`liveEval`… | 真实 RPC 名全部带前缀：`webview/liveOpen`、`webview/liveNavigate`、`webview/liveInput`、`webview/liveEval`、`webview/liveStatus`、`webview/liveClose` 等 13 个；**不存在** `webview/liveCommand`（`live_command` 只是 Rust 内部函数 main.rs:485） | 改命令分发时按真实名匹配 |
| C2 | 帧事件"帧前 4 字节写 sessionId" | 帧格式固定为 `LF1`(3)+seq(4)+width(2)+height(2)+JPEG（live.rs:166-179）；UI worker 解码强依赖此头（app.js:6747-6748、6781） | 不要在帧头插字节：改用**带 ID 的二进制通道名** `webview/liveFrame:<sid>`（见 §6），帧格式保持不变 |
| C3 | `startLive`（ui/app.js:6329-6410） | 无 `startLive` 函数；实际是 `showLiveView`(6329-6402) + `openLive`(6404-6423) | 改造入口改名 |
| C4 | "先做 dbx 原生分屏：每模型一个 workbench tab，零改插件 UI" | `window.dbxPlugin.openWorkbench(id, ctx, {forceNew:true})` **确实存在**（bridge:1494），但需新增 manifest 权限 `host.workbench`（bridge:825）；且所有同插件 tab **共用一个 sidecar**（host.rs:125，按 plugin_id keyed），sidecar 内仍是**单例 LiveSession**，事件全局广播（host.rs:592-623）→ 多 tab 既拿不到多 WebView2，也会串帧 | **原生分屏不是"零改动"**。要真正多实例，无论哪种分屏都必须先做后端改造。**推荐先做插件内多 pane**（见 §3 D1） |
| C5 | `host.openWorkbench + forceNew` 是 UI 可直接调用的现成 API | 存在但本插件 `app.js` **从未使用**；`manifest.json` 无 `host.workbench` 权限；且 manifest 的 `action` 只支持 `reuse:"singleton"`（manifest.json:49），无 `forceNew` 字段 | 若采用原生分屏，须走 `window.dbxPlugin.openWorkbench` + 加权限，而非 manifest 声明 |
| C6 | 文件上传"配合 host.binary 把字节送进 iframe" | 方向反了。sidecar（main.rs）**零 host API 调用**（`_context: RequestContext` 未使用）；host 文件 API 只能从 **UI iframe** 调（bridge:1588-1607）。字节必须 UI→backend→WebView2 | 新增 `webview/liveUpload` RPC + CDP `DOM.setFileInputFiles`（见 §7 阶段 4） |
| C7 | `host.filesystem` 权限风险已解除 | 部分正确：`pickFiles`/`saveFile`/`readFileChunk` 免权限门控属实；但 WebView2 侧必须新增 `Browser/Page.setDownloadBehavior`、`DOM.setFileInputFiles`、`Page.fileChooserOpened`（live.rs 全文无这些，F8 成立）；另 `Runtime.enable` 也缺失 | 文件功能是实质开发量，不是"接线"；列为阶段 4 |
| C8 | 群发用 `webview/liveEval`（"357-372 / 674-712 行"） | 行号分属两个文件：main.rs:357-372（长度 64KB 限制 + 8s 超时）、live.rs:674-712（CDP `Runtime.evaluate`）。描述需合并 | 群发超时按 8s 设计 |
| C9 | "liveState 改为 Map" 的同时 `applyLiveFrame` 按帧 ID 分派 | 现状是单对象（app.js:851-867）、单 canvas（`#liveCanvas`）、单 `pendingFrame` 槽（app.js:6484-6486）；曾因 worker 再留 pending 导致"只画第一帧"的 bug | 多 pane 时每 pane 独立 state + 独立 canvas + 独立 worker，槽位不共享 |

**调研文档中仍然成立的事实**（不必再核实）：F1/F2 沙箱 iframe + `sandbox="allow-scripts"` + CSP `default-src 'none'`（Host.vue:954、bridge:1213）；F3 sidecar 按 plugin_id 单例（host.rs:125）；F5 每 LiveSession 独立 STA 线程/窗口/WebView2/profile（live.rs:96-125、224-336）；F9 profile 落盘免登录（main.rs:449）。

---

## 3. 需用户确认的决策项（先定再做）

| ID | 决策 | 选项 | 建议 |
|---|---|---|---|
| **D1** | 分屏实现方式 | **A. 插件内多 pane**（一个 workbench tab 内多个 canvas）／ B. DBX 原生多 tab（`openWorkbench(forceNew)` + 宿主分屏） | **选 A**。理由：① 两条路都绕不开后端多实例（C4）；② A 不受 `host.workbench` 权限和宿主 tab 布局限制；③ 群发栏与多 pane 在同一 iframe 内，状态同步最简单；④ 用户要的是"一个界面中使用"，A 更贴合。B 可作为二期叠加。 |
| **D2** | 插件身份 | A. 本仓库原地改造成新插件（改 manifest id/name 为 `com.xingxi.llm-workbench`，保留原 Browser 能力为兼容）／ B. 新建独立仓库 `dbx-plugin-llm-web`，只 fork 必要代码 | **选 A**：本仓库已是 fork 基线且含完整 Cargo 依赖与打包流程；改 ID 后可与原 Browser 共存。若你希望彻底独立，选 B。 |
| **D3** | 登录态隔离粒度 | A. 每模型独立 profile（`live-browser/<modelId>`）／ B. 所有模型共用一个 profile | **选 A**：各站点 Cookie 不互相污染，符合"各自保持独立登录态"；代价是磁盘占用（每个 profile 数十 MB）。 |
| **D4** | 群发是否回收回答 | A. 只群发，回答在各 pane 里看（调研文档默认）／ B. 自动抓取回答并汇总 | **选 A**：站点 DOM 不稳定，抓取易碎；你的原始需求只要求"一键批量群发"。B 可作为三期增强。 |

> 用户确认 D1–D4 后，按 §7 阶段 0 开始；任何一项改选都会改变 §5/§6/§7 的对应部分。

---

## 4. 目标架构

```
┌──────────────────────── DBX 主界面 ────────────────────────┐
│  插件 workbench tab（单个 sandbox iframe，D1=A）            │
│  ┌──────────┬──────────┬──────────┐  ┌──────────────────┐ │
│  │ pane A   │ pane B   │ pane C   │  │ 群发栏            │ │
│  │ canvas A │ canvas B │ canvas C │  │ [模型多选][文本]   │ │
│  │ (ChatGPT)│ (Claude) │ (豆包)   │  │ [发送] 状态列表    │ │
│  └──────────┴──────────┴──────────┘  └──────────────────┘ │
│  ┌────────────── 模型管理面板 ───────────────┐            │
│  │ 名称 / URL / 启用 / 删除 / 访问记录        │            │
│  └───────────────────────────────────────────┘            │
└──────────────────────────┬────────────────────────────────┘
                           │ window.dbxPlugin.invoke / onBinary
                           │ event: webview/liveFrame:<modelId>
┌──────────────────────────▼────────────────────────────────┐
│  插件 sidecar（单进程，Plugin.live = HashMap<modelId,Sess>）│
│  ┌────────────┐ ┌────────────┐ ┌────────────┐            │
│  │ LiveSession│ │ LiveSession│ │ LiveSession│  ...        │
│  │ STA thread │ │ STA thread │ │ STA thread │            │
│  │ WebView2   │ │ WebView2   │ │ WebView2   │            │
│  │ profile/A  │ │ profile/B  │ │ profile/C  │            │
│  └────────────┘ └────────────┘ └────────────┘            │
│  Store: models[] + visits[] + web-reader-settings.json      │
└────────────────────────────────────────────────────────────┘
```

关键不变量：

1. **一会话一 modelId**：`sessionId == model.id`，省去映射表。
2. **一会话一 profile**：`data_dir()/live-browser/<modelId>`，登录态独立持久。
3. **一会话一 STA 线程 + 窗口 + WebView2**：完全继承现有 `run()`（live.rs:224-336），不改引擎内核。
4. **帧按通道名路由**：不改帧二进制格式，只把事件通道从 `webview/liveFrame` 改为 `webview/liveFrame:<modelId>`。
5. **非活动 pane 暂停**：调用 `webview/livePause` 停 screencast，控制 CPU/带宽（并发上限建议 ≤4）。

---

## 5. 数据结构定义

### 5.1 后端 Rust（`backend/src/main.rs` 的 `Store` 附近，约 1221 行）

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct LlmModel {
    id: String,          // 稳定 ID（nanoid/时间戳），同时作为 sessionId 与 profile 目录名
    name: String,        // 显示名（ChatGPT / Claude / 豆包 / Kimi / DeepSeek / 元宝 / 通义…）
    url: String,         // 对话页 URL，仅校验 http/https 前缀
    enabled: bool,
    created_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct VisitRecord {
    model_id: String,
    at: i64,             // unix millis
}

// 在现有 Store（main.rs:1221-1230）中追加两个字段：
//   #[serde(default)] models: Vec<LlmModel>,
//   #[serde(default)] visits: Vec<VisitRecord>,   // 上限 500，超出丢最旧
```

### 5.2 前端状态（`ui/app.js`，替换 `liveState` 单对象 851-867）

```js
state.models    = [];   // 从 webview/modelsList 载入
state.visits    = [];
state.liveStates = {};  // modelId -> { active, canvas, worker, lastSeq, pendingFrame, ... }
state.panes     = [];   // [{ modelId, el, canvas }]，与 liveStates 一一对应
```

### 5.3 内置模型种子（首次启动写入，之后由用户全权管理）

| 名称 | URL |
|---|---|
| ChatGPT | `https://chatgpt.com/` |
| Claude | `https://claude.ai/` |
| 豆包 | `https://www.doubao.com/chat/` |
| Kimi | `https://kimi.moonshot.cn/` |
| DeepSeek | `https://chat.deepseek.com/` |
| 腾讯元宝 | `https://yuanbao.tencent.com/chat` |
| 通义千问 | `https://tongyi.aliyun.com/qianwen/` |

> URL 为示例默认值，用户可增删改；不做"官方认证"校验，与 t_all 一致只校验 scheme。

---

## 6. RPC 协议（全部经 `window.dbxPlugin.invoke`，见 app.js:1064）

### 6.1 模型与记录管理（新增）

| 方法 | 参数 | 返回 |
|---|---|---|
| `webview/modelsList` | `{}` | `{models: LlmModel[], visits: VisitRecord[]}` |
| `webview/modelUpsert` | `{model: LlmModel}` | `{ok:true, model}` |
| `webview/modelRemove` | `{id}` | `{ok:true}`（同时 close 对应 session） |
| `webview/visitRecord` | `{modelId}` | `{ok:true}`（写一条访问记录） |

### 6.2 实时会话（现有命令全部加 `sessionId`）

| 方法 | 新增/变更参数 | 说明 |
|---|---|---|
| `webview/liveOpen` | `{sessionId, url, viewportWidth?, viewportHeight?, deviceScaleFactor?}` | 不存在则创建：profile=`live-browser/<sid>`；已存在则复用并导航 |
| `webview/liveNavigate` | `{sessionId, url}` | |
| `webview/liveBack`/`liveForward`/`liveReload`/`livePause`/`liveResume` | `{sessionId}` | |
| `webview/liveInput` | `{sessionId, event}` | 现有校验逻辑不变（main.rs:500-527） |
| `webview/liveQuality` | `{sessionId, quality}` | |
| `webview/liveResize` | `{sessionId, viewportWidth, viewportHeight, deviceScaleFactor}` | |
| `webview/liveEval` | `{sessionId, expression}` | 保留 64KB / 8s 限制（main.rs:357-372） |
| `webview/liveStatus` | `{sessionId?}` | 不传则返回全部 |
| `webview/liveClose` | `{sessionId?}` | 不传关闭全部 |
| `webview/liveList` | `{}` | 新增：返回活跃 session 概览 |
| `webview/liveUpload` | `{sessionId, fileName, mimeType, offset, chunkB64, final}` | 新增：分块落盘 + 末块 `DOM.setFileInputFiles` |
| `webview/liveDownloads` | `{sessionId?}` | 新增：列出下载目录文件 |
| `webview/liveDownloadRead` | `{sessionId, fileName}` | 新增：返回 base64（分块由 UI 控制） |
| `webview/batchSend` | `{text, sessionIds[]}` | 新增：串行 eval，返回每会话 `{ok,error?}` |

### 6.3 事件（backend → UI）

| 通道/方法 | 载荷 | 变更 |
|---|---|---|
| binary `webview/liveFrame:<sessionId>` | 帧字节（格式不变） | 通道名带 ID；UI `onLiveBinary` 改前缀匹配（app.js:6772-6789） |
| event `webview/liveState` | `{sessionId, url, title, canGoBack, canGoForward, screencast, viewport, capture}` | 载荷加 `sessionId` |
| event `webview/liveError` | `{sessionId, message}` | 载荷加 `sessionId` |
| event `webview/liveDownloaded` | `{sessionId, fileName, size}` | 新增 |

---

## 7. 逐阶段实施与验证（每阶段有硬验证，不通过不进下一阶段）

### 阶段 0：PoC 多实例双帧验证（**止损门**）

**目标**：把单例改成两个实例，证明两个 WebView2 同时出帧、互不串扰。

改动（精确位置）：
1. `backend/src/main.rs:41-47`：`live: Mutex<Option<live::LiveSession>>` → `live: Mutex<HashMap<String, live::LiveSession>>`
2. `backend/src/main.rs:437-483` `live_open`：接收 `sessionId`；`entry(sid).or_insert_with(|| LiveSession::start(data_dir()/live-browser/sid))`
3. `backend/src/live.rs:181-202` `FrameSink::new`：增加 `session_id` 参数，binary 通道改为 `format!("webview/liveFrame:{}", sid)`
4. `backend/src/live.rs:873-888` `emit_state`/`emit_error`：payload 注入 `sessionId`
5. `backend/src/main.rs:312-390`（13 个 live RPC 分支）与 `485-527`（`live_command`/`live_input`）：全部加 `sessionId` 并从 map 取 session
6. `ui/app.js:6772-6789` `onLiveBinary`：解析通道里的 sid，按 sid 分派帧

**已实施并验证（2026-11-21，GATE PASS）**：

为避免在止损门验证前改动生产 RPC，PoC 采用"复用真实引擎、旁路验证"的方式：
- `backend/src/live.rs`：新增 `LiveSession::start_with_channel(profile, Option<String>)`，`run`/`FrameSink` 接受可选帧通道名；默认 `None` → `webview/liveFrame`，生产路径 `start()` 行为**零变化**（已 `cargo check` 验证）。
- `backend/examples/multi_session_poc.rs`（新）：example target，通过 `#[path] mod live` 直接编译生产 `live.rs`，实现一个最小 `PluginHandler`（`poc/open` / `poc/input` / `poc/eval` / `poc/close`），用两个独立 profile + 两个帧通道 `webview/liveFrame:a` / `:b` 启动两个真实会话。
- 外部 harness（Python，临时脚本，非交付物）经 stdio-framed 协议驱动，本地 HTTP 提供两个带 `requestAnimationFrame` 动画的页面（静态页在 screencast 下几乎不产帧，必须用动画页测持续推帧）。

**实测结果（8/8 PASS）**：

| 检查 | 结果 |
|---|---|
| 两个会话同时 open | PASS |
| A 持续推帧 | PASS（8s 内 252 帧，seq 递增，LF1+JPEG 有效） |
| B 持续推帧 | PASS（8s 内 67 帧） |
| 帧头/分辨率/JPEG 有效 | PASS（1280×900，FFD8 头） |
| 两个独立 DOM 上下文 | PASS（A=SESSION-A\|640，B=SESSION-B\|212） |
| 对 A 输入不阻塞 B | PASS（B seq 69→111） |
| 关闭 A 后 B 继续 | PASS（B seq 117→198） |

**结论**：两个 WebView2 同进程 STA 并行成立，帧按通道隔离，互不串扰；**不需要**改多 sidecar（不触及 dbx 宿主）。阶段 0 的门已过，按计划进入阶段 1。

> 编译命令（SDK 需注入，CLI 打包时自动完成）：
> `cargo build --example multi_session_poc --config "patch.crates-io.dbx-plugin-sdk.path='<CLI>/sdk-root/plugins/sdk/rust/dbx-plugin-sdk'"`

### 阶段 1：模型配置管理 + 访问记录（**已完成并验证 2026-11-21**）

改动（实际落地）：
1. `backend/src/models.rs`（新）：`LlmModel` / `VisitRecord` 结构体（camelCase + `default`）、7 个种子模型、URL scheme 校验（仅 http/https + 必须有 host）、ID 生成（时间戳+序号，文件安全）、`normalize`/`upsert`/`remove`/`record_visit`（访问记录上限 500 丢最旧）、10 个单测。
2. `main.rs` `Store` 加 `models: Vec<LlmModel>` / `visits: Vec<VisitRecord>` / `models_seeded: bool`（全部 `#[serde(default)]`，兼容老配置）；`Store::load()` 在 `models_seeded=false` 时写入种子并持久化——**用户删空后不会在下次启动被灌回**。
3. `main.rs` `handle()` 新增 4 个 RPC：`webview/modelsList` / `modelUpsert` / `modelRemove` / `visitRecord`；各自走 `Store` 加锁 + `persist()`（落 `web-reader-settings.json`）。`modelRemove` 同时清理该模型的访问记录。
4. `ui/index.html` 设置抽屉新增第 4 个 tab `#tabModels` + pane `#paneModels`（新增/编辑表单、`#modelList`、`#modelVisitList`）；`ui/style.css` 新增 `.mdl*` 列表样式。
5. `ui/app.js`：`state.models/visits/modelEditingId`；`el` 缓存新增模型面板元素；`setDrawerTab` 的 `panes` map 加 `models` 并触发 `renderModelPanel()`；新增 `loadModels`/`renderModelPanel`/`renderModelList`/`renderModelVisits`/`startModelEdit`/`saveModelFromForm`/`deleteModel`/`wireModelsPanel`；`start()` 中 `wireModelsPanel()` + `Promise.all` 载入 `loadModels()`；中英 i18n 各 24 键。

**验证结果**：
- `cargo test --bin dbx-plugin-webview`：**30/30 PASS**（含 models.rs 10 项 + Store 往返/老配置兼容 2 项）。
- 浏览器运行时冒烟（注入 mock `window.dbxPlugin`，真实加载 app.js）：**4 个页签、模型 pane 渲染 3 行、2 条访问记录、切页签正常**；CRUD 全流程 **PASS**（新增 3→4 并持久化、编辑预填、改名持久化、删除后 3 行且记录清除）。
- `node --check ui/app.js` 通过；JS `$(id)` 引用 218 处全部命中 HTML 的 258 个 id（0 缺失）。

> 真机验收（DBX 宿主内安装未签名包）仍待用户执行：重点看 profile 落盘免登录与后续阶段的分屏/群发/文件。

### 阶段 2：免登录 + 插件内分屏（**已完成并验证 2026-11-21**）

> 实施策略调整：不改承担约 40 处引用的旧单实例 `liveState`（小红书实时页继续用它，避免回归），
> 改为**新增一套自包含的工作台子系统**（`wbPanes`/`wbOrder`/`wbActiveId`），复用 15b 的
> `LIVE_WORKER_CODE` 解码器。这比"把 liveState 改成 Map"风险低得多，且两条路径互不干扰。

后端改动（生产路径，阶段 0 PoC 之前一直未落地）：
1. `live.rs`：新增 `start_session(profile, sid)`——帧通道命名 `webview/liveFrame:<sid>`；
   `Context` 加 `session_id`，`emit_state`/`emit_error` 载荷注入 `sessionId`（空则不写，兼容旧单实例）；
   保留 `start()`/`start_with_channel()` 供旧路径与 PoC。
2. `main.rs`：`Plugin.live` 从 `Mutex<Option<LiveSession>>` → `Mutex<HashMap<String, LiveSession>>`；
   13 个 live RPC 全部按 sessionId 路由；新增 `webview/liveList`；`liveOpen`/`liveClose` 支持按 sid；
   `modelRemove` 一并关闭对应会话。
3. **向后兼容**：新增 `live_sid()`——不传 sessionId 的旧调用落 `"default"` 会话，仍用原 profile 目录、
   原帧通道 `webview/liveFrame`、事件不带 sessionId → 升级后小红书等旧实时页登录态与画面不变。

前端改动：
1. `index.html` `#content` 内新增 `#paneHost`（`.wb` 容器 + 顶栏 + `#wbGrid` 栅格 + 空状态）。
2. `app.js` 新增 §15a 工作台子系统：每 pane 独立 `{canvas, worker, pendingFrame, lastSeq, input, viewport}`；
   `wbOpen`(建 pane+写访问记录+liveOpen)、`wbClosePane`、`wbFocus`(活动 resume/其余 pause)、
   `wbOnBinary`(按 `webview/liveFrame:<sid>` 通道分派)、`wbOnState/Error`(按 sessionId)、
   每 pane 独立鼠标/滚轮/键盘/中文输入法转发；模型列表每行加「打开」按钮。
3. `onLiveBinary`/`wireLiveEvents`：带 `sessionId`/`:sid` 通道的事件走工作台，其余仍走旧实时页。
4. `style.css` 新增 `.wb`/`.wb-pane*` 自适应栅格样式。

**验证结果**：
- `cargo test --bin dbx-plugin-webview`：**31/31 PASS**（新增 `session_id_falls_back_to_default` 兼容测试）。
- 浏览器运行时冒烟（注入 mock bridge，真实加载 app.js，mock 按会话通道持续推 JPEG 帧）：
  - 打开 ChatGPT/Claude/豆包 **3 个 pane**，各 1 canvas、各自标题正确、**active=1**；
  - 关闭其中一个 → 剩 2 个，ids 正确；
  - 帧路由验证：**3/3 pane 状态变为「就绪」**（只有帧被解码绘制才会置位）→ 帧按通道正确分派到各画布。
- `node --check` 通过；JS 225 处 `$(id)` 全部命中 266 个 HTML id（0 缺失）。
- 阶段 0 PoC `cargo build --example multi_session_poc` 仍通过（向后兼容未破坏）。

> 真机验收（DBX 宿主内安装未签名包）仍待用户执行：重点看 3 个真实 WebView2 登录态、切 pane 停帧降 CPU、以及"关 pane 重开仍登录 / 刷新 DBX 仍登录"（依赖 sidecar profile 落盘，架构已具备）。

### 阶段 3：一键批量群发（**已完成并验证 2026-11-21**）

改动（实际落地）：
1. `backend/src/batch.rs`（新）：`SelectorRule`/`BatchConfig` 结构、内置默认表（7 站点 + 通配兜底）、域名后缀匹配（最具体优先）、注入脚本生成（prototype setter + `input`/`change` 事件 + `contenteditable` 走 textContent + InputEvent；触发 click 或 Enter）。外置 `selectors.json` 首次运行写入数据目录，之后用户可编辑；文件损坏时回退内置默认且不覆盖用户文件。7 个单测。
2. `main.rs` 新增 `webview/batchSend`：按 `{text, sessionIds[]}` 串行对每个 session 执行注入脚本（Reply channel 同步等待，串行避免超时叠加与风控）；单会话失败不影响其余；返回 `{ok, okCount, total, results[]}`，每项 `{sessionId, ok, error?, via?}`。
3. `ui/index.html` 工作台顶栏加「批量群发」按钮 + 可展开群发栏（模型勾选 chip + 全选/全不选 + 文本框 + 发送 + 逐模型结果）。
4. `ui/app.js`：新增 `batchSelected` 状态、`renderBatchPanel`/`batchSend`/`renderBatchResults` 等；默认勾选已打开的 pane；中英 i18n 各 13 键；`applyCopy` 同步文案。
5. selector 初值（真机需按站点改版校准，可在数据目录 `selectors.json` 里改）：
   ChatGPT `#prompt-textarea`+send-button、Claude `div[contenteditable]`+Send msg、豆包/Kimi/DeepSeek/元宝/通义 `textarea` 或 contenteditable + Enter、以及通配兜底。

**验证结果**：
- `cargo test --bin dbx-plugin-webview`：**38/38 PASS**（batch 模块 7 项新单测：域名匹配/通配兜底/子域匹配/脚本转义/JSON 往返）。
- 浏览器 mock 冒烟（真实加载 app.js + mock bridge）：群发栏展开、列出 4 模型、默认勾选 3 个已开模型、发送后逐模型显示「已发送」+ 汇总「共 3 个，成功 3 个」、`batchSend` RPC 确实被调用 —— **全部 PASS**。

### 阶段 4：图片/文件上传与下载（**已完成并验证 2026-11-21**）

改动（实际落地，**不改 dbx 主程序、manifest 权限不变**）：
1. `backend/src/transfer.rs`（新，跨平台纯逻辑）：文件名/会话 ID 目录穿越防护、分块落盘（offset 连续性校验）、自带 base64 编解码、下载目录列举（按时间倒序、过滤 .crdownload）、下载文件分块读取；8 个单测。
2. `backend/src/live.rs`：`Command` 枚举加 `AttachFile{path,reply}`；`configure()` 新增 `Runtime.enable` + `Browser.setDownloadBehavior`（失败回退 `Page.setDownloadBehavior`），下载目录按会话隔离 `data_dir()/live-downloads/<sid>`；新增 `wire_download_events` 订阅 `Browser/Page.downloadProgress`，完成时发 `webview/liveDownloaded`；`attach_file` 走 CDP `DOM.getDocument` → `DOM.querySelector('input[type=file]')` → `DOM.setFileInputFiles` 链式回调。
3. `main.rs` 新增 3 个 RPC：`webview/liveUpload`（分块写 `<dataDir>/live-uploads/<sid>/<fileName>`，末块触发 AttachFile）、`webview/liveDownloads`、`webview/liveDownloadRead`（返回 `read` 字段供 UI 精确推进 offset）；`liveOpen` 给每个会话传下载目录。
4. UI：每 pane 标题栏加「上传 ⬆ / 下载 ⬇」按钮；上传走 `plugin.fileTransfer.pick/read` 分块送 base64；下载弹层列出文件、逐个用 `webview/liveDownloadRead` 读回后 `plugin.saveFile` 落盘；`webview/liveDownloaded` 事件自动刷新弹层；Escape 关闭弹层。中英 i18n 各 15 键。
5. 已核实宿主 bridge：`fileTransfer.pick/read` 与 `saveFile` 免 manifest 权限门控（pluginHostBridge.ts:979-985、1546-1551、1588-1607）；`plugin.invoke` 只走 `backend.invoke`，文件 API 必须走 `plugin.fileTransfer`/`plugin.saveFile`（本实现已正确区分）。

**验证结果**：
- `cargo test --bin dbx-plugin-webview`：**46/46 PASS**（transfer 8 项新单测：base64 往返/目录穿越/保留字符清洗/会话 ID 安全/目录归属/分块重组/偏移缺口拒绝/下载列举）。
- `cargo build --example multi_session_poc`：PASS（阶段 0 PoC 向后兼容未破坏，签名调整已适配）。
- 浏览器 mock 冒烟：每 pane 上传/下载按钮齐全（3+3）、下载弹层打开并渲染 2 文件、保存链路调用 `liveDownloadRead`、Escape 关闭弹层、上传链路调用 `liveUpload` —— **全部 PASS**。
- `node --check ui/app.js` PASS；JS 245 处 `$(id)` 全部命中 287 个 HTML id（0 缺失）。

> **真机验收（DBX 宿主内安装未签名包）待用户执行**：阶段 3 需在已登录的模型里群发"你好"确认文本进入输入框并发出（selector 可能需按站点改版校准）；阶段 4 需实测真实 WebView2 的 `Browser.setDownloadBehavior` 是否生效、CDP 挂载文件到各站 `input[type=file]` 是否成功（screencast 模式下若 `DOM.querySelector` 不稳，备选监听 `Page.fileChooserOpened` 拿 backendNodeId 注入）。自动化层（后端单测 + 浏览器 mock UI）已全部覆盖，真机仅剩 WebView2 站点特定的选择器/CDP 兼容性校准。

---

## 8. 测试计划

| 层级 | 手段 | 覆盖 |
|---|---|---|
| 后端单测 | `cd backend && cargo test` | Store 序列化兼容（老配置无 models 字段）、model CRUD、URL scheme 校验、selector 表序列化 |
| PoC 集成 | `cargo run --example multi_session_poc` | 阶段 0 双会话双帧 |
| 前端调试 | `dbx-plugin dev --path . --port 5190` | UI 面板渲染、事件分派（无宿主时走 memoryStore 降级） |
| 真机验收 | 本地安装未签名包（插件中心勾选「允许安装未签名开发包」） | 阶段 1-4 全部验证步骤 |

---

## 9. 风险与取舍

| 风险 | 应对 |
|---|---|
| 多 WebView2 同进程 COM/STA 冲突 | 阶段 0 即验证；现有每会话独立 STA 线程 + 窗口，冲突概率低；若失败需改多 sidecar（触及 dbx，须单独评估） |
| 多 pane 推帧占 CPU/带宽 | 非活动 pane `livePause`；quality 降到 60；并发上限 4；每 pane 有界队列（现为 `sync_channel(16)`，live.rs:189） |
| 群发 selector 随站点改版失效 | selector 外置 `selectors.json`，用户可改；失败返回明确错误；不做自动抓取（D4=A） |
| 站点风控/验证码 | 真 WebView2 + 用户手动验证；插件不做绕过 |
| screencast 下 `fileChooserOpened` 拦截不稳 | 优先 `DOM.querySelector + setFileInputFiles`；实测备选方案 |
| 官方商店签名 | 本地安装未签名包；不进官方商店（调研文档 §1.2 已排除） |
| 隐私：Cookie 落盘 | 每模型独立 profile 存于插件数据目录；README 如实披露；卸载清理 |

---

## 10. 交付物目录（D2=A 时）

```
DBX-Browser-LLM/
├── docs/llm-web-workbench-plugin-plan.md   # 已转存的调研文档
├── docs/llm-web-workbench-dev-spec.md       # 本规格
├── manifest.json                            # 改 id/name/描述，permissions 视 D1 决定是否加 host.workbench
├── backend/
│   ├── examples/multi_session_poc.rs        # 新增：阶段 0 验证
│   └── src/
│       ├── main.rs                          # 多实例 map、RPC 路由、Store 扩展
│       ├── live.rs                          # 通道带 ID、CDP 文件能力（基本复用引擎）
│       ├── models.rs                        # 新增：模型配置 CRUD + 种子
│       ├── batch.rs                         # 新增：群发 selector + 注入
│       └── selectors.json                   # 新增：可编辑 selector 表
└── ui/
    ├── index.html                           # 模型面板 + 多 pane 容器 + 群发栏
    ├── app.js                               # 帧路由/多 pane/群发/模型管理
    └── style.css                            # Pane 栅格与面板样式
```

---

## 11. 状态与下一步

阶段 0–4 均已实施并通过自动化验证（后端单测 46/46 + 浏览器 mock 冒烟 22/22 + JS 语法/DOM 引用校验 + PoC 构建）。D1–D4 按建议值执行：插件内多 pane / 原地改造本仓库 / 每模型独立 profile / 群发不回收回答。

**下一步（需用户在真机完成）**：
1. 用 `dbx-plugin` CLI 打包未签名开发包，在 DBX 插件中心勾选「允许安装未签名开发包」安装。
2. 打开 3 个模型登录态验证多 pane、切 pane 停帧、刷新 DBX 后免登录。
3. 逐个校准 `selectors.json`（群发）与真实下载/CDP 文件挂载（阶段 4）。
4. 真机全部通过后，再考虑 D2 的独立仓库拆分 / 原生多 tab 分屏（B 方案）作为二期。
