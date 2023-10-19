use std::{fs, path::Path};

use mmmm_core::{Cache, CacheError};
use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, OptionalExtension};

pub struct SqliteCache {
    pool: Pool<SqliteConnectionManager>,
}

impl SqliteCache {
    pub fn new<P>(location: P, clear: bool) -> color_eyre::Result<Self>
    where
        P: AsRef<Path>,
    {
        fs::create_dir_all(&location)?;
        let db_path = location.as_ref().join("mmmm.db");
        if clear {
            fs::remove_file(&db_path)?;
        }
        let manager = r2d2_sqlite::SqliteConnectionManager::file(&db_path);
        let pool = r2d2::Pool::builder()
            .max_size(1)
            .build(manager)?;
        pool.get()?
            .execute("CREATE TABLE IF NOT EXISTS cache (namespace TEXT, key TEXT, data TEXT)", params![])?;
        Ok(Self { pool })
    }
}

impl Cache for SqliteCache {
    fn put(&self, namespace: &str, key: &str, data: &str) -> Result<(), CacheError> {
        let conn: PooledConnection<SqliteConnectionManager> = self.pool.get().map_err(from_r2d2)?;
        conn.execute("INSERT INTO cache (namespace, key, data) VALUES (?1, ?2, ?3)", (namespace, key, data))
            .map_err(from_rusqlite)?;
        Ok(())
    }

    fn get(&self, namespace: &str, key: &str) -> Result<Option<String>, CacheError> {
        let conn = self.pool.get().map_err(from_r2d2)?;
        conn.query_row("SELECT data FROM cache WHERE namespace = ?1 AND key = ?2", (namespace, key), |row| {
            row.get(0)
        })
        .optional()
        .map_err(from_rusqlite)
    }
}

fn from_r2d2(value: r2d2::Error) -> CacheError {
    CacheError {
        msg: format!("r2d2 error: {}", value.to_string()),
    }
}

fn from_rusqlite(value: rusqlite::Error) -> CacheError {
    CacheError {
        msg: format!("rusqlite error: {}", value.to_string()),
    }
}
