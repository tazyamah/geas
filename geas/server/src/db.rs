//! DB の層。Geas の【保存】（store）を SQLite に置く。
//! 実体ごとに1つの表（キー＋JSON）を持つ。処理はメモリ上の保存で動き、ここへは処理が変えた行だけを書く。
//! 別の接続（同じファイルを使う別のサーバー・人の手）が書いたかは data_version で分かり、そのときだけ全部を読み直す。
//! この層だけが SQLite を知っている（別の DB に替えるときは、ここだけを替える）。
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

pub struct Db {
    conn: Connection,
    entities: Vec<String>,
}

pub struct Wait {
    pub id: i64,
    pub trigger: String,
    pub args: Value,
    pub collected: Value,
    pub deadline: i64,
}

type R<T> = Result<T, String>;
fn e(x: rusqlite::Error) -> String {
    format!("DB: {}", x)
}

impl Db {
    pub fn open(path: &str, entities: Vec<String>) -> R<Db> {
        let conn = Connection::open(path).map_err(e)?;
        // 同じファイルを使う別プロセスが書き込み中なら、失敗にせず待つ（直列の保証は SQLite の書き込みロックが担う）
        conn.busy_timeout(std::time::Duration::from_secs(10)).map_err(e)?;
        // 行の書き込み（実体ごとに書き込みと削除の 2 つ）を、準備済みのまま使い回す
        conn.set_prepared_statement_cache_capacity(16 + 2 * entities.len());
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;").map_err(e)?;
        for en in &entities {
            conn.execute_batch(&format!("CREATE TABLE IF NOT EXISTS \"{}\" (k TEXT PRIMARY KEY, v TEXT NOT NULL);", en)).map_err(e)?;
        }
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS geas_waits (id INTEGER PRIMARY KEY AUTOINCREMENT, trigger TEXT NOT NULL, args TEXT NOT NULL, collected TEXT NOT NULL, deadline INTEGER NOT NULL, UNIQUE(trigger, args));
             CREATE TABLE IF NOT EXISTS geas_effects (id INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL, name TEXT NOT NULL, args TEXT NOT NULL, idem_key TEXT NOT NULL, trigger TEXT);
             CREATE TABLE IF NOT EXISTS geas_kv (k TEXT PRIMARY KEY, v INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS geas_waits_deadline ON geas_waits (deadline);",
        )
        .map_err(e)?;
        Ok(Db { conn, entities })
    }

    pub fn begin(&self) -> R<()> {
        self.conn.execute_batch("BEGIN IMMEDIATE").map_err(e)
    }
    pub fn commit(&self) -> R<()> {
        self.conn.execute_batch("COMMIT").map_err(e)
    }
    pub fn rollback(&self) {
        let _ = self.conn.execute_batch("ROLLBACK");
    }
    /// 読むだけのトランザクション。複数の表を同じ時点の内容で読むために使う（終わりは rollback）
    pub fn begin_read(&self) -> R<()> {
        self.conn.execute_batch("BEGIN").map_err(e)
    }
    pub fn in_transaction(&self) -> bool {
        !self.conn.is_autocommit()
    }
    /// 別の接続（同じ DB を使う別のサーバー・人の手）が書き込むと変わる値。この接続の書き込みでは変わらない（PRAGMA data_version）。
    /// サーバーは保存をメモリに持ち、この値が変わったときだけ DB から読み直す
    pub fn data_version(&self) -> R<i64> {
        self.conn.query_row("PRAGMA data_version", [], |r| r.get(0)).map_err(e)
    }

    /// 保存の全部の行を、(実体の名前, キー, 行の JSON) で 1 行ずつ渡す
    pub fn each_row(&self, mut f: impl FnMut(&str, &str, &str) -> R<()>) -> R<()> {
        for en in &self.entities {
            let mut st = self.conn.prepare(&format!("SELECT k, v FROM \"{}\"", en)).map_err(e)?;
            let mut rows = st.query([]).map_err(e)?;
            while let Some(r) = rows.next().map_err(e)? {
                let k = r.get_ref(0).map_err(e)?.as_str().map_err(|x| format!("DB: {}", x))?;
                let v = r.get_ref(1).map_err(e)?.as_str().map_err(|x| format!("DB: {}", x))?;
                f(en, k, v)?;
            }
        }
        Ok(())
    }

