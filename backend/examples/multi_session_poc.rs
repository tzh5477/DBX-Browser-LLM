//! 阶段 0 止损门 PoC：在同一进程内启动两个真实的 LiveSession（各自 STA 线程 +
//! WebView2 + 独立 profile），验证两者能同时推帧、输入互不串扰、关闭其一不影响另一。
//!
//! 该 example 直接复用生产 live.rs 引擎，不改动生产 RPC 逻辑。帧通过带会话后缀的
//! 二进制通道发出（webview/liveFrame:<id>），由外部 harness 按通道归类统计。
//!
//! 运行（SDK 由 CLI 注入）：
//!   cargo run --example multi_session_poc --config "patch.crates-io.dbx-plugin-sdk.path='<SDK>'"
//! 然后用 tools/ 下的 harness 驱动 stdio-framed 协议。

#![cfg(windows)]

#[path = "../src/live.rs"]
mod live;

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;

use dbx_plugin_sdk::{
    PluginEmitter, PluginError, PluginHandler, PluginMetadata, PluginServer, PluginTransport,
    RequestContext,
};
use serde_json::{json, Value};

struct Poc {
    sessions: Mutex<HashMap<String, live::LiveSession>>,
}

impl Poc {
    fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    fn session(&self, _id: &str) -> Result<std::sync::MutexGuard<'_, HashMap<String, live::LiveSession>>, PluginError> {
        self.sessions
            .lock()
            .map_err(|_| PluginError::new(-32000, "会话表锁不可用"))
    }
}

fn req_str<'a>(params: &'a Value, key: &str) -> Result<&'a str, PluginError> {
    params
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| PluginError::new(-32602, format!("缺少参数 {key}")))
}

impl PluginHandler for Poc {
    fn handle(
        &self,
        _context: RequestContext,
        method: &str,
        params: Value,
        emitter: &PluginEmitter,
    ) -> Result<Value, PluginError> {
        match method {
            "poc/open" => {
                let id = req_str(&params, "id")?.to_string();
                let url = req_str(&params, "url")?.to_string();
                let channel = format!("webview/liveFrame:{}", id);
                let profile = std::env::temp_dir()
                    .join("dbx-poc-profiles")
                    .join(&id);
                // 复用真实引擎：独立 profile + 独立帧通道。
                let session = live::LiveSession::start_with_channel(profile, Some(channel))
                    .map_err(|message| PluginError::new(-32000, message))?;
                session
                    .send(live::Command::Open {
                        url,
                        emitter: emitter.clone(),
                    })
                    .map_err(|message| PluginError::new(-32000, message))?;
                self.session(&id)?.insert(id.clone(), session);
                Ok(json!({ "ok": true, "id": id }))
            }
            "poc/input" => {
                let id = req_str(&params, "id")?;
                let event = params.get("event").cloned().unwrap_or(Value::Null);
                let sessions = self.session(id)?;
                let session = sessions
                    .get(id)
                    .ok_or_else(|| PluginError::new(-32000, format!("会话 {id} 不存在")))?;
                session
                    .send(live::Command::Input(event))
                    .map_err(|message| PluginError::new(-32000, message))?;
                Ok(json!({ "ok": true }))
            }
            "poc/eval" => {
                let id = req_str(&params, "id")?.to_string();
                let expression = req_str(&params, "expression")?.to_string();
                let (tx, rx) = mpsc::channel();
                {
                    let sessions = self.session(&id)?;
                    let session = sessions
                        .get(&id)
                        .ok_or_else(|| PluginError::new(-32000, format!("会话 {id} 不存在")))?;
                    session
                        .send(live::Command::Eval {
                            expression,
                            reply: tx,
                        })
                        .map_err(|message| PluginError::new(-32000, message))?;
                }
                match rx.recv_timeout(Duration::from_secs(8)) {
                    Ok(Ok(value)) => Ok(json!({ "ok": true, "value": value })),
                    Ok(Err(message)) => Err(PluginError::new(-32000, message)),
                    Err(_) => Err(PluginError::new(-32000, "eval 超时")),
                }
            }
            "poc/close" => {
                let id = req_str(&params, "id")?.to_string();
                if let Some(session) = self.session(&id)?.remove(&id) {
                    session.close();
                }
                Ok(json!({ "ok": true }))
            }
            "poc/status" => {
                let sessions = self.session("status")?;
                Ok(json!({ "ids": sessions.keys().collect::<Vec<_>>() }))
            }
            _ => Err(PluginError::method_not_found(method)),
        }
    }
}

fn main() -> std::io::Result<()> {
    // 清理上一次运行的 profile，保证每个会话拿到全新 WebView2 环境。
    let _ = std::fs::remove_dir_all(std::env::temp_dir().join("dbx-poc-profiles"));
    let metadata = PluginMetadata::new("com.example.live-poc", env!("CARGO_PKG_VERSION"))
        .with_capability("events");
    PluginServer::new(metadata, Poc::new())
        .transport(PluginTransport::Framed)
        .serve()
}
