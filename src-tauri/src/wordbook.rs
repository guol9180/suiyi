//! 生词本：本地先记，再往 Anki 送。
//!
//! 为什么不是「点一下直接调 AnkiConnect」：Anki 没开、插件没装、牌组被删，
//! 任何一种情况都会让用户刚刚点下的那个词凭空消失。这里改成先落到本地库，
//! 立刻把词条显示出来，能连上就顺手同步；连不上就留在「待同步」，
//! 下次同步时批量补发。界面上看到的每一个状态都对应库里的真实字段。

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const WORDBOOK_FILE: &str = "wordbook.db";
/// 一次批量同步最多处理多少条，避免 Anki 没开时反复空转
pub const SYNC_BATCH: i64 = 200;

/// 生词本里的一条词
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WordEntry {
    pub id: i64,
    /// 毫秒时间戳
    pub created_at: i64,
    /// 词条本身，也就是 Anki 卡片的正面
    pub term: String,
    /// 音标或读音，可为空
    pub reading: String,
    /// 释义或译文，Anki 卡片的背面
    pub meaning: String,
    /// 来源：selection / screenshot / manual / input
    pub source: String,
    /// 同步成功的时间；None 表示还在待同步队列里
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synced_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note_id: Option<i64>,
    /// 最近一次同步失败的原因，同步成功后清空
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl WordEntry {
    pub fn synced(&self) -> bool {
        self.synced_at.is_some()
    }
}

/// 生词本总览：界面上「N 张待同步」直接用这里的数字
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WordbookStats {
    pub total: i64,
    pub pending: i64,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn open(dir: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("创建数据目录失败: {e}"))?;
    let conn = Connection::open(dir.join(WORDBOOK_FILE))
        .map_err(|e| format!("打开生词本失败: {e}"))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         CREATE TABLE IF NOT EXISTS words (
           id         INTEGER PRIMARY KEY AUTOINCREMENT,
           created_at INTEGER NOT NULL,
           term       TEXT    NOT NULL,
           reading    TEXT    NOT NULL DEFAULT '',
           meaning    TEXT    NOT NULL,
           source     TEXT    NOT NULL DEFAULT '',
           synced_at  INTEGER,
           note_id    INTEGER,
           last_error TEXT
         );
         CREATE INDEX IF NOT EXISTS idx_words_created ON words(created_at DESC);
         CREATE INDEX IF NOT EXISTS idx_words_pending ON words(synced_at);",
    )
    .map_err(|e| format!("初始化生词本失败: {e}"))?;
    Ok(conn)
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<WordEntry> {
    Ok(WordEntry {
        id: row.get(0)?,
        created_at: row.get(1)?,
        term: row.get(2)?,
        reading: row.get(3)?,
        meaning: row.get(4)?,
        source: row.get(5)?,
        synced_at: row.get(6)?,
        note_id: row.get(7)?,
        last_error: row.get(8)?,
    })
}

const COLUMNS: &str =
    "id, created_at, term, reading, meaning, source, synced_at, note_id, last_error";

/// 同一个词条加同一个释义只留一条，重复添加直接把已有的那条还回去
pub fn add(
    dir: &Path,
    term: &str,
    reading: &str,
    meaning: &str,
    source: &str,
) -> Result<(WordEntry, bool), String> {
    let term = term.trim();
    let reading = reading.trim();
    let meaning = meaning.trim();
    let source = source.trim();
    if term.is_empty() || meaning.is_empty() {
        return Err("词条或释义为空，无法加入生词本".into());
    }

    let conn = open(dir)?;
    if let Some(existing) = find(&conn, term, meaning)? {
        return Ok((existing, false));
    }

    let created_at = now_ms();
    conn.execute(
        "INSERT INTO words (created_at, term, reading, meaning, source)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![created_at, term, reading, meaning, source],
    )
    .map_err(|e| format!("写入生词本失败: {e}"))?;
    let id = conn.last_insert_rowid();

    Ok((
        WordEntry {
            id,
            created_at,
            term: term.to_string(),
            reading: reading.to_string(),
            meaning: meaning.to_string(),
            source: source.to_string(),
            synced_at: None,
            note_id: None,
            last_error: None,
        },
        true,
    ))
}

fn find(conn: &Connection, term: &str, meaning: &str) -> Result<Option<WordEntry>, String> {
    conn.query_row(
        &format!(
            "SELECT {COLUMNS} FROM words
             WHERE term = ?1 COLLATE NOCASE AND meaning = ?2 COLLATE NOCASE
             ORDER BY id LIMIT 1"
        ),
        params![term, meaning],
        row_to_entry,
    )
    .optional()
    .map_err(|e| format!("查询词条失败: {e}"))
}

/// 按加入时间倒序列出
pub fn list(dir: &Path, limit: i64, offset: i64) -> Result<Vec<WordEntry>, String> {
    let conn = open(dir)?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM words ORDER BY created_at DESC, id DESC LIMIT ?1 OFFSET ?2"
        ))
        .map_err(|e| format!("查询生词本失败: {e}"))?;
    let rows = stmt
        .query_map(params![limit.clamp(1, 1000), offset.max(0)], row_to_entry)
        .map_err(|e| format!("查询生词本失败: {e}"))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| format!("读取生词本失败: {e}"))
}