    /// 1 行を書く。row が None なら、その行を消す
    pub fn write_row(&self, entity: &str, key: &str, row: Option<&str>) -> R<()> {
        if !self.has_entity(entity) {
            return Err(format!("DB: 実体「{}」の表はありません", entity));
        }
        match row {
            Some(v) => self
                .conn
                .prepare_cached(&format!("INSERT INTO \"{}\" (k, v) VALUES (?1, ?2) ON CONFLICT(k) DO UPDATE SET v = excluded.v", entity))
                .and_then(|mut st| st.execute(params![key, v]))
                .map_err(e)?,
            None => self.conn.prepare_cached(&format!("DELETE FROM \"{}\" WHERE k = ?1", entity)).and_then(|mut st| st.execute(params![key])).map_err(e)?,
        };
        Ok(())
    }

    /// 保存のどの表にも行が無いか（全件は読まない）
    pub fn is_empty(&self) -> R<bool> {
        for en in &self.entities {
            let any: Option<i64> = self.conn.query_row(&format!("SELECT 1 FROM \"{}\" LIMIT 1", en), [], |r| r.get(0)).optional().map_err(e)?;
            if any.is_some() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn has_entity(&self, entity: &str) -> bool {
        self.entities.iter().any(|x| x == entity)
    }

    pub fn next_counter(&self, name: &str) -> R<i64> {
        self.conn
            .execute("INSERT INTO geas_kv (k, v) VALUES (?1, 1) ON CONFLICT(k) DO UPDATE SET v = v + 1", params![name])
            .map_err(e)?;
        self.conn.query_row("SELECT v FROM geas_kv WHERE k = ?1", params![name], |r| r.get(0)).map_err(e)
    }

    // ---- 待ち（トリガの実体） ----
    pub fn add_wait(&self, trigger: &str, args: &Value, deadline: i64) -> R<()> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO geas_waits (trigger, args, collected, deadline) VALUES (?1, ?2, '{}', ?3)",
                params![trigger, args.to_string(), deadline],
            )
            .map_err(e)?;
        Ok(())
    }
    fn wait_of(r: &rusqlite::Row) -> rusqlite::Result<Wait> {
        let args: String = r.get(2)?;
        let col: String = r.get(3)?;
        Ok(Wait { id: r.get(0)?, trigger: r.get(1)?, args: serde_json::from_str(&args).unwrap_or(Value::Null), collected: serde_json::from_str(&col).unwrap_or(Value::Null), deadline: r.get(4)? })
    }
    pub fn find_wait(&self, trigger: &str, args: &Value) -> R<Option<Wait>> {
        self.conn
            .query_row("SELECT id, trigger, args, collected, deadline FROM geas_waits WHERE trigger = ?1 AND args = ?2", params![trigger, args.to_string()], Db::wait_of)
            .optional()
            .map_err(e)
    }
    pub fn set_collected(&self, id: i64, collected: &Value) -> R<()> {
        self.conn.execute("UPDATE geas_waits SET collected = ?1 WHERE id = ?2", params![collected.to_string(), id]).map_err(e)?;
        Ok(())
    }
    /// 待ちを消す。消せた（この呼び出しが消した）なら true。すでに他のサーバーが消していたら false
    pub fn remove_wait(&self, id: i64) -> R<bool> {
        let n = self.conn.execute("DELETE FROM geas_waits WHERE id = ?1", params![id]).map_err(e)?;
        Ok(n > 0)
    }
    pub fn due_waits(&self, now: i64) -> R<Vec<Wait>> {
        let mut st = self.conn.prepare("SELECT id, trigger, args, collected, deadline FROM geas_waits WHERE deadline <= ?1 ORDER BY id").map_err(e)?;
        let rows = st.query_map(params![now], Db::wait_of).map_err(e)?;
        rows.map(|r| r.map_err(e)).collect()
    }
    pub fn all_waits(&self) -> R<Vec<Wait>> {
        let mut st = self.conn.prepare("SELECT id, trigger, args, collected, deadline FROM geas_waits ORDER BY id").map_err(e)?;
        let rows = st.query_map([], Db::wait_of).map_err(e)?;
        rows.map(|r| r.map_err(e)).collect()
    }

