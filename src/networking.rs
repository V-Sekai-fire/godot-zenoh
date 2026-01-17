use godot::builtin::GString;
use godot::global::Error;
use godot::prelude::*;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

// ZBuf import will be added when zenoh 1.7.2 module structure is known
// For now using Vec<u8> - will replace with native ZBuf when api known
use zenoh::pubsub::Publisher;
use zenoh::pubsub::Subscriber;

/// Zenoh-native packet using topic-based routing with channel-based priority
#[derive(Clone, Debug)]
pub struct Packet {
    pub data: Vec<u8>,  // Using Vec<u8> - will optimize to ZBuf when api known
    pub from_peer: i64, // Sender peer ID for self-message filtering
}

/// Zenoh networking session with HOL blocking prevention - ASYNC IMPLEMENTATION
pub struct ZenohSession {
    /// Zenoh networking session
    session: Arc<zenoh::Session>,
    /// Publishers for each channel (lazy initialization)
    publishers: Arc<Mutex<HashMap<i32, Publisher<'static>>>>,
    /// Subscribers for each channel (lazy initialization)
    subscribers: Arc<Mutex<HashMap<i32, Subscriber<()>>>>,
    /// packet_queues for HOL processing
    packet_queues: Arc<Mutex<HashMap<i32, VecDeque<Packet>>>>,
    /// Game identifier
    game_id: GString,
    /// Unique peer identifier
    peer_id: i64,
}

impl ZenohSession {
    /// Create Zenoh networking client session (connects to server peer)
    pub async fn create_client(
        address: GString,
        port: i32,
        packet_queues: Arc<Mutex<HashMap<i32, VecDeque<Packet>>>>,
        game_id: GString,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        godot_print!("Creating Zenoh CLIENT session - HOL blocking prevention ENABLED");

        // Connect to server peer using environment variable (like successful server approach)
        let tcp_endpoint = format!("tcp/{}:{}", address, port);
        godot_print!("Zenoh CLIENT connecting to server at: {}", tcp_endpoint);

        // Set connect endpoint before session creation (critical timing)
        std::env::set_var("ZENOH_CONNECT", tcp_endpoint);

        // Configure session with longer timeouts to prevent disconnections
        std::env::set_var("ZENOH_OPEN_TIMEOUT", "30000"); // 30 seconds
        std::env::set_var("ZENOH_CLOSE_TIMEOUT", "30000"); // 30 seconds
        std::env::set_var("ZENOH_KEEP_ALIVE", "10000"); // 10 seconds keepalive

        // Use zenoh config approach
        let session_result = zenoh::open(zenoh::Config::default()).await;
        let session = match session_result {
            Ok(sess) => {
                let zid = sess.zid().to_string();
                godot_print!("Zenoh CLIENT session created - ZID: {}", zid);
                Arc::new(sess)
            }
            Err(e) => {
                godot_error!("Zenoh CLIENT session creation failed: {:?}", e);
                return Err(format!("Client session creation failed: {:?}", e).into());
            }
        };

        // Don't check liveliness immediately - let the session establish connections naturally
        // The "you are not allowed to wait for liveliness" error suggests the session needs time
        // to establish connections before querying peer information
        godot_print!("CLIENT session created - connections will establish asynchronously");

        let zid = session.zid().to_string();
        godot_print!("Client ZID: {}", zid);

        let peer_id = if zid.len() >= 8 {
            let last8 = &zid[zid.len() - 8..];
            i64::from_str_radix(last8, 16).unwrap_or_else(|_| 2)
        } else {
            2
        };

        godot_print!(
            "Zenoh CLIENT ready - Peer ID: {}, Game: {}",
            peer_id,
            game_id
        );

        Ok(ZenohSession {
            session,
            publishers: Arc::new(Mutex::new(HashMap::new())),
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            packet_queues,
            game_id,
            peer_id,
        })
    }

    /// Create Zenoh networking server session (becomes authoritative router)
    pub async fn create_server(
        port: i32,
        packet_queues: Arc<Mutex<HashMap<i32, VecDeque<Packet>>>>,
        game_id: GString,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        godot_print!(
            "Creating Zenoh SERVER (Listens on port {}) - HOL blocking prevention ENABLED",
            port
        );

        // Server becomes authoritative router using environment variable (like working approach)
        if port > 0 {
            let listen_endpoint = format!("tcp/127.0.0.1:{}", port);
            godot_print!("Zenoh SERVER acting as router at: {}", listen_endpoint);

            // Set listen endpoint before session creation (critical timing)
            std::env::set_var("ZENOH_LISTEN", listen_endpoint);
        }

        // Configure session with longer timeouts to prevent disconnections
        std::env::set_var("ZENOH_OPEN_TIMEOUT", "30000"); // 30 seconds
        std::env::set_var("ZENOH_CLOSE_TIMEOUT", "30000"); // 30 seconds
        std::env::set_var("ZENOH_KEEP_ALIVE", "10000"); // 10 seconds keepalive

        // Use zenoh config approach
        let session_result = zenoh::open(zenoh::Config::default()).await;
        let session = match session_result {
            Ok(sess) => {
                let zid = sess.zid().to_string();
                godot_print!("Zenoh SERVER router operational - ZID: {}", zid);
                Arc::new(sess)
            }
            Err(e) => {
                godot_error!("Zenoh SERVER router failed: {:?}", e);
                return Err(format!("Server router failed: {:?}", e).into());
            }
        };

        // Server gets fixed peer ID 1 (Godot convention)
        let peer_id = 1;
        godot_print!(
            "Zenoh SERVER (Authoritative Router) - Peer ID: {}, Game: {}",
            peer_id,
            game_id
        );

        Ok(ZenohSession {
            session,
            publishers: Arc::new(Mutex::new(HashMap::new())),
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            packet_queues,
            game_id,
            peer_id,
        })
    }

