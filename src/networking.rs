use godot::builtin::GString;
use godot::global::Error;
use godot::prelude::*;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::runtime::{Builder, Runtime};
// ZBuf import will be added when zenoh 1.7.2 module structure is known
// For now using Vec<u8> - will replace with native ZBuf when api known
use zenoh::pubsub::Publisher;
use zenoh::pubsub::Subscriber;

/// Zenoh-native packet using topic-based routing with channel-based priority
#[derive(Clone, Debug)]
pub struct Packet {
    pub data: Vec<u8>, // Using Vec<u8> - will optimize to ZBuf when api known
}

/// Zenoh networking session with HOL blocking prevention - ASYNC IMPLEMENTATION
pub struct ZenohSession {
    /// Zenoh networking session
    session: Arc<zenoh::Session>,
    /// Async runtime for Zenoh operations
    runtime: Runtime,
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
        godot_print!("🎯 Creating Zenoh CLIENT session - HOL blocking prevention ENABLED");

        // Connect to server peer using environment variable (like successful server approach)
        let tcp_endpoint = format!("tcp/{}:{}", address, port);
        godot_print!("🔌 Zenoh CLIENT connecting to server at: {}", tcp_endpoint);

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
                godot_print!("✅ Zenoh CLIENT session created - ZID: {}", zid);
                Arc::new(sess)
            }
            Err(e) => {
                godot_error!("❌ Zenoh CLIENT session creation failed: {:?}", e);
                return Err(format!("Client session creation failed: {:?}", e).into());
            }
        };

        // Don't check liveliness immediately - let the session establish connections naturally
        // The "you are not allowed to wait for liveliness" error suggests the session needs time
        // to establish connections before querying peer information
        godot_print!("ℹ️ CLIENT session created - connections will establish asynchronously");

        let zid = session.zid().to_string();
        godot_print!("🌐 Client ZID: {}", zid);

        let peer_id = if zid.len() >= 8 {
            let last8 = &zid[zid.len()-8..];
            i64::from_str_radix(last8, 16).unwrap_or_else(|_| 2)
        } else {
            2
        };

        godot_print!("✅ Zenoh CLIENT ready - Peer ID: {}, Game: {}", peer_id, game_id);

        Ok(ZenohSession {
            session,
            runtime: Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap(),
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
            "🎯 Creating Zenoh SERVER (Listens on port {}) - HOL blocking prevention ENABLED",
            port
        );

        // Server becomes authoritative router using environment variable (like working approach)
        if port > 0 {
            let listen_endpoint = format!("tcp/127.0.0.1:{}", port);
            godot_print!("🛡️ Zenoh SERVER acting as router at: {}", listen_endpoint);

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
                godot_print!("✅ Zenoh SERVER router operational - ZID: {}", zid);
                Arc::new(sess)
            }
            Err(e) => {
                godot_error!("❌ Zenoh SERVER router failed: {:?}", e);
                return Err(format!("Server router failed: {:?}", e).into());
            }
        };

        // Server gets fixed peer ID 1 (Godot convention)
        godot_print!("✅ Zenoh SERVER (Authoritative Router) - Peer ID: 1, Game: {}", game_id);

        Ok(ZenohSession {
            session,
            runtime: Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap(),
            publishers: Arc::new(Mutex::new(HashMap::new())),
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            packet_queues,
            game_id,
            peer_id: 1, // Server is always peer 1
        })
    }

    /// HOL BLOCKING PREVENTION: Send packet on specific virtual channel (async)
    pub async fn send_packet(&self, p_buffer: &[u8], game_id: GString, channel: i32) -> Error {
        let _topic = format!("godot/game/{}/channel{:03}", game_id, channel);

        // Try to get existing publisher
        if let Some(publisher) = self.publishers.lock().unwrap().get(&channel) {
            let data = p_buffer.to_vec(); // Use Vec<u8> directly

            if let Err(e) = publisher.put(data).await {
                godot_error!(
                    "Failed to send Zenoh packet on channel {}: {:?}",
                    channel,
                    e
                );
                return Error::FAILED;
            }

            // Debug: Always log sent packets for now
            godot_print!(
                "📤 DEBUG: Packet sent via Zenoh HOL channel {} (size: {})",
                channel,
                p_buffer.len()
            );
        }

        godot_print!(
            "⚠️ Zenoh publisher not ready for channel {}, queuing locally",
            channel
        );
        self.queue_packet_locally(p_buffer, channel, self.peer_id);
        Error::OK
    }

    /// HOL BLOCKING PREVENTION: Setup publisher/subscriber for virtual channel
    pub fn setup_channel(&self, channel: i32) -> Error {
        let game_id = &self.game_id;
        let packet_queues = Arc::clone(&self.packet_queues);
        let peer_id = self.peer_id;

        // Only log for channel 0 and every 50th channel to reduce spam
        if channel == 0 || channel % 50 == 0 {
            godot_print!(
                "🎛️ Setting up Zenoh HOL virtual channel {} for peer {}",
                channel,
                peer_id
            );
        }

        // Lazy initialization of publisher
        {
            let mut publishers = self.publishers.lock().unwrap();
            if publishers.get(&channel).is_none() {
                let session = Arc::clone(&self.session);
                let publisher_result = self.runtime.block_on(async move {
                    let topic: &'static str = Box::leak(
                        format!("godot/game/{}/channel{:03}", game_id, channel).into_boxed_str(),
                    );
                    session.declare_publisher(topic).await
                });

                match publisher_result {
                    Ok(publisher) => {
                        publishers.insert(channel, publisher);
                        // Only log success for channel 0 and every 50th channel
                        if channel == 0 || channel % 50 == 0 {
                            godot_print!("✅ Zenoh publisher created for HOL channel {}", channel);
                        }
                    }
                    Err(e) => {
                        godot_error!(
                            "❌ Failed to create Zenoh publisher for channel {}: {:?}",
                            channel,
                            e
                        );
                        return Error::FAILED;
                    }
                }
            }
        }

        // Lazy initialization of subscriber with HOL processing
        {
            let mut subscribers = self.subscribers.lock().unwrap();
            if subscribers.get(&channel).is_none() {
                let session = Arc::clone(&self.session);

                let subscriber_result = self.runtime.block_on(async {
                    let topic: &'static str = Box::leak(format!("godot/game/{}/channel{:03}", game_id, channel).into_boxed_str());
                    let _packet_queues = packet_queues.clone();

                    session.declare_subscriber(topic)
                        .callback(move |sample| {
                            // HOL BLOCKING PREVENTION: Received packet from Zenoh topic
                            // Use zenoh 1.7.2 payload conversion methods
                            let payload_bytes = sample.payload().to_bytes();
                            let data: Vec<u8> = payload_bytes.to_vec();
                            let topic_str = sample.key_expr().as_str();
                            let hol_priority = channel; // Extract from topic or use channel mapping

                            // Debug: Always log received packets for now
                            godot_print!("📥 DEBUG: HOL PREVENTION: RECEIVED PACKET on topic '{}' (channel: {}, size: {})",
                                   topic_str, hol_priority, sample.payload().len());

                            let packet = Packet {
                                data,
                            };

                            let mut queues = _packet_queues.lock().unwrap();
                            queues.entry(channel).or_insert_with(VecDeque::new).push_back(packet);
                        })
                        .await
                });

                match subscriber_result {
                    Ok(subscriber) => {
                        subscribers.insert(channel, subscriber);
                        // Only log success for channel 0 and every 50th channel
                        if channel == 0 || channel % 50 == 0 {
                            godot_print!("✅ Zenoh subscriber created for HOL channel {}", channel);
                        }
                    }
                    Err(e) => {
                        godot_error!(
                            "❌ Failed to create Zenoh subscriber for channel {}: {:?}",
                            channel,
                            e
                        );
                        return Error::FAILED;
                    }
                }
            }
        }

        Error::OK
    }

    /// Get the peer ID for this session
    pub fn get_peer_id(&self) -> i64 {
        self.peer_id
    }

    /// Get the zenoh session ZID
    pub fn get_zid(&self) -> String {
        self.session.zid().to_string()
    }

    /// Local queue fallback for HOL processing
    fn queue_packet_locally(&self, p_buffer: &[u8], channel: i32, _from_peer_id: i64) {
        let data = p_buffer.to_vec(); // Use Vec<u8> directly
        let packet = Packet {
            data,
        };

        let mut queues = self.packet_queues.lock().unwrap();
        queues
            .entry(channel)
            .or_insert_with(VecDeque::new)
            .push_back(packet);
    }
}
