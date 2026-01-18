use godot::global::Error;
use godot::prelude::*;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use zenoh::pubsub::Publisher;
use zenoh::time::Timestamp;

/// Zenoh-native packet using topic-based routing with channel-based priority
#[derive(Clone, Debug)]
pub struct Packet {
    pub data: Vec<u8>,        // Packet payload data
    pub timestamp: Timestamp, // Zenoh timestamp for distributed coordination
    pub peer_id: i64,         // Sender peer ID
}

/// Zenoh networking session with channel-based topics - ASYNC IMPLEMENTATION
pub struct ZenohSession {
    /// Zenoh networking session
    session: Arc<zenoh::Session>,
    /// Publishers for each channel (lazy initialization)
    publishers: Arc<Mutex<HashMap<i32, Publisher<'static>>>>,
    /// Packet queues for each channel
    packet_queues: Arc<Mutex<HashMap<i32, VecDeque<Packet>>>>,
    /// Game identifier
    game_id: String,
    /// Unique peer identifier
    peer_id: i64,
}

impl ZenohSession {
    /// Create Zenoh networking client session (connects to server peer)
    pub async fn create_client(
        address: String,
        port: i32,
        game_id: String,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Configure session to connect to the server router
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
            i64::from_str_radix(last8, 16).unwrap_or_else(|_| 2)
        } else {
            2
        };

        Ok(ZenohSession {
            session,
            publishers: Arc::new(Mutex::new(HashMap::new())),
            packet_queues: Arc::new(Mutex::new(HashMap::new())),
            game_id,
            peer_id,
        })
    }

    pub async fn create_server(
        port: i32,
        game_id: String,
        connect_addr: Option<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Server becomes authoritative router by listening on the specified port
        let listen_endpoint = format!("tcp/127.0.0.1:{}", port);
        std::env::set_var("ZENOH_LISTEN", listen_endpoint);

        // If connect address provided, connect to another router
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
            packet_queues: Arc::new(Mutex::new(HashMap::new())),
            game_id,
            peer_id,
        })
    }

    /// Send packet on specific topic-based channel (async)
    pub async fn send_packet(&self, p_buffer: &[u8], game_id: String, channel: i32) -> Error {
        let _topic = format!("godot/game/{}/channel{:03}", game_id, channel);

        // Try to get existing publisher
        if let Some(publisher) = self.publishers.lock().unwrap().get(&channel) {
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
        if !self.publishers.lock().unwrap().contains_key(&channel) {
            let publisher = self.session.declare_publisher(topic).await?;
            self.publishers.lock().unwrap().insert(channel, publisher);
        }

        // Setup subscriber and receiver task
        {
            let subscriber = self.session.declare_subscriber(topic).await?;
            let queue_clone = Arc::clone(&self.packet_queues);
            let session_clone = Arc::clone(&self.session);
            tokio::spawn(async move {
                loop {
                    match subscriber.recv_async().await {
                        Ok(sample) => {
                            let payload = sample.payload().to_bytes().to_vec();
                            if payload.len() >= 8 {
                                let peer_id_bytes: [u8; 8] = payload[0..8].try_into().unwrap();
                                let peer_id = i64::from_le_bytes(peer_id_bytes);
                                let data = payload[8..].to_vec();
                                let timestamp = sample
                                    .timestamp()
                                    .copied()
                                    .unwrap_or_else(|| session_clone.new_timestamp());
                                let packet = Packet {
                                    data,
                                    timestamp,
                                    peer_id,
                                };
                                let mut queues = queue_clone.lock().unwrap();
                                queues
                                    .entry(channel)
                                    .or_insert(VecDeque::new())
                                    .push_back(packet);
                            }
                        }
                        Err(_) => break, // Subscriber closed or error
                    }
                }
            });
        }

        Ok(())
    }

    /// Get the peer ID for this session
    pub fn get_peer_id(&self) -> i64 {
        self.peer_id
    }

    /// Get the zenoh session ZID
    pub fn get_zid(&self) -> String {
        self.session.zid().to_string()
    }

    /// Get Zenoh timestamp
    pub fn get_timestamp(&self) -> Timestamp {
        self.session.new_timestamp()
    }

    /// Get total available packet count across all channels
    pub fn get_available_packet_count(&self) -> i32 {
        let queues = self.packet_queues.lock().unwrap();
        queues.values().map(|q| q.len() as i32).sum()
    }

    /// Get packet count for specific channel
    pub fn get_channel_packet_count(&self, channel: i32) -> i32 {
        let queues = self.packet_queues.lock().unwrap();
        queues.get(&channel).map(|q| q.len() as i32).unwrap_or(0)
    }

    /// Get next packet from specific channel
    pub fn get_packet(&self, channel: i32) -> Option<Packet> {
        let mut queues = self.packet_queues.lock().unwrap();
        if let Some(queue) = queues.get_mut(&channel) {
            queue.pop_front()
        } else {
            None
        }
    }
}
