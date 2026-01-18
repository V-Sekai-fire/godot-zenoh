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
            game_id,
            peer_id,
        })
    }

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
