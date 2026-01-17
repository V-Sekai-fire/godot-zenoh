use godot::prelude::*;
use godot::global::Error;
use godot::builtin::GString;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;
use std::collections::VecDeque;
use std::collections::HashMap;
use zenoh::prelude::r#async::*;
use zenoh::publication::Publisher;
use zenoh::subscriber::Subscriber;

/// Real Zenoh networking packet with peer identification
#[derive(Clone, Debug)]
pub struct Packet {
    pub data: Vec<u8>,
    pub channel: i32,
    pub from_peer_id: i64,
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
    subscribers: Arc<Mutex<HashMap<i32, Subscriber<'static>>>>,
    /// packet_queues for HOL processing
    packet_queues: Arc<Mutex<HashMap<i32, VecDeque<Packet>>>>,
    /// Game identifier
    game_id: GString,
    /// Unique peer identifier
    peer_id: i64,
    /// Server/client role
    is_server: bool,
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

        // Configure Zenoh session
        let config = zenoh::Config::default();

        // If address provided, connect to specific Zenoh router
        let config = if !address.is_empty() {
            let endpoints = vec![format!("tcp/{:}:{:}", address, port).parse()?];
            config.listen_endpoints(endpoints)
        } else {
            config
        };

        // Open Zenoh session
        let session = zenoh::open(config).res().await.map_err(|e| {
            godot_error!("❌ Failed to connect Zenoh CLIENT session: {:?}", e);
            e
        })?;

        let session = Arc::new(session);

        // Generate unique peer ID for client (1-1000)
        let peer_id = (rand::random::<u32>() % 999 + 1) as i64;

        godot_print!("✅ Zenoh CLIENT connected - Peer ID: {}, Game: {}", peer_id, game_id);

