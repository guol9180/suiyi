//! M4 翻译历史：落 SQLite，支持搜索、清理与保留上限。
//!
//! 设计取舍：
//! - 失败记录也存：设计稿里「哪条服务总是失败」本身就是用户要的决策信息；
//! - 保留上限 2000 条，插入后顺手裁掉更旧的，避免库无限长大；
//! - 数据库放在配置目录，与 services.json 同级，方便一起备份或删除。

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const HISTORY_FILE: &str = "history.db";
/// 保留的最大条数，超出后按时间裁掉最旧的
pub const MAX_ENTRIES: i64 = 2000;

/// 一条历史记录
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: i64,
    /// 毫秒时间戳
    pub created_at: i64,
    /// 来源：selection（划词）/ screenshot（截图）/ manual（手输）/ input（输入框转译，v0.8.1 起已移除，仅历史数据还在）
    pub kind: String,
    pub source: String,
    pub translated: String,
    pub service_name: String,
    pub elapsed_ms: i64,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 待写入的记录（还没有 id；created_at 由调用方给**翻译开始时刻**，
/// 而不是各服务完成的时刻——同一轮的每条服务记录共享同一个开始时间戳，
/// 历史列表才能按「翻译轮次」排序，而不是被各服务的完成顺序搅乱）
#[derive(Debug, Clone)]
pub struct NewEntry {
    pub created_at: i64,
    pub kind: String,
    pub source: String,
    pub translated: String,
    pub service_name: String,
    pub elapsed_ms: i64,
    pub ok: bool,
    pub error: Option<String>,
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn open(dir: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("创建数据目录失败: {e}"))?;
    let conn = Connection::open(dir.join(HISTORY_FILE))
        .map_err(|e| format!("打开历史库失败: {e}"))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         CREATE TABLE IF NOT EXISTS history (
           id           INTEGER PRIMARY KEY AUTOINCREMENT,
           created_at   INTEGER NOT NULL,
           kind         TEXT    NOT NULL,
           source       TEXT    NOT NULL,
           translated   TEXT    NOT NULL,
           service_name TEXT    NOT NULL DEFAULT '',
           elapsed_ms   INTEGER NOT NULL DEFAULT 0,
           ok           INTEGER NOT NULL DEFAULT 1,
           error        TEXT
         );
         CREATE INDEX IF NOT EXISTS idx_history_created ON history(created_at DESC);",
    )
    .map_err(|e| format!("初始化历史库失败: {e}"))?;
    Ok(conn)
}

/// 写入一条记录，并顺手裁掉超出上限的旧记录
pub fn record(dir: &Path, entry: NewEntry) -> Result<i64, String> {
    let conn = open(dir)?;
    conn.execute(
        "INSERT INTO history
           (created_at, kind, source, translated, service_name, elapsed_ms, ok, error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            entry.created_at,
            entry.kind,
            entry.source,
            entry.translated,
            entry.service_name,
            entry.elapsed_ms,
            entry.ok as i64,
            entry.error,
        ],
    )
    .map_err(|e| format!("写入历史失败: {e}"))?;
    let id = conn.last_insert_rowid();
    trim(&conn, MAX_ENTRIES)?;
    Ok(id)
}

/// 只保留最新的 `keep` 条
fn trim(conn: &Connection, keep: i64) -> Result<(), String> {
    conn.execute(
        "DELETE FROM history WHERE id NOT IN (
           SELECT id FROM history ORDER BY created_at DESC, id DESC LIMIT ?1
         )",
        params![keep.max(1)],
    )
    .map_err(|e| format!("裁剪历史失败: {e}"))?;
    Ok(())
}

/// 当前记录条数
pub fn count(dir: &Path) -> Result<i64, String> {
    let conn = open(dir)?;
    conn.query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
        .map_err(|e| format!("统计历史失败: {e}"))
}