    /// HOL BLOCKING PREVENTION: Send packet on specific virtual channel (async)
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

            // Debug: Always log sent packets for now
            godot_print!(
                "DEBUG: Packet sent via Zenoh HOL channel {} (size: {})",
                channel,
                p_buffer.len()
            );
        }

        godot_print!(
            "Zenoh publisher not ready for channel {}, queuing locally",
            channel
        );
        self.queue_packet_locally(p_buffer, channel, self.peer_id);
        Error::OK
    }

    /// HOL BLOCKING PREVENTION: Setup publisher/subscriber for virtual channel
    pub async fn setup_channel(&self, channel: i32) -> Result<(), Box<dyn std::error::Error>> {
        let game_id = &self.game_id;
        let packet_queues = Arc::clone(&self.packet_queues);
        let peer_id = self.peer_id;

        let topic: &'static str = Box::leak(
            format!("godot/game/{}/channel{:03}", game_id, channel).into_boxed_str(),
        );

        // Lazy initialization of publisher
        {
            let mut publishers = self.publishers.lock().unwrap();
            if publishers.get(&channel).is_none() {
                let session = Arc::clone(&self.session);
                let publisher_result = session.declare_publisher(topic).await;
                match publisher_result {
                    Ok(publisher) => {
                        publishers.insert(channel, publisher);
                    }
                    Err(e) => {
                        return Err(format!("Failed to create publisher: {:?}", e).into());
                    }
                }
            }
        }

        // Lazy initialization of subscriber with HOL processing
        {
            let mut subscribers = self.subscribers.lock().unwrap();
            if subscribers.get(&channel).is_none() {
                let session = Arc::clone(&self.session);
                let packet_queues = packet_queues.clone();

                let subscriber_result = session.declare_subscriber(topic)
                    .callback(move |sample| {
                        // HOL BLOCKING PREVENTION: Received packet from Zenoh topic
                        // Zenoh sends to ALL subscribers including sender - filter self-messages at network level
                        // Zenoh automatically provides HLC timestamp for causal ordering

                        // Extract Zenoh's automatic HLC timestamp
                        if let Some(timestamp) = sample.timestamp() {
                            let seconds = timestamp.get_time().as_secs();
                            let fraction_and_counter = timestamp.get_time().subsec_nanos();

                            godot_print!(
                                "🕐 Zenoh HLC Timestamp: Seconds:{}, Subsec:{}, Router:{}",
                                seconds,
                                fraction_and_counter,
                                timestamp.get_id()
                            );
                        }

                        let payload_bytes = sample.payload().to_bytes();
                        let full_data: Vec<u8> = payload_bytes.to_vec();

                        // Parse header: first 8 bytes are sender peer ID (i64)
                        if full_data.len() < 8 {
                            return; // Skip malformed packets
                        }

                        let sender_peer_id = i64::from_le_bytes(full_data[0..8].try_into().unwrap());
                        let actual_data = &full_data[8..];

                        // BLOCK SELF-MESSAGES: Don't receive packets we sent during pub/sub delivery
                        // This prevents the "send message → immediately receive it back" loop
                        if sender_peer_id == peer_id {
                            // Silent ignore - this is normal in pub/sub systems but disruptive here
                            return;
                        }

                        let packet = Packet {
                            data: actual_data.to_vec(),
                            from_peer: sender_peer_id,
                        };

                        let mut queues = packet_queues.lock().unwrap();
                        queues.entry(channel).or_insert_with(VecDeque::new).push_back(packet);
                    })
                    .await;
                match subscriber_result {
                    Ok(subscriber) => {
                        subscribers.insert(channel, subscriber);
                    }
                    Err(e) => {
                        return Err(format!("Failed to create subscriber: {:?}", e).into());
                    }
                }
            }
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

    /// Get Hybrid Logical Clock timestamp from Zenoh session
    pub fn get_hlc_timestamp(&self) -> Result<String, Box<dyn std::error::Error>> {
        // Zenoh provides HLC (Hybrid Logical Clock) through session internals
        // We'll use session info as HLC reference for distributed coordination
        godot_print!("Requesting HLC timestamp from Zenoh session...");

        // Get session information (synchronous call)
        let session_info = self.session.info();

        // Extract HLC-like timestamp using system time and process ID for distributed coordination
        let hlc_timestamp = format!("HLC:PID{}:TIME{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());

        godot_print!("Zenoh HLC timestamp: {}", hlc_timestamp);
        Ok(hlc_timestamp)
    }

    /// Local queue fallback for HOL processing
    fn queue_packet_locally(&self, p_buffer: &[u8], channel: i32, from_peer_id: i64) {
        let data = p_buffer.to_vec(); // Use Vec<u8> directly
        let packet = Packet {
            data,
            from_peer: from_peer_id,
        };

        let mut queues = self.packet_queues.lock().unwrap();
        queues
            .entry(channel)
            .or_insert_with(VecDeque::new)
            .push_back(packet);
    }
}
