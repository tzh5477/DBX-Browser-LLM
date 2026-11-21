//! 文件上传 / 下载的纯逻辑（阶段 4）。
//!
//! 这里只做与平台无关的部分：文件名与会话 ID 的目录穿越防护、上传分块落盘、
//! base64 编解码、下载目录列举。真正的 WebView2/CDP 动作（把本地文件挂到
//! `input[type=file]`、设置下载目录、监听下载完成）在 live.rs。
//!
//! 分成两层的好处：文件安全与分块重组可以跨平台单测，而 COM 代码只能靠
//! 真机验证——把尽可能多的逻辑挪到这一层，能显著提高可验证的覆盖面。

use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

/// 目录穿越/非法文件名清洗：只保留最后一段路径，剔除控制字符与 Windows 保留字符。
/// 返回 None 表示这个名字不安全（例如 `..`、空串、纯点号）。
pub fn safe_file_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    // 同时把 \ 与 / 当分隔符，取最后一段，挡住 ../ 与 ..\ 穿越。
    let base = trimmed.rsplit(|c| c == '/' || c == '\\').next().unwrap_or("");
    let base = base.trim().trim_matches('.');
    if base.is_empty() || base == ".." {
        return None;
    }
    let mut out = String::with_capacity(base.len());
    for ch in base.chars() {
        let bad = ch.is_control()
            || matches!(ch, '<' | '>' | ':' | '"' | '|' | '?' | '*')
            || ch == '/' || ch == '\\';
        if bad {
            out.push('_');
        } else {
            out.push(ch);
        }
    }
    let out = out.trim().to_string();
    if out.is_empty() || out.len() > 240 {
        return None;
    }
    Some(out)
}

/// 会话 ID 作为目录名：只允许文件安全字符，防止 sessionId 逃出 uploads/downloads 根目录。
pub fn safe_session_id(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > 128 {
        return None;
    }
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        let ok = ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_');
        out.push(if ok { ch } else { '_' });
    }
    if out.is_empty() { None } else { Some(out) }
}

/// 上传暂存目录：<dataDir>/live-uploads/<sid>
pub fn upload_dir(data_dir: &Path, sid: &str) -> Option<PathBuf> {
    Some(data_dir.join("live-uploads").join(safe_session_id(sid)?))
}

/// 下载目录：<dataDir>/live-downloads/<sid>
pub fn download_dir(data_dir: &Path, sid: &str) -> Option<PathBuf> {
    Some(data_dir.join("live-downloads").join(safe_session_id(sid)?))
}

/// 把一个 base64 分块写到目标文件的 offset 处，返回写后文件总长度。
///
/// 用读+写+创建模式打开并 seek 到 offset：既支持顺序分块，也允许丢包重传后覆盖，
/// 同时拒绝 offset 超过当前文件长度（防止在文件里留下空洞或错乱数据）。
pub fn write_chunk(dir: &Path, file_name: &str, offset: u64, bytes: &[u8]) -> Result<u64, String> {
    let name = safe_file_name(file_name).ok_or_else(|| "文件名不合法".to_string())?;
    fs::create_dir_all(dir).map_err(|error| format!("创建上传目录失败：{error}"))?;
    let path = dir.join(&name);
    // 防二次穿越：拼接后的路径必须仍在 dir 之下。
    if !path.starts_with(dir) {
        return Err("上传路径越界".to_string());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|error| format!("打开上传文件失败：{error}"))?;
    let current = file.metadata().map(|m| m.len()).unwrap_or(0);
    if offset > current {
        return Err(format!("上传分块偏移不连续：offset={offset} size={current}"));
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| format!("定位上传文件失败：{error}"))?;
    file.write_all(bytes)
        .map_err(|error| format!("写入上传分块失败：{error}"))?;
    file.flush().map_err(|error| format!("刷新上传文件失败：{error}"))?;
    file.metadata()
        .map(|m| m.len())
        .map_err(|error| format!("读取上传文件大小失败：{error}"))
}

/// 读取已暂存上传文件的完整路径（供 AttachFile 传给 CDP）。
pub fn staged_path(dir: &Path, file_name: &str) -> Result<PathBuf, String> {
    let name = safe_file_name(file_name).ok_or_else(|| "文件名不合法".to_string())?;
    let path = dir.join(&name);
    if !path.starts_with(dir) || !path.is_file() {
        return Err("上传文件不存在或路径越界".to_string());
    }
    Ok(path)
}

/// 下载目录中的一个文件。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadEntry {
    pub name: String,
    pub size: u64,
    /// Unix 毫秒。
    pub modified: i64,
}

/// 列出下载目录下的文件（按修改时间倒序），忽略子目录与临时下载文件（.crdownload）。
pub fn list_downloads(dir: &Path) -> Vec<DownloadEntry> {
    let mut entries = Vec::new();
    let Ok(read_dir) = fs::read_dir(dir) else { return entries };
    for item in read_dir.flatten() {
        let Ok(ft) = item.file_type() else { continue };
        if !ft.is_file() {
            continue;
        }
        let name = item.file_name().to_string_lossy().to_string();
        if name.ends_with(".crdownload") || name.starts_with('.') {
            continue;
        }
        let meta = match item.metadata() { Ok(m) => m, Err(_) => continue };
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        entries.push(DownloadEntry { name, size: meta.len(), modified });
    }
    entries.sort_by(|a, b| b.modified.cmp(&a.modified));
    entries
}