/// 还在等同步的词条，按加入顺序（先进先出）补发
pub fn pending(dir: &Path) -> Result<Vec<WordEntry>, String> {
    let conn = open(dir)?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM words WHERE synced_at IS NULL
             ORDER BY created_at ASC, id ASC LIMIT ?1"
        ))
        .map_err(|e| format!("查询待同步词条失败: {e}"))?;
    let rows = stmt
        .query_map(params![SYNC_BATCH], row_to_entry)
        .map_err(|e| format!("查询待同步词条失败: {e}"))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| format!("读取待同步词条失败: {e}"))
}

pub fn mark_synced(dir: &Path, id: i64, note_id: Option<i64>) -> Result<(), String> {
    let conn = open(dir)?;
    conn.execute(
        "UPDATE words SET synced_at = ?2, note_id = ?3, last_error = NULL WHERE id = ?1",
        params![id, now_ms(), note_id],
    )
    .map_err(|e| format!("更新同步状态失败: {e}"))?;
    Ok(())
}

pub fn mark_failed(dir: &Path, id: i64, error: &str) -> Result<(), String> {
    let conn = open(dir)?;
    conn.execute(
        "UPDATE words SET last_error = ?2 WHERE id = ?1",
        params![id, error],
    )
    .map_err(|e| format!("记录同步失败原因失败: {e}"))?;
    Ok(())
}

/// 删除返回是否真的删掉了一条，前端据此决定要不要刷新
pub fn remove(dir: &Path, id: i64) -> Result<bool, String> {
    let conn = open(dir)?;
    let n = conn
        .execute("DELETE FROM words WHERE id = ?1", params![id])
        .map_err(|e| format!("删除词条失败: {e}"))?;
    Ok(n > 0)
}

pub fn stats(dir: &Path) -> Result<WordbookStats, String> {
    let conn = open(dir)?;
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM words", [], |r| r.get(0))
        .map_err(|e| format!("统计生词本失败: {e}"))?;
    let pending: i64 = conn
        .query_row("SELECT COUNT(*) FROM words WHERE synced_at IS NULL", [], |r| {
            r.get(0)
        })
        .map_err(|e| format!("统计待同步失败: {e}"))?;
    Ok(WordbookStats { total, pending })
}

// ==================== 命令层 ====================

/// 一次操作之后的完整视图：列表 + 计数 + 这次同步的结果。
/// 每次都把整个视图还回去，前端不用再补一次查询，也就不会出现
/// 「列表已经是新的、计数还是旧的」这类错位。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WordbookView {
    pub entries: Vec<WordEntry>,
    pub stats: WordbookStats,
    /// 这次操作有没有词条被送进 Anki
    pub synced: bool,
    /// 词条之前就在 Anki 里
    pub duplicate: bool,
    /// 同步失败的原因，成功或排队时为 None
    pub error: Option<String>,
}

async fn db<T, F>(f: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("生词本任务失败: {e}"))?
}