    // ---- 外部作用の記録 ----
    pub fn log_effect(&self, ts: i64, name: &str, args: &Value, idem_key: &str, trigger: Option<&str>) -> R<()> {
        self.conn
            .execute("INSERT INTO geas_effects (ts, name, args, idem_key, trigger) VALUES (?1, ?2, ?3, ?4, ?5)", params![ts, name, args.to_string(), idem_key, trigger])
            .map_err(e)?;
        Ok(())
    }
    pub fn recent_effects(&self, n: i64) -> R<Vec<Value>> {
        let mut st = self.conn.prepare("SELECT id, ts, name, args, idem_key, trigger FROM geas_effects ORDER BY id DESC LIMIT ?1").map_err(e)?;
        let rows = st
            .query_map(params![n], |r| {
                let args: String = r.get(3)?;
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?, "ts": r.get::<_, i64>(1)?, "name": r.get::<_, String>(2)?,
                    "args": serde_json::from_str::<Value>(&args).unwrap_or(Value::Null), "idem_key": r.get::<_, String>(4)?, "trigger": r.get::<_, Option<String>>(5)?
                }))
            })
            .map_err(e)?;
        rows.map(|r| r.map_err(e)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(db: &Db) -> Vec<(String, String, String)> {
        let mut out = Vec::new();
        db.each_row(|en, k, v| {
            out.push((en.to_string(), k.to_string(), v.to_string()));
            Ok(())
        })
        .unwrap();
        out
    }

    // 1 行ずつ書き、消せる。ほかの行と実体には触れない。知らない実体の表には書かない
    #[test]
    fn write_row_writes_and_deletes_only_that_row() {
        let db = Db::open(":memory:", vec!["A".into(), "B".into()]).unwrap();
        db.write_row("A", "a1", Some("{\"n\":1}")).unwrap();
        db.write_row("A", "a2", Some("{\"n\":2}")).unwrap();
        db.write_row("B", "b1", Some("{\"n\":3}")).unwrap();
        db.write_row("A", "a1", Some("{\"n\":10}")).unwrap();
        db.write_row("A", "a2", None).unwrap();
        assert_eq!(rows(&db), vec![("A".into(), "a1".into(), "{\"n\":10}".into()), ("B".into(), "b1".into(), "{\"n\":3}".into())]);
        assert!(db.write_row("geas_kv\" (k) VALUES ('x'); --", "k", Some("{}")).is_err());
    }

    // data_version は、同じ DB を使う別の接続が書いたときだけ変わる（自分の書き込みでは変わらない）。
    // サーバーはこれで、メモリに持った保存を読み直すかを決める
    #[test]
    fn data_version_changes_only_when_another_connection_writes() {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let path = std::env::temp_dir().join(format!("geas_db_test_{}_{}.sqlite", std::process::id(), nanos));
        let p = path.to_str().unwrap();
        let a = Db::open(p, vec!["A".into()]).unwrap();
        let b = Db::open(p, vec!["A".into()]).unwrap();
        let v0 = a.data_version().unwrap();
        a.begin().unwrap();
        a.write_row("A", "a1", Some("{}")).unwrap();
        a.commit().unwrap();
        assert_eq!(a.data_version().unwrap(), v0);
        b.write_row("A", "b1", Some("{}")).unwrap();
        let v1 = a.data_version().unwrap();
        assert_ne!(v1, v0);
        // トランザクションの中でも、始まる前に別の接続が書いた分は見える
        b.write_row("A", "b2", Some("{}")).unwrap();
        a.begin().unwrap();
        assert_ne!(a.data_version().unwrap(), v1);
        assert_eq!(rows(&a).len(), 3);
        a.rollback();
        drop((a, b));
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{}", p, suffix));
        }
    }
}
