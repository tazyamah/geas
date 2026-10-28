//! DB の層。Geas の【保存】（store）を SQLite に置く。
//! 実体ごとに1つの表（キー＋JSON）を持ち、処理の前後で保存の差分だけを書く。
//! この層だけが SQLite を知っている（別の DB に替えるときは、ここだけを替える）。
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{Map, Value};

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
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;").map_err(e)?;
        for en in &entities {
            conn.execute_batch(&format!("CREATE TABLE IF NOT EXISTS \"{}\" (k TEXT PRIMARY KEY, v TEXT NOT NULL);", en)).map_err(e)?;
        }
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS geas_waits (id INTEGER PRIMARY KEY AUTOINCREMENT, trigger TEXT NOT NULL, args TEXT NOT NULL, collected TEXT NOT NULL, deadline INTEGER NOT NULL, UNIQUE(trigger, args));
             CREATE TABLE IF NOT EXISTS geas_effects (id INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL, name TEXT NOT NULL, args TEXT NOT NULL, idem_key TEXT NOT NULL, trigger TEXT);
             CREATE TABLE IF NOT EXISTS geas_kv (k TEXT PRIMARY KEY, v INTEGER NOT NULL);",
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

    /// 保存の全体を {実体: {キー: 行}} で読む
    pub fn load_store(&self) -> R<Value> {
        let mut all = Map::new();
        for en in &self.entities {
            let mut st = self.conn.prepare(&format!("SELECT k, v FROM \"{}\"", en)).map_err(e)?;
            let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(e)?;
            let mut m = Map::new();
            for r in rows {
                let (k, v) = r.map_err(e)?;
                m.insert(k, serde_json::from_str(&v).map_err(|x| x.to_string())?);
            }
            all.insert(en.clone(), Value::Object(m));
        }
        Ok(Value::Object(all))
    }

    /// 前後の保存を比べて、変わった行だけを書く
    pub fn save_diff(&self, before: &Value, after: &Value) -> R<()> {
        for en in &self.entities {
            let empty = Map::new();
            let b = before.get(en).and_then(|x| x.as_object()).unwrap_or(&empty);
            let a = after.get(en).and_then(|x| x.as_object()).unwrap_or(&empty);
            for (k, v) in a {
                if b.get(k) != Some(v) {
                    self.conn
                        .execute(&format!("INSERT INTO \"{}\" (k, v) VALUES (?1, ?2) ON CONFLICT(k) DO UPDATE SET v = excluded.v", en), params![k, v.to_string()])
                        .map_err(e)?;
                }
            }
            for k in b.keys() {
                if !a.contains_key(k) {
                    self.conn.execute(&format!("DELETE FROM \"{}\" WHERE k = ?1", en), params![k]).map_err(e)?;
                }
            }
        }
        Ok(())
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
    pub fn remove_wait(&self, id: i64) -> R<()> {
        self.conn.execute("DELETE FROM geas_waits WHERE id = ?1", params![id]).map_err(e)?;
        Ok(())
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
