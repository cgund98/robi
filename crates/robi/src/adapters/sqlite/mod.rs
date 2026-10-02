//! SQLite pool and schema bootstrap.

use std::str::FromStr;
use std::time::Duration;

use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    SqlitePool,
};

/// A failure opening the file or applying migrations.
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("sqlite connection failed: {0}")]
    Connect(#[from] sqlx::Error),

    #[error("sqlite migration failed: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

/// Open `database_url`, set the local-file pragmas, and apply migrations.
///
/// In-memory URLs skip `journal_mode=WAL`. SQLite refuses WAL on a memory
/// database, and the adapter tests use one.
pub async fn init_pool(database_url: &str) -> Result<SqlitePool, InitError> {
    let mut options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(30));

    if !is_memory(database_url) {
        options = options.journal_mode(SqliteJournalMode::Wal);
    }

    let pool = SqlitePoolOptions::new()
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                sqlx::query("PRAGMA temp_store = MEMORY;")
                    .execute(&mut *conn)
                    .await?;
                sqlx::query("PRAGMA cache_size = -20000;")
                    .execute(&mut *conn)
                    .await?;
                Ok(())
            })
        })
        .connect_with(options)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}

fn is_memory(database_url: &str) -> bool {
    database_url.contains("mode=memory") || database_url.contains(":memory:")
}
