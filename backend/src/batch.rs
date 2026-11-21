//! 一键批量群发（阶段 3）。
//!
//! 职责分两层：
//! 1. 站点选择器表（域名 → 输入框 selector + 发送 selector + 触发方式），外置为
//!    `selectors.json` 供用户按站点改版自行校准；缺失或损坏时回退到内置默认表。
//! 2. 注入脚本生成：用 `Runtime.evaluate` 把文本写进各大模型输入框并触发发送。
//!    用 prototype 的原生 setter 写值 + 派发 input/change 事件，兼容 React/Vue 等
//!    受控组件；对 contenteditable 走 textContent + InputEvent。
//!
//! 本模块是纯逻辑（不依赖 WebView2），可在任意平台单测。真正的 RPC 编排在 main.rs。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use url::Url;

/// 单个站点的注入规则。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SelectorRule {
    /// 稳定标识（仅用于展示/编辑）。
    pub id: String,
    /// 显示名。
    pub name: String,
    /// 匹配的域名后缀列表；`"*"` 表示兜底通配。
    pub hosts: Vec<String>,
    /// 输入框 CSS selector；为空时脚本按常见输入框自动探测。
    pub input_selector: String,
    /// 发送按钮 CSS selector；trigger=enter 时可为空。
    pub send_selector: String,
    /// 触发方式：`"click"` 点击发送按钮，或 `"enter"` 派发回车。
    pub trigger: String,
}

impl Default for SelectorRule {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            hosts: Vec::new(),
            input_selector: String::new(),
            send_selector: String::new(),
            trigger: "enter".to_string(),
        }
    }
}

/// 完整的群发配置（selectors.json 的顶层结构）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BatchConfig {
    pub selectors: Vec<SelectorRule>,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            selectors: built_in_selectors(),
        }
    }
}

/// 内置 selector 表（首次运行写入 selectors.json，之后由用户全权维护）。
///
/// 这些都是"起始值"，站点改版后需在设置里改 selectors.json；与 t_all 一样
/// 不做自动抓取、不做绕过风控。
pub fn built_in_selectors() -> Vec<SelectorRule> {
    vec![
        SelectorRule {
            id: "chatgpt".into(),
            name: "ChatGPT".into(),
            hosts: vec!["chatgpt.com".into(), "chat.openai.com".into()],
            input_selector: "#prompt-textarea".into(),
            send_selector: "button[data-testid='send-button']".into(),
            trigger: "click".into(),
        },
        SelectorRule {
            id: "claude".into(),
            name: "Claude".into(),
            hosts: vec!["claude.ai".into()],
            input_selector: "div[contenteditable='true']".into(),
            send_selector: "button[aria-label='Send message']".into(),
            trigger: "click".into(),
        },
        SelectorRule {
            id: "doubao".into(),
            name: "豆包".into(),
            hosts: vec!["doubao.com".into()],
            input_selector: "textarea".into(),
            send_selector: String::new(),
            trigger: "enter".into(),
        },
        SelectorRule {
            id: "kimi".into(),
            name: "Kimi".into(),
            hosts: vec!["moonshot.cn".into(), "kimi.com".into()],
            input_selector: "textarea".into(),
            send_selector: String::new(),
            trigger: "enter".into(),
        },
        SelectorRule {
            id: "deepseek".into(),
            name: "DeepSeek".into(),
            hosts: vec!["deepseek.com".into()],
            input_selector: "textarea".into(),
            send_selector: String::new(),
            trigger: "enter".into(),
        },
        SelectorRule {
            id: "yuanbao".into(),
            name: "腾讯元宝".into(),
            hosts: vec!["yuanbao.tencent.com".into()],
            input_selector: "div[contenteditable='true']".into(),
            send_selector: String::new(),
            trigger: "enter".into(),
        },
        SelectorRule {
            id: "tongyi".into(),
            name: "通义千问".into(),
            hosts: vec!["aliyun.com".into()],
            input_selector: "textarea".into(),
            send_selector: String::new(),
            trigger: "enter".into(),
        },
        SelectorRule {
            id: "generic".into(),
            name: "通用兜底".into(),
            hosts: vec!["*".into()],
            input_selector: String::new(),
            send_selector: String::new(),
            trigger: "enter".into(),
        },
    ]
}