async fn view(dir: &Path) -> Result<WordbookView, String> {
    let d = dir.to_path_buf();
    let (entries, stats) = db(move || Ok((list(&d, 200, 0)?, stats(&d)?))).await?;
    Ok(WordbookView {
        entries,
        stats,
        synced: false,
        duplicate: false,
        error: None,
    })
}

/// 把一个词条推给 Anki，并按真实结果更新本地状态
async fn push(dir: &Path, url: &str, deck: &str, entry: &WordEntry) -> (bool, bool, Option<String>) {
    let (id, term, meaning) = (entry.id, entry.term.clone(), entry.meaning.clone());
    let (d, u, k) = (dir.to_path_buf(), url.to_string(), deck.to_string());

    match crate::anki::add_note(&u, &k, &term, &meaning).await {
        // 已经在 Anki 里了，同样算同步完成，否则这条会永远卡在待同步
        Ok(r) if r.duplicate => {
            let _ = db(move || mark_synced(&d, id, None)).await;
            (true, true, None)
        }
        Ok(r) if r.added => {
            let note_id = r.note_id;
            let _ = db(move || mark_synced(&d, id, note_id)).await;
            (true, false, None)
        }
        Ok(r) => {
            let msg = r.error.unwrap_or_else(|| "Anki 没有接受这个词条".to_string());
            let m = msg.clone();
            let _ = db(move || mark_failed(&d, id, &m)).await;
            (false, false, Some(msg))
        }
        Err(e) => {
            let m = e.clone();
            let _ = db(move || mark_failed(&d, id, &m)).await;
            (false, false, Some(e))
        }
    }
}

#[tauri::command]
pub async fn wordbook_list(app: tauri::AppHandle) -> Result<WordbookView, String> {
    let dir = crate::commands::config_dir(&app)?;
    view(&dir).await
}

/// 加一个词：先落本地，再顺手推一次。推不动就留在待同步队列里，
/// 不把「Anki 没开」当成错误抛给用户。
#[tauri::command]
pub async fn wordbook_add(
    app: tauri::AppHandle,
    term: String,
    reading: Option<String>,
    meaning: String,
    source: Option<String>,
) -> Result<WordbookView, String> {
    let dir = crate::commands::config_dir(&app)?;
    let reading = reading.unwrap_or_default();
    let source = source.unwrap_or_else(|| "manual".into());

    let d = dir.clone();
    let (entry, created) = db(move || add(&d, &term, &reading, &meaning, &source)).await?;

    if !created {
        // 重复添加不重复推送，只把「已经在生词本里」告诉用户
        let mut out = view(&dir).await?;
        out.duplicate = true;
        out.error = Some("这个词条已经在生词本里了".into());
        return Ok(out);
    }

    let file = crate::config::load_services(&dir)?;
    let (synced, duplicate, error) =
        push(&dir, &file.anki_url, &file.anki_deck, &entry).await;

    // push 已经改过库，这里读到的就是最终状态
    let mut out = view(&dir).await?;
    out.synced = synced;
    out.duplicate = duplicate;
    out.error = if duplicate {
        Some("这个词条已经在 Anki 里了".to_string())
    } else {
        error
    };
    Ok(out)
}

/// 批量补发待同步的词条。Anki 没开时返回一条能看懂的失败原因。
#[tauri::command]
pub async fn wordbook_sync(app: tauri::AppHandle) -> Result<WordbookView, String> {
    let dir = crate::commands::config_dir(&app)?;
    let file = crate::config::load_services(&dir)?;

    let d = dir.clone();
    let queue = db(move || pending(&d)).await?;

    let (mut synced, mut duplicate, mut first_error) = (0usize, 0usize, None);
    for entry in &queue {
        let (ok, dup, err) = push(&dir, &file.anki_url, &file.anki_deck, entry).await;
        if ok {
            synced += 1;
        }
        if dup {
            duplicate += 1;
        }
        if first_error.is_none() {
            first_error = err;
        }
    }

    let mut out = view(&dir).await?;
    out.synced = synced > 0;
    out.duplicate = duplicate > 0;
    out.error = first_error;
    Ok(out)
}