/// 查询历史。`query` 非空时在原文与译文里做模糊匹配，`kind` 非空时按来源过滤。
pub fn list(
    dir: &Path,
    query: Option<String>,
    kind: Option<String>,
    limit: i64,
    offset: i64,
) -> Result<Vec<HistoryEntry>, String> {
    let conn = open(dir)?;
    let like = query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(|q| format!("%{q}%"));
    let kind = kind.filter(|k| !k.trim().is_empty());

    let mut stmt = conn
        .prepare(
            "SELECT id, created_at, kind, source, translated, service_name, elapsed_ms, ok, error
               FROM history
              WHERE (?1 IS NULL OR source LIKE ?1 OR translated LIKE ?1)
                AND (?2 IS NULL OR kind = ?2)
              ORDER BY created_at DESC, id DESC
              LIMIT ?3 OFFSET ?4",
        )
        .map_err(|e| format!("准备查询失败: {e}"))?;

    let rows = stmt
        .query_map(params![like, kind, limit.clamp(1, 500), offset.max(0)], |r| {
            Ok(HistoryEntry {
                id: r.get(0)?,
                created_at: r.get(1)?,
                kind: r.get(2)?,
                source: r.get(3)?,
                translated: r.get(4)?,
                service_name: r.get(5)?,
                elapsed_ms: r.get(6)?,
                ok: r.get::<_, i64>(7)? != 0,
                error: r.get(8)?,
            })
        })
        .map_err(|e| format!("执行查询失败: {e}"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("读取历史失败: {e}"))
}

/// 删除单条
pub fn delete(dir: &Path, id: i64) -> Result<(), String> {
    let conn = open(dir)?;
    conn.execute("DELETE FROM history WHERE id = ?1", params![id])
        .map_err(|e| format!("删除历史失败: {e}"))?;
    Ok(())
}

/// 清空全部
pub fn clear(dir: &Path) -> Result<(), String> {
    let conn = open(dir)?;
    conn.execute("DELETE FROM history", [])
        .map_err(|e| format!("清空历史失败: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("suiyi-history-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn entry_at(created_at: i64, src: &str, dst: &str, ok: bool) -> NewEntry {
        NewEntry {
            created_at,
            kind: "selection".into(),
            source: src.into(),
            translated: dst.into(),
            service_name: "ZAI".into(),
            elapsed_ms: 900,
            ok,
            error: if ok { None } else { Some("429 限流".into()) },
        }
    }

    /// 排序看调用方给的 created_at（翻译开始时刻），与插入先后无关：
    /// 一轮翻译的多条服务记录共享同一时间戳，轮与轮严格按开始时间排。
    #[test]
    fn ordering_follows_provided_created_at() {
        let dir = tmp_dir("ordering");

        // 先插「新的」，再插「旧的」，最后插中间：列表仍按时间倒序
        record(&dir, entry_at(200, "newer", "新", true)).unwrap();
        record(&dir, entry_at(100, "older", "旧", true)).unwrap();
        record(&dir, entry_at(150, "middle", "中", true)).unwrap();

        let all = list(&dir, None, None, 50, 0).unwrap();
        let sources: Vec<&str> = all.iter().map(|h| h.source.as_str()).collect();
        assert_eq!(sources, vec!["newer", "middle", "older"], "按 created_at 倒序");

        // 同一轮的两条服务记录时间戳相同：并列时按写入顺序倒排（id DESC），稳定
        record(&dir, entry_at(300, "same-round", "svc1", true)).unwrap();
        record(&dir, entry_at(300, "same-round", "svc2", true)).unwrap();
        let all = list(&dir, None, None, 50, 0).unwrap();
        assert_eq!(all[0].source, "same-round");
        assert_eq!(all[0].translated, "svc2", "同时间戳按 id 倒序，后写的在前");
        assert_eq!(all[1].translated, "svc1");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn insert_list_search_delete() {
        let dir = tmp_dir("basic");

        record(&dir, entry_at(1, "hello world", "你好世界", true)).unwrap();
        record(&dir, entry_at(2, "good morning", "早上好", true)).unwrap();
        record(&dir, entry_at(3, "fail case", "", false)).unwrap();

        // 默认按时间倒序，最新的在前
        let all = list(&dir, None, None, 50, 0).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].source, "fail case");
        assert!(!all[0].ok);
        assert_eq!(all[0].error.as_deref(), Some("429 限流"));

        // 搜索命中原文与译文
        assert_eq!(list(&dir, Some("hello".into()), None, 50, 0).unwrap().len(), 1);
        assert_eq!(list(&dir, Some("早上".into()), None, 50, 0).unwrap().len(), 1);

        // 按来源过滤
        assert_eq!(list(&dir, None, Some("selection".into()), 50, 0).unwrap().len(), 3);
        assert_eq!(list(&dir, None, Some("manual".into()), 50, 0).unwrap().len(), 0);

        // 删除单条
        let id = list(&dir, Some("good".into()), None, 1, 0).unwrap()[0].id;
        delete(&dir, id).unwrap();
        assert_eq!(list(&dir, None, None, 50, 0).unwrap().len(), 2);

        // 清空
        clear(&dir).unwrap();
        assert!(list(&dir, None, None, 50, 0).unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn trims_to_max_entries() {
        let dir = tmp_dir("trim");
        for i in 0..15 {
            record(&dir, entry_at(1_000 + i, &format!("src-{i}"), "译文", true)).unwrap();
        }
        assert_eq!(count(&dir).unwrap(), 15);

        // 直接验裁剪逻辑，避免为了触发生产上限而插 2000 条
        let conn = open(&dir).unwrap();
        trim(&conn, 10).unwrap();
        assert_eq!(count(&dir).unwrap(), 10);

        let newest = list(&dir, None, None, 500, 0).unwrap();
        assert_eq!(newest[0].source, "src-14", "应保留最新的记录");
        assert!(newest.iter().all(|e| e.source != "src-0"), "最旧的应被裁掉");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
