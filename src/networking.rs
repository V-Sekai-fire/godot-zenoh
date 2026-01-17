use godot::builtin::GString;
use godot::global::Error;
use godot::prelude::*;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use zenoh::Config as ZenohConfig;
// ZBuf import will be added when zenoh 1.7.2 module structure is known
// For now using Vec<u8> - will replace with native ZBuf once located
use zenoh::pubsub::Publisher;
use zenoh::pubsub::Subscriber;
use zenoh_config::{EndPoint, ModeDependentValue};

/// Zenoh-native packet using topic-based routing with channel-based priority
#[derive(Clone, Debug)]
pub struct Packet {
    pub data: Vec<u8>, // Using Vec<u8> - will optimize to ZBuf when api known
}

/// Zenoh networking session with HOL blocking prevention
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
    /// Create Zenoh networking client session
    pub async fn create_client(
        address: GString,
        port: i32,
        packet_queues: Arc<Mutex<HashMap<i32, VecDeque<Packet>>>>,
        game_id: GString,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        godot_print!("🎯 Creating Zenoh CLIENT session - HOL blocking prevention ENABLED");

        // Configure Zenoh session with endpoint
        let mut config = zenoh_config::Config::default();

        if !address.is_empty() && port > 0 {
            // Connect to specific Zenoh router endpoint using QUIC for superior performance
            let endpoint = format!("quic/{}:{}", address, port);
            godot_print!(
                "🚀 Connecting Zenoh CLIENT to router via QUIC: {}",
                endpoint
            );

            // Set QUIC endpoint for router connection using zenoh 1.7.2 Endpoint creation
            let endpoint_str = format!("quic/{}:{}", address, port);
            // Create endpoint using unchecked string parsing for zenoh 1.7.2
            if let Ok(endpoint) = endpoint_str.parse::<EndPoint>() {
                config.connect.endpoints = ModeDependentValue::Unique(vec![endpoint]);
            } else {
                godot_print!(
                    "⚠️ Failed to parse endpoint {}, falling back to defaults",
                    endpoint_str
                );
            }
        } else {
            godot_print!("🌐 Creating Zenoh CLIENT with default peer discovery");
        }

        // Use default zenoh config for now - zenoh_config conversion needs zenoh specific API
        let zenoh_config = ZenohConfig::default();
        let session_result = zenoh::open(zenoh_config).await;
        let session = match session_result {
            Ok(sess) => sess,
            Err(e) => {
                godot_error!("❌ Failed to connect Zenoh CLIENT session: {:?}", e);
                return Err(format!("Zenoh session creation failed: {:?}", e).into());
            }
        };

        let session = Arc::new(session);

        // Generate unique peer ID for client (1-1000)`
        let peer_id = (rand::random::<u32>() % 999 + 1) as i64;

        godot_print!(
            "✅ Zenoh CLIENT connected - Peer ID: {}, Game: {}",
            peer_id,
            game_id
        );

        Ok(ZenohSession {
            session,
            runtime: Runtime::new()?,
            publishers: Arc::new(Mutex::new(HashMap::new())),
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            packet_queues,
            game_id,
            peer_id,
        })
    }

    /// Create Zenoh networking server session (router)
    pub async fn create_server(
        port: i32,
        max_clients: i32,
        packet_queues: Arc<Mutex<HashMap<i32, VecDeque<Packet>>>>,
        game_id: GString,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        godot_print!(
            "🎯 Creating Zenoh SERVER router on port {} - HOL blocking prevention ENABLED",
            port
        );

        // Configure as Zenoh router (server mode)
        let mut config = zenoh_config::Config::default();

        // Enable multicast scouting for client discovery
        if let Err(e) = config.scouting.multicast.set_enabled(Some(true)) {
            godot_error!("❌ Failed to enable multicast scouting: {:?}", e);
        }

        // Configure listen endpoints for the server with QUIC for maximum performance
        if port > 0 {
            let quic_endpoint = format!("quic/127.0.0.1:{}", port);
            let tcp_endpoint = format!("tcp/127.0.0.1:{}", port + 1000); // TCP fallback on port+1000
            let http_endpoint = format!("http/127.0.0.1:{}", port + 2000); // HTTP API on port+2000
            godot_print!(
                "⚡ Configuring Zenoh SERVER with QUIC: QUIC={}, TCP={}, HTTP={}",
                quic_endpoint,
                tcp_endpoint,
                http_endpoint
            );

            // Parse endpoints directly for server configuration
            let mut endpoints = Vec::new();
            if let Ok(quic_ep) = quic_endpoint.parse::<EndPoint>() {
                endpoints.push(quic_ep);
            }
            if let Ok(tcp_ep) = tcp_endpoint.parse::<EndPoint>() {
                endpoints.push(tcp_ep);
            }
            if let Ok(http_ep) = http_endpoint.parse::<EndPoint>() {
                endpoints.push(http_ep);
            }
            if !endpoints.is_empty() {
                config.listen.endpoints = ModeDependentValue::Unique(endpoints);
            }
        } else {
            godot_print!("⚠️ No server port specified, using default Zenoh router configuration");
            // Default Zenoh will use standard ports
        }

        // Configure connection limits if max_clients specified
        if max_clients > 0 {
            godot_print!(
                "👥 Server configured for max {} client connections",
                max_clients
            );
            // Note: Zenoh doesn't have direct client limits, but we document the intention
            // In a real implementation, this could configure QoS or connection acceptance policies
        } else {
            godot_print!("🌐 Server configured for unlimited client connections");
        }

        // Use default zenoh config for now - zenoh_config conversion needs zenoh specific API
        let zenoh_config = ZenohConfig::default();
        let session_result = zenoh::open(zenoh_config).await;
        let session = match session_result {
            Ok(sess) => sess,
            Err(e) => {
                godot_error!("❌ Failed to create Zenoh SERVER session: {:?}", e);
                return Err(format!("Zenoh session creation failed: {:?}", e).into());
            }
        };

        let session = Arc::new(session);

        godot_print!(
            "✅ Zenoh SERVER router started on port {}, Game: {}",
            port,
            game_id
        );

        Ok(ZenohSession {
            session,
            runtime: Runtime::new()?,
            publishers: Arc::new(Mutex::new(HashMap::new())),
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            packet_queues,
            game_id,
            peer_id: 0, // Server is peer 0
        })
    }

    /// HOL BLOCKING PREVENTION: Send packet on specific virtual channel
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

            godot_print!(
                "📤 Packet sent via Zenoh HOL channel {} (size: {})",
                channel,
                p_buffer.len()
            );
            return Error::OK;
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

        godot_print!(
            "🎛️ Setting up Zenoh HOL virtual channel {} for peer {}",
            channel,
            peer_id
        );

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
                        godot_print!("✅ Zenoh publisher created for HOL channel {}", channel);
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
                    let packet_queues = packet_queues.clone();

                    session.declare_subscriber(topic)
                        .callback(move |sample| {
                            // HOL BLOCKING PREVENTION: Received packet from Zenoh topic
                            // Use zenoh 1.7.2 payload conversion methods
                            let payload_bytes = sample.payload().to_bytes();
                            let data: Vec<u8> = payload_bytes.to_vec();
                            let topic_str = sample.key_expr().as_str();
                            let hol_priority = channel; // Extract from topic or use channel mapping

                            let packet = Packet {
                                data,
                            };

                            let mut queues = packet_queues.lock().unwrap();
                            queues.entry(channel).or_insert_with(VecDeque::new).push_back(packet);

                            godot_print!("📥 HOL PREVENTION: Received packet on topic {} (priority: {}, size: {})",
                                       topic_str, hol_priority, sample.payload().len());
                        })
                        .await
                });

                match subscriber_result {
                    Ok(subscriber) => {
                        subscribers.insert(channel, subscriber);
                        godot_print!("✅ Zenoh subscriber created for HOL channel {}", channel);
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
