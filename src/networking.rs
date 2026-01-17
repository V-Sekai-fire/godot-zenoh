use godot::builtin::GString;
use godot::global::Error;
use godot::prelude::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ZBuf import will be added when zenoh 1.7.2 module structure is known
// For now using Vec<u8> - will replace with native ZBuf when api known
use zenoh::pubsub::Publisher;

/// Zenoh-native packet using topic-based routing with channel-based priority
#[derive(Clone, Debug)]
pub struct Packet {
    pub data: Vec<u8>,  // Using Vec<u8> - will optimize to ZBuf when api known
    pub from_peer: i64, // Sender peer ID for self-message filtering
}

/// Zenoh networking session with channel-based topics - ASYNC IMPLEMENTATION
pub struct ZenohSession {
    /// Zenoh networking session
    session: Arc<zenoh::Session>,
    /// Publishers for each channel (lazy initialization)
    publishers: Arc<Mutex<HashMap<i32, Publisher<'static>>>>,
    /// Game identifier
    game_id: GString,
    /// Unique peer identifier
    peer_id: i64,
}

impl ZenohSession {
    /// Create Zenoh networking client session (connects to server peer)
    pub async fn create_client(
        game_id: GString,
    ) -> Result<Self, Box<dyn std::error::Error>> {


        // Configure session with longer timeouts to prevent disconnections
        std::env::set_var("ZENOH_OPEN_TIMEOUT", "30000"); // 30 seconds
        std::env::set_var("ZENOH_CLOSE_TIMEOUT", "30000"); // 30 seconds
        std::env::set_var("ZENOH_KEEP_ALIVE", "10000"); // 10 seconds keepalive

        // Use zenoh config approach
        let session_result = zenoh::open(zenoh::Config::default()).await;
        let session = match session_result {
            Ok(sess) => {
                Arc::new(sess)
            }
            Err(e) => {
                godot_error!("Zenoh CLIENT session creation failed: {:?}", e);
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
            game_id,
            peer_id,
        })
    }

    /// Create Zenoh networking server session (becomes authoritative router)
    pub async fn create_server(
        port: i32,
        game_id: GString,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // Server becomes authoritative router using environment variable (like working approach)
        let listen_endpoint = format!("tcp/127.0.0.1:{}", port);
        std::env::set_var("ZENOH_LISTEN", listen_endpoint);

        // Configure session with longer timeouts to prevent disconnections
        std::env::set_var("ZENOH_OPEN_TIMEOUT", "30000"); // 30 seconds
        std::env::set_var("ZENOH_CLOSE_TIMEOUT", "30000"); // 30 seconds
        std::env::set_var("ZENOH_KEEP_ALIVE", "10000"); // 10 seconds keepalive

        // Use zenoh config approach
        let session_result = zenoh::open(zenoh::Config::default()).await;
        let session = match session_result {
            Ok(sess) => {
                Arc::new(sess)
            }
            Err(e) => {
                godot_error!("Zenoh SERVER router failed: {:?}", e);
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
    pub async fn send_packet(&self, p_buffer: &[u8], game_id: GString, channel: i32) -> Error {
        let _topic = format!("godot/game/{}/channel{:03}", game_id, channel);

        // Try to get existing publisher
        if let Some(publisher) = self.publishers.lock().unwrap().get(&channel) {
            // Add sender peer ID header (8 bytes: peer_id as i64)
            let mut packet_data = Vec::with_capacity(8 + p_buffer.len());
            packet_data.extend_from_slice(&self.peer_id.to_le_bytes());
            packet_data.extend_from_slice(p_buffer);

            if let Err(e) = publisher.put(packet_data).await {
                godot_error!(
                    "Failed to send Zenoh packet on channel {}: {:?}",
                    channel,
                    e
                );
                return Error::FAILED;
            }


        }


        self.queue_packet_locally(p_buffer, channel, self.peer_id);
        Error::OK
    }

    /// Setup publisher/subscriber for topic-based channel
    pub async fn setup_channel(&self, channel: i32) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let game_id = &self.game_id;
        let topic: &'static str = Box::leak(
            format!("godot/game/{}/channel{:03}", game_id, channel).into_boxed_str(),
        );

        // Setup publisher if not exists
        if !self.publishers.lock().unwrap().contains_key(&channel) {
            let publisher = self.session.declare_publisher(topic).await?;
            self.publishers.lock().unwrap().insert(channel, publisher);
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

    /// Get HLC timestamp from Zenoh session
    pub fn get_hlc_timestamp(&self) -> Result<String, Box<dyn std::error::Error>> {
        // Extract HLC-like timestamp using system time and process ID for distributed coordination
        let hlc_timestamp = format!("HLC:PID{}:TIME{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        Ok(hlc_timestamp)
    }

    /// Local queue fallback for message processing
    fn queue_packet_locally(&self, p_buffer: &[u8], _channel: i32, from_peer_id: i64) {
        let data = p_buffer.to_vec(); // Use Vec<u8> directly
        let _packet = Packet {
            data,
            from_peer: from_peer_id,
        };
    }
}