/// selectors.json 的路径。
pub fn config_path(data_dir: &Path) -> PathBuf {
    data_dir.join("selectors.json")
}

/// 读取群发配置：文件不存在则写入内置默认表并返回；存在但解析失败也回退默认，
/// 同时把损坏内容留给用户（不覆盖），避免吞掉他的手工修改。
pub fn load_config(data_dir: &Path) -> BatchConfig {
    let path = config_path(data_dir);
    match std::fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<BatchConfig>(&raw) {
            Ok(config) if !config.selectors.is_empty() => config,
            Ok(_) => BatchConfig::default(),
            Err(error) => {
                eprintln!("[batch] selectors.json 解析失败：{error}，使用内置默认表");
                BatchConfig::default()
            }
        },
        Err(_) => {
            let config = BatchConfig::default();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(pretty) = serde_json::to_string_pretty(&config) {
                if let Err(error) = std::fs::write(&path, pretty) {
                    eprintln!("[batch] 写入默认 selectors.json 失败：{error}");
                }
            }
            config
        }
    }
}

/// 取 URL 的 host（小写）；解析失败返回空串。
pub fn host_of(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_string))
        .unwrap_or_default()
        .to_lowercase()
}

fn host_matches(host: &str, suffix: &str) -> bool {
    let suffix = suffix.trim().trim_start_matches('.').to_lowercase();
    if suffix.is_empty() {
        return false;
    }
    if suffix == "*" {
        return true;
    }
    host == suffix || host.ends_with(&format!(".{suffix}"))
}

/// 按 URL 匹配规则：先找最具体的域名规则（非通配），再退回通配兜底。
pub fn select_for_url<'a>(config: &'a BatchConfig, url: &str) -> Option<&'a SelectorRule> {
    let host = host_of(url);
    if host.is_empty() {
        return config.selectors.iter().find(|rule| {
            rule.hosts.iter().any(|h| h.trim() == "*")
        });
    }
    // 优先非通配、后缀最长的规则，避免 aliyun.com 抢在 yuanbao.tencent.com 前面。
    let mut best: Option<(&SelectorRule, usize)> = None;
    for rule in &config.selectors {
        for h in &rule.hosts {
            if h.trim() == "*" {
                continue;
            }
            if host_matches(&host, h) {
                let specificity = h.trim().len();
                if best.map(|(_, n)| specificity > n).unwrap_or(true) {
                    best = Some((rule, specificity));
                }
            }
        }
    }
    if let Some((rule, _)) = best {
        return Some(rule);
    }
    config
        .selectors
        .iter()
        .find(|rule| rule.hosts.iter().any(|h| h.trim() == "*"))
}

/// 通用兜底规则：域名未命中任何配置时使用。
pub fn generic_rule() -> SelectorRule {
    built_in_selectors()
        .into_iter()
        .find(|rule| rule.id == "generic")
        .unwrap_or_default()
}

