use crate::protocol::IPC_SOCKET_PATH;
use anyhow::Result;
use btleplug::api::Manager as _;
use btleplug::platform::{Adapter, Manager};
use client::Client;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
#[cfg(unix)]
use tokio::net::UnixListener;
#[cfg(windows)]
use tokio::net::windows::named_pipe::ServerOptions;
use tokio::sync::{Mutex, mpsc};

mod client;
mod device_actor;

type DeviceMap = Arc<Mutex<HashMap<String, mpsc::Sender<DeviceCommand>>>>;

enum DeviceCommand {
    StartRecording { lsl_stream_name: String },
    Shutdown,
}

pub struct Daemon {
    adapter: Adapter,
    device_map: DeviceMap,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        println!("Daemon stopping attempting to free resources")
    }
}

impl Daemon {
    pub async fn new() -> Result<Self> {
        let manager = Manager::new().await?;
        let adapter = manager.adapters().await?.into_iter().next().unwrap();
        let device_map = DeviceMap::new(Mutex::new(HashMap::new()));
        Ok(Self {
            adapter,
            device_map,
        })
    }
    pub async fn run(&self) -> Result<()> {
        println!("Daemon listening on {}", IPC_SOCKET_PATH);

        #[cfg(unix)]
        {
            let _ = tokio::fs::remove_file(IPC_SOCKET_PATH).await;
            let listener = UnixListener::bind(IPC_SOCKET_PATH)?;

            loop {
                match listener.accept().await {
                    Ok((mut stream, _addr)) => {
                        println!("Client connected.");
                        let device_map_clone = self.device_map.clone();
                        let adapter_clone = self.adapter.clone();

                        // Spawn a task to handle this client
                        tokio::task::spawn_local(async move {
                            if let Err(e) = Client::new(adapter_clone, device_map_clone)
                                .handle(&mut stream)
                                .await
                            {
                                eprintln!("Client error: {}", e);
                            }
                            println!("Client disconnected.");
                        });
                    }
                    Err(e) => eprintln!("Failed to accept client: {}", e),
                }
            }
        }

        #[cfg(windows)]
        {
            // WINDOWS: Loop creating new pipe instances
            loop {
                // Create a new pipe instance for the next client.
                let mut server = ServerOptions::new().create(IPC_SOCKET_PATH)?;

                // Wait for a client to connect to this specific instance
                server.connect().await?;
                println!("Client connected.");

                let device_map_clone = self.device_map.clone();
                let adapter_clone = self.adapter.clone();

                // Spawn a task to handle this client
                // The `server` object itself is the stream
                tokio::spawn(async move {
                    if let Err(e) = Client::new(adapter_clone, device_map_clone)
                        .handle(&mut server)
                        .await
                    {
                        eprintln!("Client error: {}", e);
                    }
                    println!("Client disconnected.");
                });
            }
        }
    }
}