/// 从下载目录读取指定文件的指定区间，返回 base64 与是否读到末尾。
pub fn read_download_chunk(
    dir: &Path,
    file_name: &str,
    offset: u64,
    max_bytes: usize,
) -> Result<(String, u64, bool), String> {
    let name = safe_file_name(file_name).ok_or_else(|| "文件名不合法".to_string())?;
    let path = dir.join(&name);
    if !path.starts_with(dir) || !path.is_file() {
        return Err("下载文件不存在或路径越界".to_string());
    }
    let mut file = fs::File::open(&path).map_err(|error| format!("打开下载文件失败：{error}"))?;
    let total = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut buf = vec![0u8; max_bytes];
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| format!("定位下载文件失败：{error}"))?;
    let read = file.read(&mut buf).map_err(|error| format!("读取下载文件失败：{error}"))?;
    buf.truncate(read);
    let done = offset.saturating_add(read as u64) >= total;
    Ok((b64_encode(&buf), total, done))
}

// ── base64（自带实现，避免为此引入新依赖） ────────────────────────────

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn b64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    let mut i = 0;
    while i < input.len() {
        let a = input[i];
        let b = if i + 1 < input.len() { input[i + 1] } else { 0 };
        let c = if i + 2 < input.len() { input[i + 2] } else { 0 };
        out.push(B64[(a >> 2) as usize] as char);
        out.push(B64[(((a & 0x03) << 4) | (b >> 4)) as usize] as char);
        if i + 1 < input.len() {
            out.push(B64[(((b & 0x0f) << 2) | (c >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < input.len() {
            out.push(B64[(c & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

pub fn b64_decode(input: &str) -> Option<Vec<u8>> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for &ch in bytes {
        if ch == b'=' { break; }
        if ch == b'\n' || ch == b'\r' || ch == b' ' || ch == b'\t' { continue; }
        let val = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a' + 26,
            b'0'..=b'9' => ch - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32;
        acc = (acc << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1u32 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_roundtrip() {
        for raw in [b"".as_slice(), b"a", b"ab", b"abc", b"hello world", &[0xff, 0x00, 0x80, 0x7f]] {
            let encoded = b64_encode(raw);
            assert_eq!(b64_decode(&encoded).unwrap(), raw);
        }
    }

    #[test]
    fn rejects_directory_traversal_in_file_names() {
        assert_eq!(safe_file_name("../../evil.txt"), Some("evil.txt".into()));
        assert_eq!(safe_file_name("..\\..\\evil.txt"), Some("evil.txt".into()));
        assert_eq!(safe_file_name(".."), None);
        assert_eq!(safe_file_name("   "), None);
        assert_eq!(safe_file_name("a/b/c.png"), Some("c.png".into()));
    }

    #[test]
    fn sanitizes_windows_reserved_characters() {
        assert_eq!(safe_file_name("a:b*c?.png"), Some("a_b_c_.png".into()));
    }

    #[test]
    fn session_id_is_file_safe() {
        assert_eq!(safe_session_id("model-123"), Some("model-123".into()));
        assert_eq!(safe_session_id("../escape"), Some("___escape".into()));
        assert_eq!(safe_session_id(""), None);
    }

    #[test]
    fn dirs_are_under_data_root() {
        let root = Path::new("D:/tmp/data");
        assert_eq!(upload_dir(root, "m1").unwrap(), root.join("live-uploads/m1"));
        assert_eq!(download_dir(root, "m1").unwrap(), root.join("live-downloads/m1"));
        assert!(upload_dir(root, "").is_none());
    }

    #[test]
    fn chunked_write_reassembles_file() {
        let root = std::env::temp_dir().join(format!("dbx-transfer-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let data = b"hello\x00world";
        let chunks = [(0usize, &data[0..5]), (5, &data[5..11])];
        for (offset, chunk) in chunks {
            let len = write_chunk(&root, "f.bin", offset as u64, chunk).unwrap();
            let _ = len;
        }
        let merged = fs::read(root.join("f.bin")).unwrap();
        assert_eq!(merged, data);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn chunk_write_rejects_gap() {
        let root = std::env::temp_dir().join(format!("dbx-transfer-gap-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        write_chunk(&root, "f.bin", 0, b"abc").unwrap();
        assert!(write_chunk(&root, "f.bin", 10, b"x").is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn lists_downloads_newest_first() {
        let root = std::env::temp_dir().join(format!("dbx-transfer-list-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("a.txt"), b"aa").unwrap();
        fs::write(root.join("b.txt"), b"bbbb").unwrap();
        fs::write(root.join("x.crdownload"), b"partial").unwrap();
        let list = list_downloads(&root);
        assert_eq!(list.len(), 2);
        assert!(list.iter().all(|e| e.name.ends_with(".txt")));
        let (b64, total, done) = read_download_chunk(&root, "b.txt", 0, 1024).unwrap();
        assert_eq!(b64_decode(&b64).unwrap(), b"bbbb");
        assert_eq!(total, 4);
        assert!(done);
        let _ = fs::remove_dir_all(&root);
    }
}