/// 把规则里的字符串安全编码成 JS 字符串字面量（含引号）。
fn js_str(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

/// 生成注入脚本：写值 + 触发发送，返回 JSON 状态串。
///
/// 脚本刻意自包含、无外部依赖，且用 try/catch 包住每一步，任何失败都返回结构化
/// 错误，便于 UI 逐模型展示"成功/失败/原因"。
pub fn build_script(text: &str, rule: &SelectorRule) -> String {
    let t = js_str(text);
    let input = js_str(&rule.input_selector);
    let send = js_str(&rule.send_selector);
    let trigger = js_str(&rule.trigger.trim().to_lowercase());

    format!(
        r#"(function(){{
  var t={t}, inputSel={input}, sendSel={send}, trigger={trigger};
  function one(sel){{ if(!sel) return null; try{{ return document.querySelector(sel); }}catch(e){{ return null; }} }}
  function ret(ok, extra){{
    var o = {{ ok: ok }};
    if (extra) for (var k in extra) o[k] = extra[k];
    return JSON.stringify(o);
  }}
  var input = null;
  var candidates = inputSel ? [inputSel] : ['textarea', 'div[contenteditable="true"]', 'input[type="text"]'];
  for (var i=0;i<candidates.length;i++){{ input = one(candidates[i]); if (input) break; }}
  if (!input) return ret(false, {{ error: 'input-not-found' }});
  var editable = input.isContentEditable || (input.getAttribute && input.getAttribute('contenteditable') === 'true');
  try {{
    if (editable) {{
      input.focus();
      input.textContent = t;
      input.dispatchEvent(new InputEvent('input', {{ bubbles: true, data: t, inputType: 'insertText' }}));
      input.dispatchEvent(new Event('change', {{ bubbles: true }}));
    }} else {{
      var proto = input.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
      var desc = Object.getOwnPropertyDescriptor(proto, 'value');
      if (desc && desc.set) desc.set.call(input, t); else input.value = t;
      input.dispatchEvent(new Event('input', {{ bubbles: true }}));
      input.dispatchEvent(new Event('change', {{ bubbles: true }}));
    }}
  }} catch(e) {{ return ret(false, {{ error: 'set-value-failed', detail: String(e && e.message || e) }}); }}
  input.focus();
  if (trigger === 'enter') {{
    try {{
      var ky = {{ key: 'Enter', code: 'Enter', keyCode: 13, which: 13, bubbles: true, cancelable: true }};
      input.dispatchEvent(new KeyboardEvent('keydown', ky));
      input.dispatchEvent(new KeyboardEvent('keypress', ky));
    }} catch(e) {{ return ret(false, {{ error: 'enter-failed', detail: String(e && e.message || e) }}); }}
    return ret(true, {{ via: 'enter' }});
  }}
  var btn = one(sendSel);
  if (!btn) return ret(false, {{ error: 'send-button-not-found' }});
  try {{ btn.click(); }} catch(e) {{ return ret(false, {{ error: 'click-failed', detail: String(e && e.message || e) }}); }}
  return ret(true, {{ via: 'click' }});
}})()"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_table_has_all_seed_models_and_fallback() {
        let rules = built_in_selectors();
        assert!(rules.len() >= 8);
        assert!(rules.iter().any(|r| r.id == "chatgpt"));
        assert!(rules.iter().any(|r| r.id == "generic" && r.hosts.iter().any(|h| h == "*")));
    }

    #[test]
    fn host_of_parses_and_lowercases() {
        assert_eq!(host_of("https://chatgpt.com/"), "chatgpt.com");
        assert_eq!(host_of("https://www.DOUBAO.com/chat/"), "www.doubao.com");
        assert_eq!(host_of("not a url"), "");
    }

    #[test]
    fn matches_specific_rule_over_generic() {
        let config = BatchConfig::default();
        let rule = select_for_url(&config, "https://yuanbao.tencent.com/chat").unwrap();
        // 应命中 yuanbao 而不是更宽泛的 aliyun/generic。
        assert_eq!(rule.id, "yuanbao");
        let rule = select_for_url(&config, "https://chatgpt.com/").unwrap();
        assert_eq!(rule.id, "chatgpt");
    }

    #[test]
    fn falls_back_to_generic_for_unknown_host() {
        let config = BatchConfig::default();
        let rule = select_for_url(&config, "https://some-unknown-site.example/x").unwrap();
        assert_eq!(rule.id, "generic");
    }

    #[test]
    fn subdomain_matches_parent_suffix() {
        assert!(host_matches("www.kimi.moonshot.cn", "moonshot.cn"));
        assert!(host_matches("kimi.moonshot.cn", "moonshot.cn"));
        assert!(!host_matches("evilmoonshot.cn", "moonshot.cn"));
    }

    #[test]
    fn script_escapes_quotes_and_newlines_in_text() {
        let rule = SelectorRule {
            input_selector: "textarea".into(),
            trigger: "click".into(),
            ..SelectorRule::default()
        };
        let script = build_script("hello \"world\"\n第二行", &rule);
        assert!(script.contains("hello \\\"world\\\""));
        assert!(script.contains("\\n第二行"));
        assert!(script.contains("JSON.stringify"));
        assert!(script.contains("desc.set.call"));
        assert!(script.contains("btn.click()"));
    }

    #[test]
    fn config_roundtrips_through_json() {
        let config = BatchConfig::default();
        let raw = serde_json::to_string(&config).unwrap();
        let parsed: BatchConfig = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.selectors.len(), config.selectors.len());
        assert_eq!(parsed.selectors[0].id, "chatgpt");
    }
}
