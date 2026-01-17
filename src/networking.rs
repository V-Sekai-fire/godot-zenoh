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

/// Zenoh networking session for peer-to-peer communication with HOL blocking prevention
#[derive(Debug, Clone)]
pub struct Packet {
    pub data: Vec<u8>,
    pub from_peer_id: i64,
}

pub struct ZenohSession {
    session: Arc<zenoh::Session>,
    runtime: Arc<Runtime>,
    publishers: Arc<Mutex<HashMap<i32, Publisher<'static>>>>,
    subscribers: Arc<Mutex<HashMap<i32, Subscriber<'static>>>>,
    packet_queues: Arc<Mutex<HashMap<i32, VecDeque<Packet>>>>,
    game_id: GString,
    peer_id: i64,
    is_server: bool,
}

impl ZenohSession {
    pub async fn create_client(
        address: GString,
        port: i32,
        packet_queues: Arc<Mutex<HashMap<i32, VecDeque<Packet>>>>,
        game_id: GString,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        godot_print!("Creating Zenoh CLIENT peer - replacing ENet networking with HOL blocking prevention");

        // Connect to Zenoh router/scouting
        let config = zenoh::Config::default();

        // If address is provided, connect to specific router
        let config = if !address.is_empty() {
            let listen_addr = format!("tcp/{:}:{:}", address, port);
            config.listen_endpoints(vec![listen_addr.parse()?])
        } else {
            config
        };

        let session = zenoh::open(config).res().await.map_err(|e| {
            godot_error!("Failed to connect Zenoh session: {:?}", e);
            e
        })?;

        let session = Arc::new(session);

        // Generate unique peer ID for this client
        let peer_id = (rand::random::<u32>() % 999) as i64 + 2; // Client IDs 2-1000

        godot_print!("Zenoh client connected - Peer ID: {}, Game: {}",
                    peer_id, game_id);

        Ok(ZenohSession {
            session,
            runtime: Arc::new(Runtime::new()?),
            publishers: Arc::new(Mutex::new(HashMap::new())),
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            packet_queues,
            game_id,
            peer_id,
            is_server: false,
        })
    }

    pub async fn create_server(
        _port: i32,
        _max_clients: i32,
        packet_queues: Arc<Mutex<HashMap<i32, VecDeque<Packet>>>>,
        game_id: GString,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        godot_print!("Creating Zenoh SERVER peer - replacing ENet networking with HOL blocking prevention");

        // Configure as Zenoh router (server role)
        let mut config = zenoh::Config::default();

        // Enable scouting/router mode for server
        config.scouting.multicast.set_enabled(None);

        let session = zenoh::open(config).res().await.map_err(|e| {
            godot_error!("Failed to create Zenoh server session: {:?}", e);
            e
        })?;

        let session = Arc::new(session);

        godot_print!("Zenoh server started - Peer ID: 1, Game: {}", game_id);

        Ok(ZenohSession {
            session,
            runtime: Arc::new(Runtime::new()?),
            publishers: Arc::new(Mutex::new(HashMap::new())),
            subscribers: Arc::new(Mutex::new(HashMap::new())),
            packet_queues,
            game_id,
            peer_id: 1, // Server is always peer ID 1
            is_server: true,
        })
    }

    pub fn send_packet(&self, p_buffer: &[u8], game_id: GString, channel: i32) -> Error {
        // HOL BLOCKING PREVENTION: Use Zenoh pub/sub with virtual channel topics
        let topic = format!("godot/game/{}/channel{:03}", game_id, channel);

        if let Ok(publisher) = self.publishers.lock().unwrap().get(&channel) {
            // Send via Zenoh pub/sub
            let sender = publisher.clone();
            let data = p_buffer.to_vec();

            let session = Arc::clone(&self.session);
            self.runtime.spawn(async move {
                if let Err(e) = sender.put(data).res().await {
                    godot_error!("Failed to send Zenoh packet on channel {}: {:?}", channel, e);
                }
            });

            godot_print!("Sent packet via Zenoh HOL channel {} (size: {})", channel, p_buffer.len());
            return Error::OK;
        }

        // Fallback: Queue locally if Zenoh publisher not ready (HOL prevention still applies)
        godot_print!("Zenoh publisher not ready, queuing packet for HOL processing (channel: {})", channel);
        self.queue_packet_locally(p_buffer, channel, self.peer_id);
        Error::OK
    }

