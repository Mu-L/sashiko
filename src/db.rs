use crate::settings::DatabaseSettings;
use anyhow::Result;
use libsql::Builder;
use tracing::info;

pub struct Database {
    pub conn: libsql::Connection,
}

impl Database {
    pub async fn new(settings: DatabaseSettings) -> Result<Self> {
        info!("Connecting to database at {}", settings.url);

        let db = if settings.url.starts_with("libsql://") || settings.url.starts_with("https://") {
            Builder::new_remote(settings.url, settings.token)
                .build()
                .await?
        } else {
            Builder::new_local(&settings.url).build().await?
        };

        let conn = db.connect()?;

        Ok(Self { conn })
    }

    pub async fn migrate(&self) -> Result<()> {
        let schema = include_str!("schema.sql");
        self.conn.execute_batch(schema).await?;
        info!("Database schema applied");
        Ok(())
    }
}
