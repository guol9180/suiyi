//! 生词本：通过 AnkiConnect 把词条加进本地 Anki。
//!
//! 为什么走 Anki 而不是自建词库：背单词这件事 Anki 已经做到极致，
//! 自建一套列表既没有复习算法也留不住用户。这里只做「一键送进 Anki」。
//!
//! AnkiConnect 是 Anki 的本地插件（默认 127.0.0.1:8765），未安装或未启动时
//! 探测会失败，前端据此给出可执行的提示而不是干巴巴的报错。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

pub const DEFAULT_ANKI_URL: &str = "http://127.0.0.1:8765";
pub const DEFAULT_ANKI_DECK: &str = "随译";
/// 加进去的词条统一打这个标签，方便在 Anki 里筛出随译添加的内容
pub const ANKI_TAG: &str = "suiyi";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnkiStatus {
    /// 能连上 AnkiConnect 且版本可用
    pub available: bool,
    pub version: Option<i64>,
    /// 目标牌组是否已存在（不存在会在第一次添加时自动创建）
    pub deck_exists: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnkiAddResult {
    pub added: bool,
    /// 词条已存在（重复添加不算失败）
    pub duplicate: bool,
    pub note_id: Option<i64>,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnkiResponse {
    result: Option<Value>,
    error: Option<Value>,
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

/// 发一条 AnkiConnect 请求。把 error 字段翻成人话返回。
async fn call(client: &reqwest::Client, url: &str, action: &str, params: Value) -> Result<Value, String> {
    let body = json!({ "action": action, "version": 6, "params": params });
    let resp = tokio::time::timeout(
        Duration::from_secs(5),
        client.post(url).json(&body).send(),
    )
    .await
    .map_err(|_| "连接 Anki 超时".to_string())?
    .map_err(|e| format!("连接 Anki 失败: {e}"))?;

    let parsed: AnkiResponse = resp
        .json()
        .await
        .map_err(|e| format!("Anki 返回内容无法解析: {e}"))?;

    if let Some(err) = parsed.error {
        if !err.is_null() {
            return Err(match err.as_str() {
                Some(s) => s.to_string(),
                None => err.to_string(),
            });
        }
    }
    Ok(parsed.result.unwrap_or(Value::Null))
}

fn normalized(url: &str) -> String {
    let u = url.trim();
    if u.is_empty() {
        DEFAULT_ANKI_URL.to_string()
    } else {
        u.to_string()
    }
}

fn normalized_deck(deck: &str) -> String {
    let d = deck.trim();
    if d.is_empty() {
        DEFAULT_ANKI_DECK.to_string()
    } else {
        d.to_string()
    }
}

/// 探测 AnkiConnect 是否可用，并检查目标牌组是否存在
pub async fn status(url: &str, deck: &str) -> AnkiStatus {
    let (url, deck) = (normalized(url), normalized_deck(deck));
    let c = client();

    let version = match call(&c, &url, "version", json!({})).await {
        Ok(v) => v.as_i64(),
        Err(e) => {
            return AnkiStatus {
                available: false,
                version: None,
                deck_exists: false,
                error: Some(e),
            }
        }
    };

    let deck_exists = match call(&c, &url, "deckNames", json!({})).await {
        Ok(v) => v
            .as_array()
            .map(|arr| arr.iter().any(|d| d.as_str() == Some(deck.as_str())))
            .unwrap_or(false),
        Err(_) => false,
    };

    AnkiStatus {
        available: true,
        version,
        deck_exists,
        error: None,
    }
}

/// 添加词条。牌组不存在会先创建；重复词条按「已在生词本中」处理，不算失败。
pub async fn add_note(
    url: &str,
    deck: &str,
    front: &str,
    back: &str,
) -> Result<AnkiAddResult, String> {
    let (url, deck) = (normalized(url), normalized_deck(deck));
    let front = front.trim();
    let back = back.trim();
    if front.is_empty() || back.is_empty() {
        return Err("词条内容为空，无法加入生词本".into());
    }

    let c = client();

    // 牌组不存在就先建，否则 addNote 会报 deck was not found
    if let Ok(v) = call(&c, &url, "deckNames", json!({})).await {
        let exists = v
            .as_array()
            .map(|arr| arr.iter().any(|d| d.as_str() == Some(deck.as_str())))
            .unwrap_or(false);
        if !exists {
            call(&c, &url, "createDeck", json!({ "deck": deck })).await?;
        }
    }

    let note = json!({
        "deckName": deck,
        "modelName": "Basic",
        "fields": { "Front": front, "Back": back },
        "tags": [ANKI_TAG],
        "options": { "allowDuplicate": false },
    });

    match call(&c, &url, "addNote", json!({ "note": note })).await {
        Ok(v) => Ok(AnkiAddResult {
            added: v.as_i64().is_some(),
            duplicate: false,
            note_id: v.as_i64(),
            error: None,
        }),
        // AnkiConnect 对重复词条会直接报错，这里当成「已存在」而不是失败
        Err(e) if e.to_lowercase().contains("duplicate") => Ok(AnkiAddResult {
            added: false,
            duplicate: true,
            note_id: None,
            error: None,
        }),
        Err(e) => Ok(AnkiAddResult {
            added: false,
            duplicate: false,
            note_id: None,
            error: Some(e),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_defaults() {
        assert_eq!(normalized("   "), DEFAULT_ANKI_URL);
        assert_eq!(normalized("http://127.0.0.1:9999"), "http://127.0.0.1:9999");
        assert_eq!(normalized_deck(""), DEFAULT_ANKI_DECK);
        assert_eq!(normalized_deck("  考研  "), "考研");
    }
}

// ==================== 命令层 ====================

/// 探测 Anki 连接状态（未安装 AnkiConnect 或 Anki 没开时会返回 available=false）
#[tauri::command]
pub async fn anki_status(app: tauri::AppHandle) -> Result<AnkiStatus, String> {
    let dir = crate::commands::config_dir(&app)?;
    let file = crate::config::load_services(&dir)?;
    Ok(status(&file.anki_url, &file.anki_deck).await)
}

/// 把词条加入生词本
#[tauri::command]
pub async fn anki_add(
    app: tauri::AppHandle,
    front: String,
    back: String,
) -> Result<AnkiAddResult, String> {
    let dir = crate::commands::config_dir(&app)?;
    let file = crate::config::load_services(&dir)?;
    add_note(&file.anki_url, &file.anki_deck, &front, &back).await
}
