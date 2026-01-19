// Copyright (c) 2026-present K. S. Ernest (iFire) Lee
// SPDX-License-Identifier: MIT

use godot::global::Error;
use godot::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use zenoh::pubsub::Publisher;
use zenoh::time::Timestamp;

/// Maximum allowed future timestamp offset for HLC leash (5 seconds in nanoseconds)
/// Prevents excessive clock skew in distributed systems by clamping timestamps too far in the future
/// Based on FoundationDB/CockroachDB: clamp rather than reject to preserve availability
const HLC_MAX_FUTURE_OFFSET_NANOS: i64 = 5_000_000_000; // 5 seconds

/// Validate HLC timestamp against leash bounds. Clamps future timestamps to prevent excessive skew.
/// Based on FoundationDB approach: never reject, always clamp to maintain availability.
/// Returns validated timestamp (may be clamped) and whether clamping occurred.
/// Public for testing the leash enforcement in property tests.
pub fn validate_hlc_timestamp(incoming: Timestamp, current: Timestamp) -> (Timestamp, bool) {
    let incoming_nanos = incoming.get_time().as_nanos() as i64;
    let current_nanos = current.get_time().as_nanos() as i64;

    let offset = incoming_nanos - current_nanos;

    if offset > HLC_MAX_FUTURE_OFFSET_NANOS {
        // Clamp to maximum allowed future timestamp by reusing current ID
        // FoundationDB approach: clamp rather than reject for availability
        let _clamped_nanos = current_nanos + HLC_MAX_FUTURE_OFFSET_NANOS;
        godot_print!(
            "HLC LEASH: Would clamp future timestamp {}ns -> {}ns ({}s off) - peer ID: {}",
            offset,
            HLC_MAX_FUTURE_OFFSET_NANOS,
            offset / 1_000_000_000,
            incoming.get_id()
        );

        // For now, use current timestamp as clamped version (TODO: proper clamping)
        // This ensures linearizability bounds are violated gracefully
        (current, true) // Return current timestamp and flag that clamping occurred
    } else {
        (incoming, false) // Valid timestamp, no clamping
    }
}

/// Zenoh-native packet using topic-based routing with channel-based priority
#[derive(Clone, Debug)]
pub struct Packet {
    pub data: Vec<u8>, // Using Vec<u8> - will optimize to ZBuf when api known
    pub timestamp: Timestamp,
}

/// Message delivery callback type for bridging networking to peer layer
pub type MessageCallback = Box<dyn Fn(PackedByteArray, i32, i32) + Send + Sync>;

/// Zenoh networking session with channel-based topics - ASYNC IMPLEMENTATION
pub struct ZenohSession {
    session: Arc<zenoh::Session>,
    publishers: Arc<Mutex<HashMap<String, Arc<Publisher<'static>>>>>,
    subscribers_initialized: Arc<Mutex<std::collections::HashSet<String>>>,
    message_queue: Arc<Mutex<Vec<Packet>>>,
    message_callback: Option<Arc<MessageCallback>>, // FIXED: Callback to deliver messages to peer
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
            message_queue: Arc::new(Mutex::new(Vec::new())),
            message_callback: None,
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
            message_queue: Arc::new(Mutex::new(Vec::new())),
            message_callback: None,
            game_id,
            peer_id,
        })
    }

    /// Set callback for message delivery to peer layer (FIXED: Critical message flow wiring)
    pub fn set_message_callback(&mut self, callback: MessageCallback) {
        self.message_callback = Some(Arc::new(callback));
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

        godot_print!(
            "DEBUG: Setting up channel {} with topic: {}",
            channel,
            topic
        );

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
            godot_print!("== Creating subscriber for topic {}", topic);
            let subscriber = self.session.declare_subscriber(topic).await?;
            godot_print!("== Subscriber created, setting up message forwarding");

            // Get references for message delivery
            let message_queue = self.message_queue.clone();
            let message_callback = self.message_callback.as_ref().cloned(); // FIXED: Get callback for peer delivery
            let sess = Arc::clone(&self.session);
            let current_channel = channel; // Capture channel for delivery

            // Spawn task to handle incoming messages and forward to Godot peer
            tokio::spawn(async move {
                let mut recv_counter = 0;
                loop {
                    match subscriber.recv_async().await {
                        Ok(sample) => {
                            recv_counter += 1;
                            let payload = sample.payload();
                            godot_print!(
                                "== RECEIVED #{}: {} bytes on topic {}",
                                recv_counter,
                                payload.len(),
                                &topic
                            );

                            // Parse message format and extract data portion
                            // Zenoh messages have 8-byte sender peer_id header, then payload
                            if payload.len() >= 8 {
                                // Convert ZBytes to Vec<u8> for processing
                                let payload_bytes = payload.to_bytes().to_vec();

                                // Extract payload data after 8-byte peer_id header
                                let message_data: Vec<u8> = payload_bytes[8..].to_vec();

                                // Extract sender peer ID from header
                                let sender_peer_bytes = &payload_bytes[0..8];
                                let sender_peer_id = i64::from_le_bytes(
                                    sender_peer_bytes.try_into().unwrap_or([0; 8]),
                                );

                                // HLC LEASH: Validate and clamp timestamp to prevent clock skew
                                let incoming_timestamp = sample
                                    .timestamp()
                                    .copied()
                                    .unwrap_or_else(|| sess.new_timestamp());
                                let current_timestamp = sess.new_timestamp();
                                let (validated_timestamp, _was_clamped) =
                                    validate_hlc_timestamp(incoming_timestamp, current_timestamp);

                                // FIXED: CRITICAL MESSAGE FLOW - Deliver to Godot peer via callback
                                // This bridges the networking.rssubscriber -> peer.rs get_packet() pipeline
                                if let Some(ref callback) = message_callback {
                                    let godot_packet =
                                        PackedByteArray::from(message_data.as_slice());
                                    callback(godot_packet, current_channel, sender_peer_id as i32);
                                    godot_print!(
                                        "== DELIVERED: packet with {} data bytes from peer {} on channel {} to Godot peer",
                                        message_data.len(),
                                        sender_peer_id,
                                        current_channel
                                    );
                                } else {
                                    // Fallback to local queue if no callback set - use validated timestamp
                                    let packet = Packet {
                                        data: message_data.clone(),
                                        timestamp: validated_timestamp,
                                    };

                                    {
                                        let mut queue = message_queue.lock().unwrap();
                                        queue.push(packet);
                                    }

                                    godot_print!(
                                        "== QUEUED: packet with {} data bytes from peer {} queued locally (no callback)",
                                        message_data.len(),
                                        sender_peer_id
                                    );
                                }
                            } else {
                                godot_error!(
                                    "== Message too short (len={}), skipping",
                                    payload.len()
                                );
                            }
                        }
                        Err(e) => {
                            godot_error!("== Subscriber error on {}: {:?}", &topic, e);
                            break;
                        }
                    }
                }
            });

            self.subscribers_initialized
                .lock()
                .unwrap()
                .insert(topic.to_string());
            godot_print!("== Subscriber setup complete for topic {}", topic);
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
