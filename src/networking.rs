// Copyright (c) 2026-present K. S. Ernest (iFire) Lee
// SPDX-License-Identifier: MIT

use godot::global::Error;
use godot::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use zenoh::pubsub::{Publisher, Subscriber};
use zenoh::handlers::FifoChannelHandler;
use zenoh::sample::Sample;
use zenoh::time::Timestamp;

/// Zenoh-native packet using topic-based routing with channel-based priority
#[derive(Clone, Debug)]
pub struct Packet {
    pub data: Vec<u8>, // Using Vec<u8> - will optimize to ZBuf when api known
    pub timestamp: Timestamp,
}

/// Received packet data for Godot interface
#[derive(Clone, Debug)]
pub struct ReceivedPacket {
    pub data: Vec<u8>,
    pub sender_peer_id: i32,
    pub channel: i32,
}

/// Zenoh networking session with channel-based topics - ASYNC IMPLEMENTATION
pub struct ZenohSession {
    session: Arc<zenoh::Session>,
    publishers: Arc<Mutex<HashMap<String, Arc<Publisher<'static>>>>>,
    subscribers: Arc<Mutex<HashMap<String, Arc<Subscriber<FifoChannelHandler<Sample>>>>>>,
    packet_queue: Arc<Mutex<Vec<ReceivedPacket>>>,
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
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            packet_queue: Arc::new(Mutex::new(Vec::new())),
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
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            packet_queue: Arc::new(Mutex::new(Vec::new())),
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

        // Setup publisher if not exists
        if !self.publishers.lock().unwrap().contains_key(topic) {
            let publisher = self.session.declare_publisher(topic).await?;
            self.publishers
                .lock()
                .unwrap()
                .insert(topic.to_string(), Arc::new(publisher));
        }

        // Setup subscriber if not exists
        if !self.subscribers.lock().unwrap().contains_key(topic) {
            let subscriber = self.session.declare_subscriber(topic).await?;
            self.subscribers
                .lock()
                .unwrap()
                .insert(topic.to_string(), Arc::new(subscriber));
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

    /// Poll all subscribers for received packets (non-blocking)
    pub async fn poll_packets(&self) -> Result<(), Box<dyn std::error::Error>> {
        let subscribers = self.subscribers.lock().unwrap().clone();

        for (topic, subscriber) in subscribers {
            // Try to receive a packet from this subscriber (non-blocking)
            match subscriber.try_recv() {
                Ok(Some(sample)) => {
                    let payload = sample.payload().to_bytes();

                    if payload.len() >= 8 {
                        // Extract sender peer ID from header (first 8 bytes)
                        let sender_peer_id = i64::from_le_bytes(payload[..8].try_into().unwrap());
                        let packet_data = payload[8..].to_vec();

                        // Parse channel from topic
                        let channel = if let Some(channel_str) = topic.split("channel").nth(1) {
                            channel_str.parse::<i32>().unwrap_or(0)
                        } else {
                            0
                        };

                        let received_packet = ReceivedPacket {
                            data: packet_data,
                            sender_peer_id: sender_peer_id as i32,
                            channel,
                        };

                        // Don't queue packets from ourselves
                        if sender_peer_id != self.peer_id {
                            self.packet_queue.lock().unwrap().push(received_packet);
                            godot_print!("Received packet from peer {} on channel {}", sender_peer_id, channel);
                        }
                    } else {
                        eprintln!("Received malformed packet: too short");
                    }
                }
                Ok(None) => {
                    // No packet available, continue
                }
                Err(e) => {
                    eprintln!("Error receiving packet: {:?}", e);
                }
            }
        }

        Ok(())
    }

    /// Get the next available packet from the queue
    pub fn get_next_packet(&self) -> Option<ReceivedPacket> {
        let mut queue = self.packet_queue.lock().unwrap();
        if !queue.is_empty() {
            Some(queue.remove(0))
        } else {
            None
        }
    }

    /// Get the number of packets available in the queue
    pub fn get_available_packet_count(&self) -> usize {
        self.packet_queue.lock().unwrap().len()
    }
}
