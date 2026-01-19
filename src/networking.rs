// Copyright (c) 2026-present K. S. Ernest (iFire) Lee
// SPDX-License-Identifier: MIT

use godot::global::Error;
use godot::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use zenoh::pubsub::Publisher;
use zenoh::time::Timestamp;

/// Zenoh-native packet using topic-based routing with channel-based priority
#[derive(Clone, Debug)]
pub struct Packet {
    pub data: Vec<u8>, // Using Vec<u8> - will optimize to ZBuf when api known
    pub timestamp: Timestamp,
}

/// Zenoh networking session with channel-based topics - ASYNC IMPLEMENTATION
pub struct ZenohSession {
    session: Arc<zenoh::Session>,
    publishers: Arc<Mutex<HashMap<String, Arc<Publisher<'static>>>>>,
    subscribers_initialized: Arc<Mutex<std::collections::HashSet<String>>>,
    game_id: String,
    peer_id: i64,
}

impl ZenohSession {
    /// Create Zenoh networking client session (connects to server peer)
    pub async fn create_client(
        address: String,
        port: i32,
        game_id: String,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let connect_endpoint = format!("tcp/{}:{}", address, port);
        std::env::set_var("ZENOH_CONNECT", connect_endpoint);

        // Configure session with longer timeouts to prevent disconnections
        std::env::set_var("ZENOH_OPEN_TIMEOUT", "30000"); // 30 seconds
        std::env::set_var("ZENOH_CLOSE_TIMEOUT", "30000"); // 30 seconds
        std::env::set_var("ZENOH_KEEP_ALIVE", "10000"); // 10 seconds keepalive

        let session_result = zenoh::open(zenoh::Config::default()).await;
        let session = match session_result {
            Ok(sess) => Arc::new(sess),
            Err(e) => {
                eprintln!("Zenoh CLIENT session creation failed: {:?}", e);
                return Err(format!("Client session creation failed: {:?}", e).into());
            }
        };

        let zid = session.zid().to_string();

        let peer_id = if zid.len() >= 8 {
            let last8 = &zid[zid.len() - 8..];
            i64::from_str_radix(last8, 16).unwrap_or(2)
        } else {
            2
        };

        Ok(ZenohSession {
            session,
            publishers: Arc::new(Mutex::new(HashMap::new())),
            subscribers_initialized: Arc::new(Mutex::new(std::collections::HashSet::new())),
            game_id,
            peer_id,
        })
    }

    /// Create Zenoh networking server session (listens for client connections)
    ///
    /// # Arguments
    /// * `port` - The port to listen on
    /// * `game_id` - Identifier for network isolation
    /// * `connect_addr` - Optional address to connect to (for hybrid mode)
    pub async fn create_server(
        port: i32,
        game_id: String,
        connect_addr: Option<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let listen_endpoint = format!("tcp/127.0.0.1:{}", port);
        std::env::set_var("ZENOH_LISTEN", listen_endpoint);

        if let Some(addr) = connect_addr {
            std::env::set_var("ZENOH_CONNECT", addr);
        }

        // Configure session with longer timeouts to prevent disconnections
        std::env::set_var("ZENOH_OPEN_TIMEOUT", "30000"); // 30 seconds
        std::env::set_var("ZENOH_CLOSE_TIMEOUT", "30000"); // 30 seconds
        std::env::set_var("ZENOH_KEEP_ALIVE", "10000"); // 10 seconds keepalive

        let session_result = zenoh::open(zenoh::Config::default()).await;
        let session = match session_result {
            Ok(sess) => Arc::new(sess),
            Err(e) => {
                eprintln!("Zenoh SERVER router failed: {:?}", e);
                return Err(format!("Server router failed: {:?}", e).into());
            }
        };

        // Server gets fixed peer ID 1 (Godot convention)
        let peer_id = 1;

        Ok(ZenohSession {
            session,
            publishers: Arc::new(Mutex::new(HashMap::new())),
            subscribers_initialized: Arc::new(Mutex::new(std::collections::HashSet::new())),
            game_id,
            peer_id,
        })
    }

