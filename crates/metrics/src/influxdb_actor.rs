use eyre::{Result, eyre};
use influxdb::{Client, ReadQuery, WriteQuery};
use kabu_core_components::Component;
use reth_tasks::TaskExecutor;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::timeout;
use tracing::{error, info, warn};

pub async fn start_influxdb_worker(
    url: String,
    database: String,
    tags: HashMap<String, String>,
    mut event_receiver: broadcast::Receiver<WriteQuery>,
) {
    let client = Client::new(url, database.clone());
    let create_db_stmt = format!("CREATE DATABASE {database}");
    let result = client.query(ReadQuery::new(create_db_stmt)).await;
    match result {
        Ok(_) => info!("Database created with name: {}", database),
        Err(e) => info!("Database creation failed or already exists: {:?}", e),
    }
    // event_receiver is already a Receiver
    loop {
        let event_result = event_receiver.recv().await;
        match event_result {
            Ok(mut event) => {
                for (key, value) in tags.iter() {
                    event = event.add_tag(key, value.clone());
                }
                let client_clone = client.clone();
                tokio::task::spawn(async move {
                    match timeout(Duration::from_millis(2000), client_clone.query(event)).await {
                        Ok(inner_result) => {
                            if let Err(e) = inner_result {
                                error!("InfluxDB Write failed: {:?}", e);
                            }
                        }
                        Err(elapsed) => {
                            error!("InfluxDB Query timed out: {}", elapsed);
                        }
                    }
                });
            }
            Err(e) => match e {
                tokio::sync::broadcast::error::RecvError::Closed => {
                    error!("InfluxDB channel closed");
                    break;
                }
                tokio::sync::broadcast::error::RecvError::Lagged(lagged) => {
                    warn!("InfluxDB lagged: {:?}", lagged);
                    continue;
                }
            },
        }
    }
}

#[derive(Clone)]
pub struct InfluxDbWriterComponent {
    url: String,
    database: String,
    tags: HashMap<String, String>,
    influxdb_write_channel_rx: Option<broadcast::Sender<WriteQuery>>,
}

impl InfluxDbWriterComponent {
    pub fn new(url: String, database: String, tags: HashMap<String, String>) -> Self {
        Self { url, database, tags, influxdb_write_channel_rx: None }
    }

    pub fn with_channel(self, channel: Option<broadcast::Sender<WriteQuery>>) -> Self {
        Self { influxdb_write_channel_rx: channel, ..self }
    }
}

impl Component for InfluxDbWriterComponent {
    fn spawn(self, executor: TaskExecutor) -> Result<()> {
        let name = self.name();

        let influxdb_write_channel_rx = self.influxdb_write_channel_rx.ok_or_else(|| eyre!("InfluxDB write channel is not set"))?;

        let event_receiver = influxdb_write_channel_rx.subscribe();

        executor.spawn_critical(name, start_influxdb_worker(self.url, self.database, self.tags, event_receiver));

        Ok(())
    }

    fn name(&self) -> &'static str {
        "InfluxDbWriterComponent"
    }
}