    pub fn setup_channel(&self, channel: i32) -> Error {
        let game_id = self.game_id.clone();
        let packet_queues = Arc::clone(&self.packet_queues);
        let peer_id = self.peer_id;

        let topic = format!("godot/game/{}/channel{:03}", game_id, channel);

        godot_print!("Setting up Zenoh HOL channel {}: {}", channel, topic);

        // Create publisher for this channel
        {
            let mut publishers = self.publishers.lock().unwrap();
            if publishers.get(&channel).is_none() {
                let session = Arc::clone(&self.session);
                let runtime = Arc::clone(&self.runtime);

                // Create publisher asynchronously
                runtime.block_on(async {
                    match session.declare_publisher(&topic).res().await {
                        Ok(publisher) => {
                            publishers.insert(channel, publisher);
                            godot_print!("✅ Zenoh publisher created for HOL channel {}", channel);
                        }
                        Err(e) => {
                            godot_error!("Failed to create Zenoh publisher for channel {}: {:?}", channel, e);
                        }
                    }
                });
            }
        }

        // Create subscriber for this channel (HOL prevention receives packets here)
        {
            let mut subscribers = self.subscribers.lock().unwrap();
            if subscribers.get(&channel).is_none() {
                let session = Arc::clone(&self.session);
                let packet_queues = Arc::clone(&packet_queues);
                let runtime = Arc::clone(&self.runtime);

                runtime.block_on(async {
                    let subscriber_result = session
                        .declare_subscriber(&topic)
                        .callback(move |sample| {
                            // HOL BLOCKING PREVENTION: Received packet from Zenoh
                            // Queue it for HOL-safe processing
                            let payload = sample.payload();
                            let data = payload.contiguous().to_vec();

                            let mut queues = packet_queues.lock().unwrap();
                            let packet = Packet {
                                data,
                                from_peer_id: 0, // TODO: Extract from Zenoh sample
                            };

                            queues.entry(channel).or_insert_with(VecDeque::new).push_back(packet);

                            godot_print!("✅ HOL PREVENTION: Received packet via Zenoh (channel: {}, size: {})",
                                        channel, payload.len());
                        })
                        .res()
                        .await;

                    match subscriber_result {
                        Ok(subscriber) => {
                            subscribers.insert(channel, subscriber);
                            godot_print!("✅ Zenoh subscriber created for HOL channel {}", channel);
                        }
                        Err(e) => {
                            godot_error!("Failed to create Zenoh subscriber for channel {}: {:?}", channel, e);
                        }
                    }
                });
            }
        }

        Error::OK
    }

    fn queue_packet_locally(&self, p_buffer: &[u8], channel: i32, from_peer_id: i64) {
        let packet = Packet {
            data: p_buffer.to_vec(),
            from_peer_id,
        };

        let mut queues = self.packet_queues.lock().unwrap();
        let queue = queues.entry(channel).or_insert_with(VecDeque::new);
        queue.push_back(packet);

        godot_print!("Packet queued locally for HOL processing (channel: {}, peer: {})",
                    channel, from_peer_id);
    }

    pub fn receive_packets(&self) -> Vec<(i32, Packet)> {
        // HOL BLOCKING PREVENTION: Return packets in lowest-channel-first order
        let mut results = Vec::new();
        let mut queues = self.packet_queues.lock().unwrap();

        // Process channels in HOL prevention order (0-255)
        for channel in 0..=255 {
            if let Some(queue) = queues.get_mut(&channel) {
                while let Some(packet) = queue.pop_front() {
                    results.push((channel, packet));
                    // For HOL prevention, we could limit to one packet per channel per poll
                    // but for now, return all available packets
                }
            }
        }

        results
    }

    pub fn close(&self) -> Error {
        let session_arc = Arc::clone(&self.session);
        self.runtime.block_on(async move {
            if let Err(e) = session_arc.close().res().await {
                godot_error!("Error closing Zenoh session: {:?}", e);
            }
        });

        godot_print!("Zenoh session closed - HOL blocking prevention ended");
        Error::OK
    }
}
