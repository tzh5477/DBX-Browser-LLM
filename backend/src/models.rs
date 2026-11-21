//! 官网大模型配置与访问记录。
//!
//! 这一层只负责纯数据：结构体、种子、URL 校验、ID 生成、CRUD 辅助函数。
//! 持久化由 main.rs 的 Store 统一落盘（web-reader-settings.json），
//! 这里不碰文件系统，方便单测。

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 访问记录上限，超出丢最旧的一条（避免 JSON 无限增长）。
pub const MAX_VISITS: usize = 500;

/// 一个"官网大模型"的配置。
///
/// `id` 同时用作实时会话的 sessionId 与 profile 目录名，必须稳定且文件安全。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LlmModel {
    pub id: String,
    pub name: String,
    pub url: String,
    pub enabled: bool,
    pub created_at: i64,
}

impl Default for LlmModel {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            url: String::new(),
            enabled: true,
            created_at: 0,
        }
    }
}

/// 一次模型访问记录（unix millis）。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VisitRecord {
    pub model_id: String,
    pub at: i64,
}

impl Default for VisitRecord {
    fn default() -> Self {
        Self {
            model_id: String::new(),
            at: 0,
        }
    }
}

/// 内置种子模型：首次启动写入，之后完全由用户管理。
///
/// URL 只是默认值，不做"官方认证"校验（与 t_all 一致，只校验 scheme）。
pub fn seed_models() -> Vec<LlmModel> {
    let mut now = now_millis();
    SEEDS
        .iter()
        .map(|(name, url)| {
            let model = LlmModel {
                id: new_id(),
                name: (*name).to_string(),
                url: (*url).to_string(),
                enabled: true,
                // 让种子模型有稳定递增的时间戳，列表顺序与这里一致。
                created_at: {
                    now += 1;
                    now
                },
            };
            model
        })
        .collect()
}

const SEEDS: &[(&str, &str)] = &[
    ("ChatGPT", "https://chatgpt.com/"),
    ("Claude", "https://claude.ai/"),
    ("豆包", "https://www.doubao.com/chat/"),
    ("Kimi", "https://kimi.moonshot.cn/"),
    ("DeepSeek", "https://chat.deepseek.com/"),
    ("腾讯元宝", "https://yuanbao.tencent.com/chat"),
    ("通义千问", "https://tongyi.aliyun.com/qianwen/"),
];

/// 时间戳 + 进程内自增序号，保证同一次运行里 IDs 不重复。
/// 只用文件安全字符（字母/数字/-），可直接当 profile 目录名。
static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn new_id() -> String {
    let millis = now_millis();
    let seq = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("m{millis}-{seq:x}")
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// 只接受 http/https，且必须有主机名。
/// 拒绝 javascript:/data:/file: 等，避免把会话导航到危险 scheme。
pub fn validate_url(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("网址不能为空".to_string());
    }
    let parsed = url::Url::parse(trimmed)
        .map_err(|error| format!("网址格式错误：{error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("只支持 http 或 https 网址".to_string());
    }
    if parsed.host_str().unwrap_or_default().is_empty() {
        return Err("网址缺少主机名".to_string());
    }
    Ok(trimmed.to_string())
}

/// 规范化一条来自 UI 的模型：校验必填项、补 ID 与创建时间。
/// `id` 为空视为新增；非空视为编辑（必须已存在，由调用方保证）。
pub fn normalize(mut model: LlmModel) -> Result<LlmModel, String> {
    model.name = model.name.trim().to_string();
    if model.name.is_empty() {
        return Err("模型名称不能为空".to_string());
    }
    if model.name.chars().count() > 80 {
        return Err("模型名称过长（最多 80 字）".to_string());
    }
    model.url = validate_url(&model.url)?;
    if model.id.trim().is_empty() {
        model.id = new_id();
    } else {
        // id 会成为 profile 目录名，只放行文件安全字符，杜绝路径穿越。
        let safe = !model.id.is_empty()
            && model.id.len() <= 80
            && model.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !safe {
            return Err("模型 ID 含非法字符（只允许字母、数字、-、_）".to_string());
        }
    }
    if model.created_at <= 0 {
        model.created_at = now_millis();
    }
    Ok(model)
}

/// 新增或替换一条模型，返回更新后的完整列表顺序。
pub fn upsert(models: &mut Vec<LlmModel>, model: LlmModel) {
    if let Some(slot) = models.iter_mut().find(|item| item.id == model.id) {
        *slot = model;
    } else {
        models.push(model);
    }
}

/// 删除模型，返回是否真的删掉了模型。
/// 调用方需自行清理对应的访问记录（`visits.retain(|v| v.model_id != id)`），
/// 这样可以避免同时可变借用 Store 的两个字段。
pub fn remove(models: &mut Vec<LlmModel>, id: &str) -> bool {
    let before = models.len();
    models.retain(|item| item.id != id);
    models.len() < before
}

/// 追加一条访问记录，超出上限丢最旧。
pub fn record_visit(visits: &mut Vec<VisitRecord>, model_id: String, at: i64) {
    visits.push(VisitRecord { model_id, at });
    if visits.len() > MAX_VISITS {
        let drop = visits.len() - MAX_VISITS;
        visits.drain(0..drop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_only_http_and_https_urls() {
        assert!(validate_url("https://chatgpt.com/").is_ok());
        assert!(validate_url("  http://localhost:8080/chat  ").is_ok());
        assert!(validate_url("javascript:alert(1)").is_err());
        assert!(validate_url("data:text/html,x").is_err());
        assert!(validate_url("file:///C:/x").is_err());
        assert!(validate_url("not a url").is_err());
        assert!(validate_url("").is_err());
    }

    #[test]
    fn normalizes_and_assigns_ids() {
        let model = normalize(LlmModel {
            name: "  ChatGPT  ".to_string(),
            url: " https://chatgpt.com/ ".to_string(),
            ..LlmModel::default()
        })
        .unwrap();
        assert_eq!(model.name, "ChatGPT");
        assert_eq!(model.url, "https://chatgpt.com/");
        assert!(!model.id.is_empty());
        assert!(model.created_at > 0);
    }

    #[test]
    fn rejects_empty_name() {
        assert!(normalize(LlmModel {
            url: "https://x.test/".to_string(),
            ..LlmModel::default()
        })
        .is_err());
    }

    #[test]
    fn upsert_replaces_in_place_and_remove_clears_visits() {
        let mut models = seed_models();
        assert_eq!(models.len(), SEEDS.len());
        let first = models[0].clone();

        let mut edited = first.clone();
        edited.name = "ChatGPT 中文".to_string();
        upsert(&mut models, edited);
        assert_eq!(models.len(), SEEDS.len());
        assert_eq!(models[0].name, "ChatGPT 中文");

        let mut visits = Vec::new();
        record_visit(&mut visits, first.id.clone(), 100);
        assert!(remove(&mut models, &first.id));
        visits.retain(|v| v.model_id != first.id);
        assert_eq!(models.len(), SEEDS.len() - 1);
        assert!(visits.is_empty());
        assert!(!remove(&mut models, &first.id));
    }

    #[test]
    fn visits_are_capped_and_drop_oldest() {
        let mut visits = Vec::new();
        for index in 0..(MAX_VISITS + 10) {
            record_visit(&mut visits, format!("m{index}"), index as i64);
        }
        assert_eq!(visits.len(), MAX_VISITS);
        assert_eq!(visits[0].model_id, format!("m10"));
        assert_eq!(visits.last().unwrap().model_id, format!("m{}", MAX_VISITS + 9));
    }
}