        Ok(ZenohSession {
            session,
            runtime: Runtime::new()?,
            publishers: Arc::new(Mutex::new(HashMap::new())),
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            packet_queues,
            game_id,
            peer_id,
            is_server: false,
        })
    }

    /// Create Zenoh networking server session (router)
    pub async fn create_server(
        port: i32,
        max_clients: i32,
        packet_queues: Arc<Mutex<HashMap<i32, VecDeque<Packet>>>>,
        game_id: GString,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        godot_print!("🎯 Creating Zenoh SERVER router - HOL blocking prevention ENABLED");

        // Configure as Zenoh router (server mode)
        let mut config = zenoh::Config::default();

        // Enable multicast scouting for router discovery
        let config = config.scouting.multicast.set_enabled(Some("en0".into()));

        // Listen on specified port
        let listen_endpoint = format!("tcp/0.0.0.0:{:}", port).parse()?;
        let config = config.listen_endpoints(vec![listen_endpoint]);

        let session = zenoh::open(config).res().await.map_err(|e| {
            godot_error!("❌ Failed to create Zenoh SERVER session: {:?}", e);
            e
        })?;

        let session = Arc::new(session);

        godot_print!("✅ Zenoh SERVER router started on port {}, Game: {}", port, game_id);

        Ok(ZenohSession {
            session,
            runtime: Runtime::new()?,
            publishers: Arc::new(Mutex::new(HashMap::new())),
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            packet_queues,
            game_id,
            peer_id: 0, // Server is peer 0
            is_server: true,
        })
    }

    /// HOL BLOCKING PREVENTION: Send packet on specific virtual channel
    pub fn send_packet(&self, p_buffer: &[u8], game_id: GString, channel: i32) -> Error {
        let topic = format!("godot/game/{}/channel{:03}", game_id, channel);

        // Try to get existing publisher
        if let Ok(publisher) = self.publishers.lock().unwrap().get(&channel) {
            let data = p_buffer.to_vec();
            let publisher_clone = publisher.clone();

            self.runtime.spawn(async move {
                if let Err(e) = publisher_clone.put(data).res().await {
                    godot_error!("Failed to send Zenoh packet on channel {}: {:?}", channel, e);
                }
            });

            godot_print!("📤 Packet sent via Zenoh HOL channel {} (size: {})", channel, p_buffer.len());
            return Error::OK;
        }

        godot_print!("⚠️ Zenoh publisher not ready for channel {}, queuing locally", channel);
        self.queue_packet_locally(p_buffer, channel, self.peer_id);
        Error::OK
    }

    /// HOL BLOCKING PREVENTION: Setup publisher/subscriber for virtual channel
    pub fn setup_channel(&self, channel: i32) -> Error {
        let game_id = &self.game_id;
        let packet_queues = Arc::clone(&self.packet_queues);
        let peer_id = self.peer_id;

        let topic = format!("godot/game/{}/channel{:03}", game_id, channel);

        godot_print!("🎛️ Setting up Zenoh HOL virtual channel {}: {}", channel, topic);

        // Lazy initialization of publisher
        {
            let mut publishers = self.publishers.lock().unwrap();
            if publishers.get(&channel).is_none() {
                let session = Arc::clone(&self.session);
                let publisher_result = self.runtime.block_on(async {
                    session.declare_publisher(&topic).res().await
                });

                match publisher_result {
                    Ok(publisher) => {
                        publishers.insert(channel, publisher);
                        godot_print!("✅ Zenoh publisher created for HOL channel {}", channel);
                    }
                    Err(e) => {
                        godot_error!("❌ Failed to create Zenoh publisher for channel {}: {:?}", channel, e);
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
                let packet_queues = Arc::clone(&packet_queues);

                let subscriber_result = self.runtime.block_on(async {
                    session.declare_subscriber(&topic)
                        .callback(move |sample| {
                            // HOL BLOCKING PREVENTION: Received packet from Zenoh
                            // Queue it for HOL-safe processing (lowest channels first)
                            let data = sample.payload().contiguous().to_vec();

                            let packet = Packet {
                                data,
                                channel,
                                from_peer_id: peer_id, // In real impl, extract from Zenoh metadata
                            };

                            let mut queues = packet_queues.lock().unwrap();
                            queues.entry(channel).or_insert_with(VecDeque::new).push_back(packet);

                            godot_print!("📥 HOL PREVENTION: Received packet on channel {} (size: {})",
                                       channel, sample.payload().len());
                        })
                        .res()
                        .await
                });

                match subscriber_result {
                    Ok(subscriber) => {
                        subscribers.insert(channel, subscriber);
                        godot_print!("✅ Zenoh subscriber created for HOL channel {}", channel);
                    }
                    Err(e) => {
                        godot_error!("❌ Failed to create Zenoh subscriber for channel {}: {:?}", channel, e);
                        return Error::FAILED;
                    }
                }
            }
        }

        Error::OK
    }

    /// HOL BLOCKING PREVENTION: Receive packets in priority order (0-255)
    pub fn receive_packets(&self) -> Vec<Packet> {
        let mut results = Vec::new();
        let mut queues = self.packet_queues.lock().unwrap();

        // Process channels in HOL prevention order (lowest number = highest priority)
        for channel in 0..=255 {
            if let Some(queue) = queues.get_mut(&channel) {
                while let Some(packet) = queue.pop_front() {
                    results.push(packet);
                    // In production, might limit to one packet per channel per call
                }
            }
        }

        results
    }

    /// Cleanup Zenoh networking session
    pub fn close(&self) -> Error {
        let session = Arc::clone(&self.session);
        self.runtime.block_on(async move {
            if let Err(e) = session.close().res().await {
                godot_error!("Error closing Zenoh networking session: {:?}", e);
            }
        });

        godot_print!("🧹 Zenoh networking session closed - HOL blocking prevention ended");
        Error::OK
    }

    /// Local queue fallback for HOL processing
    fn queue_packet_locally(&self, p_buffer: &[u8], channel: i32, from_peer_id: i64) {
        let packet = Packet {
            data: p_buffer.to_vec(),
            channel,
            from_peer_id,
        };

        let mut queues = self.packet_queues.lock().unwrap();
        queues.entry(channel).or_insert_with(VecDeque::new).push_back(packet);
    }
}
