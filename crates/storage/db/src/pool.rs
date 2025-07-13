use diesel_async::pooled_connection::AsyncDieselConnectionManager;
use diesel_async::AsyncPgConnection;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PoolError {
    #[error("Failed to create connection pool: {0}")]
    PoolError(String),
}

pub type DbPool = bb8::Pool<AsyncDieselConnectionManager<AsyncPgConnection>>;

pub async fn init_db_pool(db_url: String) -> Result<DbPool, PoolError> {
    // set up connection pool using bb8 with diesel-async manager
    let manager = AsyncDieselConnectionManager::<AsyncPgConnection>::new(db_url);
    let pool = bb8::Pool::builder().build(manager).await.map_err(|e| PoolError::PoolError(e.to_string()))?;
    Ok(pool)
}
