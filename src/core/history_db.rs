use std::path::Path;

use rusqlite::{params, Connection, Transaction};

use crate::core::model::HistoryEntry;

pub fn open(
    dir: &Path,
    legacy: impl FnOnce() -> Vec<HistoryEntry>,
) -> rusqlite::Result<Connection> {
    let mut connection = Connection::open(dir.join("history.sqlite3"))?;
    connection.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
        CREATE TABLE IF NOT EXISTS visits (id INTEGER PRIMARY KEY, url TEXT NOT NULL,
            title TEXT NOT NULL, ts INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS visits_order ON visits(ts,id);",
    )?;
    let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version == 0 {
        let transaction = connection.transaction()?;
        for entry in legacy() {
            write_visit(&transaction, &entry)?;
        }
        transaction.pragma_update(None, "user_version", 1)?;
        transaction.commit()?;
    }
    Ok(connection)
}

pub fn read(db: &Connection) -> rusqlite::Result<Vec<HistoryEntry>> {
    let mut query = db.prepare("SELECT id,url,title,ts FROM visits ORDER BY ts,id")?;
    let rows = query.query_map([], |row| {
        Ok(HistoryEntry {
            id: row.get(0)?,
            url: row.get(1)?,
            title: row.get(2)?,
            ts: row.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn write_visit(db: &Transaction<'_>, value: &HistoryEntry) -> rusqlite::Result<()> {
    db.execute(
        "INSERT INTO visits(id,url,title,ts) VALUES (?1,?2,?3,?4)
        ON CONFLICT(id) DO UPDATE SET url=excluded.url,title=excluded.title,ts=excluded.ts",
        params![value.id, value.url, value.title, value.ts],
    )?;
    Ok(())
}
