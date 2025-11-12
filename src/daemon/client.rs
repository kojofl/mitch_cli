use super::DeviceMap;
use crate::{
    daemon::{DeviceCommand, device_actor::DeviceActor},
    protocol::{ClientCommand, DaemonResponse},
};
use anyhow::Result;
use btleplug::{
    api::{Central, Peripheral, ScanFilter},
    platform::Adapter,
};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::{sync::mpsc, time};

pub struct Client {
    adapter: Adapter,
    device_map: DeviceMap,
}

impl Client {
    pub fn new(adapter: Adapter, device_map: DeviceMap) -> Self {
        Self {
            adapter,
            device_map,
        }
    }

    pub async fn handle<S>(&self, mut stream: S) -> Result<DaemonResponse>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let mut len_buf = [0u8; 8];
        stream.read_exact(&mut len_buf).await?;
        let len = u64::from_le_bytes(len_buf) as usize;
        let mut command_json = vec![0; len];
        stream.read_exact(&mut command_json).await?;
        let command: ClientCommand = serde_json::from_slice(&command_json)?;

        let response = match command {
            ClientCommand::Scan { timeout_ms } => {
                self.adapter.start_scan(ScanFilter::default()).await?;
                time::sleep(Duration::from_millis(timeout_ms)).await;
                self.adapter.stop_scan().await?;
                let mut devices = Vec::new();
                for per in self.adapter.peripherals().await? {
                    let properties = per.properties().await.unwrap();
                    let n = properties.and_then(|p| p.local_name).unwrap_or_default();
                    if n.starts_with("mitch") {
                        devices.push(n);
                    }
                }
                DaemonResponse::Devices(devices)
            }

            ClientCommand::Connect { name } => self.connect(name.as_str()).await?,

            ClientCommand::Disconnect { name } => {
                println!("Daemon: Disconnecting from {}...", name);
                let mut map = self.device_map.lock().await;

                // Find the actor's channel and send Shutdown
                if let Some(tx) = map.remove(&name) {
                    // We don't care if the send fails (task might already be dead)
                    let _ = tx.send(DeviceCommand::Shutdown).await;
                    DaemonResponse::Ok
                } else {
                    DaemonResponse::Error("Device not connected".to_string())
                }
            }

            ClientCommand::Record { name } => {
                println!("Daemon: Telling {} to record...", name);
                let map = self.device_map.lock().await;

                if let Some(tx) = map.get(&name) {
                    tx.send(DeviceCommand::StartRecording {
                        lsl_stream_name: name,
                    })
                    .await?;
                    DaemonResponse::Ok
                } else {
                    DaemonResponse::Error("Device not connected".to_string())
                }
            }
        };

        let response_json = serde_json::to_vec(&response)?;
        let len = response_json.len() as u64;
        stream.write_all(&len.to_le_bytes()).await?;
        stream.write_all(&response_json).await?;
        Ok(response)
    }

    async fn connect(&self, name: &str) -> Result<DaemonResponse> {
        println!("Daemon: Connecting to {}...", name);
        // 1. Find the peripheral (this is a simplified search)
        self.adapter.start_scan(ScanFilter::default()).await?;
        time::sleep(Duration::from_secs(5)).await;
        self.adapter.stop_scan().await?;
        let mut peripheral = None;
        for per in self.adapter.peripherals().await? {
            let properties = per.properties().await.unwrap();
            let n = properties.and_then(|p| p.local_name).unwrap_or_default();
            if n == name {
                peripheral = Some(per);
            }
        }

        let Some(peripheral) = peripheral else {
            return Ok(DaemonResponse::Error(format!("{name} not found")));
        };

        // 2. Connect
        peripheral.connect().await?;
        println!("Daemon: Connected.");

        // 3. Create the actor's command channel
        let (tx, rx) = mpsc::channel(32); // 32 is a typical buffer size
        let map_clone = self.device_map.clone();

        DeviceActor::new(name, peripheral, rx, map_clone).spawn();

        // 5. Store the sender in the map
        let mut map = self.device_map.lock().await;
        map.insert(name.to_owned(), tx);

        Ok(DaemonResponse::Ok)
    }
}