#[tauri::command]
pub async fn wordbook_remove(app: tauri::AppHandle, id: i64) -> Result<WordbookView, String> {
    let dir = crate::commands::config_dir(&app)?;
    let d = dir.clone();
    db(move || remove(&d, id)).await?;
    view(&dir).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static SEQ: AtomicU32 = AtomicU32::new(0);

    /// 每个用例一个独立目录，互不干扰
    fn tmp_dir() -> std::path::PathBuf {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "suiyi-wordbook-{}-{}",
            std::process::id(),
            n
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn adds_and_lists_newest_first() {
        let dir = tmp_dir();
        let (a, created) = add(&dir, "retrieval", "", "检索", "selection").unwrap();
        assert!(created);
        let (b, _) = add(&dir, "ablations", "", "消融实验", "manual").unwrap();
        assert!(b.id > a.id);

        let all = list(&dir, 50, 0).unwrap();
        assert_eq!(all.len(), 2);
        // 后加的排在前面：same 毫秒时按 id 兜底
        assert_eq!(all[0].term, "ablations");
        assert_eq!(all[1].term, "retrieval");
        assert_eq!(all[1].source, "selection");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicate_term_and_meaning_returns_existing_row() {
        let dir = tmp_dir();
        let (first, created) = add(&dir, "Lazy Dog", "", "懒狗", "input").unwrap();
        assert!(created);
        // 大小写不同也算同一条，不该写第二行
        let (again, created_again) = add(&dir, "lazy dog", "", "懒狗", "input").unwrap();
        assert!(!created_again);
        assert_eq!(again.id, first.id);
        assert_eq!(list(&dir, 50, 0).unwrap().len(), 1);

        // 同一个词不同释义是两条
        let (other, created_other) = add(&dir, "lazy dog", "", "懒散的狗", "input").unwrap();
        assert!(created_other);
        assert_ne!(other.id, first.id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_empty_entries() {
        let dir = tmp_dir();
        assert!(add(&dir, "  ", "", "释义", "manual").is_err());
        assert!(add(&dir, "word", "", "   ", "manual").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pending_becomes_synced_and_stats_follow() {
        let dir = tmp_dir();
        let (a, _) = add(&dir, "retrieval", "", "检索", "manual").unwrap();
        let (_b, _) = add(&dir, "ablations", "", "消融实验", "manual").unwrap();

        assert_eq!(stats(&dir).unwrap(), WordbookStats { total: 2, pending: 2 });
        // 待同步按加入顺序补发，先进先出
        let queue = pending(&dir).unwrap();
        assert_eq!(queue.len(), 2);
        assert_eq!(queue[0].id, a.id);

        mark_synced(&dir, a.id, Some(4321)).unwrap();
        let after = list(&dir, 50, 0).unwrap();
        let row = after.iter().find(|w| w.id == a.id).unwrap();
        assert!(row.synced());
        assert_eq!(row.note_id, Some(4321));
        assert!(row.last_error.is_none());

        assert_eq!(stats(&dir).unwrap(), WordbookStats { total: 2, pending: 1 });
        assert_eq!(pending(&dir).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failure_keeps_entry_pending_and_records_reason() {
        let dir = tmp_dir();
        let (a, _) = add(&dir, "retrieval", "", "检索", "manual").unwrap();
        mark_failed(&dir, a.id, "连接 Anki 失败: 拒绝连接").unwrap();

        let row = &list(&dir, 50, 0).unwrap()[0];
        assert!(!row.synced(), "失败的词条必须留在待同步队列里");
        assert_eq!(row.last_error.as_deref(), Some("连接 Anki 失败: 拒绝连接"));
        assert_eq!(stats(&dir).unwrap().pending, 1);

        // 补发成功后失败原因要清掉，不能留一条过期的报错
        mark_synced(&dir, a.id, Some(7)).unwrap();
        let row = &list(&dir, 50, 0).unwrap()[0];
        assert!(row.synced());
        assert!(row.last_error.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_reports_whether_a_row_went_away() {
        let dir = tmp_dir();
        let (a, _) = add(&dir, "retrieval", "", "检索", "manual").unwrap();
        assert!(remove(&dir, a.id).unwrap());
        assert!(!remove(&dir, a.id).unwrap());
        assert_eq!(stats(&dir).unwrap().total, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
