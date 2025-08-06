use diesel::pg::PgConnection;
use diesel::Connection;
use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::AsyncPgConnection;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness};
use thiserror::Error;
use tracing::info;

#[derive(Debug, Error)]
pub enum PoolError {
    #[error("Failed to create connection pool: {0}")]
    PoolError(String),
    #[error("Failed to run migrations: {0}")]
    MigrationError(String),
}

pub type DbPool = bb8::Pool<AsyncDieselConnectionManager<AsyncPgConnection>>;

pub async fn init_db_pool_with_migrations(db_url: String, migrations: EmbeddedMigrations) -> Result<DbPool, PoolError> {
    // First, check and run migrations using a synchronous connection
    check_and_run_migrations(&db_url, migrations)?;

    // set up connection pool using bb8 with diesel-async manager
    let manager = AsyncDieselConnectionManager::<AsyncPgConnection>::new(db_url);
    let pool = bb8::Pool::builder().build(manager).await.map_err(|e| PoolError::PoolError(e.to_string()))?;
    Ok(pool)
}

pub fn check_and_run_migrations(db_url: &str, migrations: EmbeddedMigrations) -> Result<(), PoolError> {
    let mut conn =
        PgConnection::establish(db_url).map_err(|e| PoolError::MigrationError(format!("Failed to establish connection: {e}")))?;

    info!("Checking and applying database migrations...");

    // Run migrations - this will check for pending and apply them
    match conn.run_pending_migrations(migrations) {
        Ok(applied) => {
            if applied.is_empty() {
                info!("Database is up to date - no pending migrations");
            } else {
                info!("Successfully applied {} migration(s):", applied.len());
                for (i, migration) in applied.iter().enumerate() {
                    info!("  {}. {}", i + 1, migration);
                }
            }
        }
        Err(e) => {
            return Err(PoolError::MigrationError(format!("Failed to run migrations: {e}")));
        }
    }

    // Always show the latest migration
    if let Ok(applied_migrations) = conn.applied_migrations() {
        if let Some(latest) = applied_migrations.last() {
            info!("Latest migration: {}", latest);
        } else {
            info!("No migrations have been applied yet");
        }
    }

    Ok(())
}