    /// Send packet on specific topic-based channel (async)
    pub async fn send_packet(&self, p_buffer: &[u8], game_id: String, channel: i32) -> Error {
        let topic = format!("godot/game/{}/channel{:03}", game_id, channel);

        // Try to get existing publisher
        let publisher = {
            let publishers = self.publishers.lock().unwrap();
            publishers.get(&topic).cloned()
        };

        if let Some(publisher) = publisher {
            // Add sender peer ID header (8 bytes: peer_id as i64)
            let mut packet_data = Vec::with_capacity(8 + p_buffer.len());
            packet_data.extend_from_slice(&self.peer_id.to_le_bytes());
            packet_data.extend_from_slice(p_buffer);

            if let Err(e) = publisher.put(packet_data).await {
                eprintln!(
                    "Failed to send Zenoh packet on channel {}: {:?}",
                    channel, e
                );
                return Error::FAILED;
            }
        } else {
            godot_error!("No publisher available for channel {}", channel);
            return Error::FAILED;
        }

        Error::OK
    }

    /// Setup publisher/subscriber for topic-based channel
    pub async fn setup_channel(
        &self,
        channel: i32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let game_id = &self.game_id;
        let topic: &'static str =
            Box::leak(format!("godot/game/{}/channel{:03}", game_id, channel).into_boxed_str());

        godot_print!("DEBUG: Setting up channel {} with topic: {}", channel, topic);

        // Setup publisher if not exists
        if !self.publishers.lock().unwrap().contains_key(topic) {
            godot_print!("DEBUG: Creating publisher for topic {}", topic);
            let publisher = self.session.declare_publisher(topic).await?;
            self.publishers
                .lock()
                .unwrap()
                .insert(topic.to_string(), Arc::new(publisher));
            godot_print!("DEBUG: Publisher created successfully");
        }

        // Setup subscriber for bi-directional communication
        if !self.subscribers_initialized.lock().unwrap().contains(topic) {
            godot_print!("=' Creating subscriber for topic {}", topic);
            let subscriber = self.session.declare_subscriber(topic).await?;
            godot_print!("=á Subscriber created, setting up message forwarding");

            // Get reference to message queue for delivery
            let message_queue = self.message_queue.clone();

            // Spawn task to handle incoming messages and forward to Godot peer
            tokio::spawn(async move {
                let mut recv_counter = 0;
                loop {
                    match subscriber.recv_async().await {
                        Ok(sample) => {
                            recv_counter += 1;
                            let payload = sample.payload();
                            godot_print!("=è RECEIVED #{}, {} bytes on topic {}", recv_counter, payload.len(), &topic);

                            // Parse message format and extract data portion
                            // Zenoh messages have 8-byte sender peer_id header, then payload
                            if payload.len() >= 8 {
                                // Extract data (skip peer_id header)
                                let message_data = payload.slice(8..);

                                // Create Packet for Godot
                                let packet = Packet {
                                    data: message_data.to_vec(),
                                    timestamp: sample.timestamp().unwrap_or_default(),
                                };

                                // Queue message for Godot peer
                                message_queue.lock().unwrap().push(packet);
                                godot_print!(" Queued message for Godot peer delivery");
                            } else {
                                godot_error!("L Message too short (len={}), skipping", payload.len());
                            }
                        }
                        Err(e) => {
                            godot_error!("L Subscriber error on {}: {:?}", &topic, e);
                            break;
                        }
                    }
                }
            });

            self.subscribers_initialized.lock().unwrap().insert(topic.to_string());
            godot_print!(" Subscriber setup complete for topic {}", topic);
        }

        Ok(())
    }

    pub fn get_peer_id(&self) -> i64 {
        self.peer_id
    }

    pub fn get_zid(&self) -> String {
        self.session.zid().to_string()
    }

    pub fn get_timestamp(&self) -> Timestamp {
        self.session.new_timestamp()
    }
}