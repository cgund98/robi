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
        .busy_timeout(Duration::from_secs(10));

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

/// Log a pool acquire timeout or a SQLite busy-lock timeout on its own line.
///
/// Other database errors stay with the caller. These two are the waits that
/// stall the API, and a generic repository log does not name them.
pub(crate) fn log_connection_timeout(context: &'static str, err: &sqlx::Error) {
    match err {
        sqlx::Error::PoolTimedOut => {
            tracing::error!(%context, "sqlite pool acquire timed out");
        }
        sqlx::Error::Database(db) if sqlite_busy(db.as_ref()) => {
            tracing::error!(
                %context,
                code = db.code().as_deref().unwrap_or(""),
                message = %db.message(),
                "sqlite database lock timed out"
            );
        }
        _ => {}
    }
}

fn sqlite_busy(db: &dyn sqlx::error::DatabaseError) -> bool {
    // `busy_timeout` gives up as `SQLITE_BUSY` (code 5).
    db.code().as_deref() == Some("5")
        || db.message().contains("database is locked")
        || db.message().contains("SQLITE_BUSY")
}
