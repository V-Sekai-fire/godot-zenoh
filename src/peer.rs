use godot::classes::multiplayer_peer::{ConnectionStatus, TransferMode};
use godot::classes::IMultiplayerPeerExtension;
use godot::classes::MultiplayerPeerExtension;

use godot::prelude::*;

use godot::builtin::GString as GodotString;
use godot::global::Error;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::networking::{Packet, ZenohSession};
// use uhlc::ID; NEVER USE UHLC.

// Async command types for Zenoh operations
#[derive(Debug)]
enum ZenohCommand {
    CreateServer {
        port: i32,
    },
    #[allow(dead_code)] // Note: address and port fields are reserved for future functionality
    CreateClient {
        address: String,
        port: i32,
    },
    SendPacket {
        data: Vec<u8>,
        channel: i32,
    },
}
enum ZenohStateUpdate {
    ServerCreated { zid: String },
    ClientConnected { zid: String, peer_id: i32 },
    ConnectionFailed { error: String },
}

// Async actor for Zenoh operations (properly thread-safe)
struct ZenohActor {
    session: Option<ZenohSession>,
    game_id: GodotString,
}

impl ZenohActor {
    fn new(game_id: GodotString) -> Self {
        Self {
            session: None,
            game_id,
        }
    }

    async fn handle_command(&mut self, cmd: ZenohCommand) -> Option<ZenohStateUpdate> {
        match cmd {
            ZenohCommand::CreateServer { port } => {
                match ZenohSession::create_server(port, self.game_id.clone()).await {
                    Ok(s) => {
                        let zid = s.get_zid();
                        self.session = Some(s);

                        // Auto-setup all 256 virtual channels for server
                        if let Some(sess) = &mut self.session {
                            for channel in 0..=255 {
                                if let Err(_e) = sess.setup_channel(channel).await {
                                    return Some(ZenohStateUpdate::ConnectionFailed {
                                        error: format!(
                                            "Server channel setup failed for {}",
                                            channel
                                        ),
                                    });
                                }
                            }
                            // Channels setup successfully
                        }

                        Some(ZenohStateUpdate::ServerCreated { zid })
                    }
                    Err(e) => Some(ZenohStateUpdate::ConnectionFailed {
                        error: e.to_string(),
                    }),
                }
            }
            ZenohCommand::CreateClient {
                address: _,
                port: _,
            } => {
                match ZenohSession::create_client(self.game_id.clone()).await {
                    Ok(s) => {
                        let zid = s.get_zid();
                        let peer_id = s.get_peer_id();
                        self.session = Some(s);

                        // Setup all 256 virtual channels for the client
                        if let Some(sess) = &mut self.session {
                            for channel in 0..=255 {
                                if let Err(_e) = sess.setup_channel(channel).await {
                                    return Some(ZenohStateUpdate::ConnectionFailed {
                                        error: format!(
                                            "Client channel setup failed for {}",
                                            channel
                                        ),
                                    });
                                }
                            }
                            // Channels setup successfully
                        }

                        Some(ZenohStateUpdate::ClientConnected {
                            zid,
                            peer_id: peer_id as i32,
                        })
                    }
                    Err(e) => Some(ZenohStateUpdate::ConnectionFailed {
                        error: e.to_string(),
                    }),
                }
            }
            ZenohCommand::SendPacket { data, channel } => {
                if let Some(sess) = &mut self.session {
                    let _result = sess.send_packet(&data, self.game_id.clone(), channel).await;
                    // Send result is not critical - silent failure for now
                }
                None // No event for successful send
            }
        }
    }
}

// Async bridge using std::thread to avoid runtime nesting issues
struct ZenohAsyncBridge {
    command_queue: Arc<Mutex<Vec<ZenohCommand>>>,
    event_queue: Arc<Mutex<Vec<ZenohStateUpdate>>>,
    join_handle: Option<thread::JoinHandle<()>>,
    stop_flag: Arc<Mutex<bool>>,
}

impl ZenohAsyncBridge {
    fn new(
        _packet_queues: Arc<Mutex<HashMap<i32, VecDeque<Packet>>>>,
        game_id: GodotString,
    ) -> Self {
        let command_queue = Arc::new(Mutex::new(Vec::new()));
        let event_queue = Arc::new(Mutex::new(Vec::new()));
        let stop_flag = Arc::new(Mutex::new(false));

        let cmd_queue_clone = Arc::clone(&command_queue);
        let event_queue_clone = Arc::clone(&event_queue);
        let stop_flag_clone = Arc::clone(&stop_flag);

        let actor = ZenohActor::new(game_id);

        // Spawn the worker thread
        let join_handle = thread::spawn(move || {
            let _ = Self::zenoh_worker_thread(
                actor,
                cmd_queue_clone,
                event_queue_clone,
                stop_flag_clone,
            );
        });

        Self {
            command_queue,
            event_queue,
            join_handle: Some(join_handle),
            stop_flag,
        }
    }

    fn zenoh_worker_thread(
        mut actor: ZenohActor,
        command_queue: Arc<Mutex<Vec<ZenohCommand>>>,
        event_queue: Arc<Mutex<Vec<ZenohStateUpdate>>>,
        stop_flag: Arc<Mutex<bool>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Create tokio runtime in thread (avoids GDextension nesting)
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;

        rt.block_on(async {
            // Worker thread is pure state machine - no sleeps, only queue-driven processing
            loop {
                // State machine: Check for stop signal (immediate exit)
                if *stop_flag.lock().unwrap() {
                    break;
                }

                // State machine: Process command queue (non-blocking)
                let cmds = {
                    let mut queue = command_queue.lock().unwrap();
                    std::mem::take(&mut *queue)
                };

                if !cmds.is_empty() {
                    // Process each command atomically
                    for cmd in cmds {
                        if let Some(event) = actor.handle_command(cmd).await {
                            event_queue.lock().unwrap().push(event);
                        }
                    }
                } else {
                    // Queue empty, yield control via tokio scheduler (efficient wait)
                    tokio::task::yield_now().await;
                }
            }
        });

        Ok(())
    }

    fn send_command(&self, cmd: ZenohCommand) -> Result<(), Box<dyn std::error::Error>> {
        // Block on command queue lock - this is intentional for thread safety
        let mut queue = self.command_queue.lock().unwrap();
        queue.push(cmd);
        godot_print!("Command queued for worker thread");
        Ok(())
    }

    fn get_events(&self) -> Vec<ZenohStateUpdate> {
        if let Ok(mut queue) = self.event_queue.try_lock() {
            std::mem::take(&mut *queue)
        } else {
            Vec::new()
        }
    }
}

impl Drop for ZenohAsyncBridge {
    fn drop(&mut self) {
        godot_print!("Dropping ZenohAsyncBridge");
        *self.stop_flag.lock().unwrap() = true;
        if let Some(join_handle) = self.join_handle.take() {
            let _ = join_handle.join();
        }
    }
}

#[derive(GodotClass)]
#[class(base=MultiplayerPeerExtension, tool)]
pub struct ZenohMultiplayerPeer {
    #[export]
    game_id: GodotString,

    // Async bridge for Zenoh networking
    async_bridge: Option<Box<ZenohAsyncBridge>>,

    // Peer management
    unique_id: i32,
    connection_status: i32,
    transfer_mode: i32,

    // HOL Blocking Prevention: Virtual Channel System
    packet_queues: Arc<Mutex<HashMap<i32, VecDeque<Packet>>>>,
    current_channel: i32,
    max_packet_size: i32,

    // Current packet information for get_packet_* methods
    current_packet_peer: i32,

    // Store ZID for get_zid() method
    zid: GodotString,

    base: Base<MultiplayerPeerExtension>,
}

#[godot_api]
impl IMultiplayerPeerExtension for ZenohMultiplayerPeer {
    fn init(_base: Base<MultiplayerPeerExtension>) -> Self {
        // Silent initialization - no verbose output per Unix philosophy

        Self {
            game_id: GString::new(),
            async_bridge: None,
            unique_id: 1,
            connection_status: 0, // DISCONNECTED
            transfer_mode: 0,     // UNRELIABLE
            packet_queues: Arc::new(Mutex::new(HashMap::new())),
            current_channel: 0,
            max_packet_size: 1472, // UDP MTU - Zenoh overhead
            current_packet_peer: 0,
            zid: GString::from(""),
            base: _base,
        }
    }

    // Virtual method overrides for multiplayer peer functionality
    fn get_available_packet_count(&self) -> i32 {
        let queues = self.packet_queues.lock().unwrap();
        queues.values().map(|q| q.len() as i32).sum()
    }

    fn get_max_packet_size(&self) -> i32 {
        self.max_packet_size
    }

    fn get_packet_channel(&self) -> i32 {
        self.current_channel
    }

    fn get_packet_mode(&self) -> TransferMode {
        match self.transfer_mode {
            0 => TransferMode::UNRELIABLE,
            1 => TransferMode::UNRELIABLE_ORDERED,
            2 => TransferMode::RELIABLE,
            _ => TransferMode::UNRELIABLE,
        }
    }

    fn set_transfer_channel(&mut self, channel: i32) {
        self.current_channel = channel;
        godot_print!("Virtual channel set to: {}", channel);
    }

    fn get_transfer_channel(&self) -> i32 {
        self.current_channel
    }

    fn set_transfer_mode(&mut self, mode: TransferMode) {
        // Zenoh is a pub/sub system without guaranteed delivery, so treat RELIABLE as UNRELIABLE_ORDERED
        self.transfer_mode = match mode {
            TransferMode::UNRELIABLE => 0,
            TransferMode::UNRELIABLE_ORDERED => 1,
            TransferMode::RELIABLE => 1, // Treat RELIABLE as UNRELIABLE_ORDERED since Zenoh doesn't guarantee delivery
            _ => 0,
        };
        godot_print!(
            "Transfer mode set to: {} (Zenoh pub/sub - best effort delivery)",
            self.transfer_mode
        );
    }

    fn get_transfer_mode(&self) -> TransferMode {
        self.get_packet_mode()
    }

    fn set_target_peer(&mut self, _peer_id: i32) {
        // Virtual channels don't use target peer concept
        godot_print!("Target peer setting not applicable for virtual channels");
    }

    fn get_packet_peer(&self) -> i32 {
        self.current_packet_peer
    }

    fn is_server(&self) -> bool {
        self.unique_id == 1
    }

    fn poll(&mut self) {
        // Get events from worker thread
        if let Some(bridge) = &self.async_bridge {
            let events = bridge.get_events();
            for event in events {
                match event {
                    ZenohStateUpdate::ClientConnected { zid, peer_id } => {
                        self.connection_status = 2; // CONNECTED
                        self.unique_id = peer_id;
                        self.zid = GString::from(zid.as_str());
                        godot_print!("CLIENT CONNECTED: ZID: {}, Peer ID: {}", zid, peer_id);

                        // Emit connected_to_server signal for clients
                        self.base_mut().emit_signal("connected_to_server", &[]);
                    }
                    ZenohStateUpdate::ServerCreated { zid } => {
                        self.connection_status = 2; // CONNECTED
                        self.unique_id = 1; // Server is peer 1
                        self.zid = GString::from(zid.as_str());
                        godot_print!("SERVER CREATED: ZID: {}, Peer ID: {}", zid, self.unique_id);
                    }
                    ZenohStateUpdate::ConnectionFailed { error } => {
                        self.connection_status = 0; // DISCONNECTED
                        godot_error!("CONNECTION FAILED: {}", error);

                        // Emit connection_failed signal
                        self.base_mut().emit_signal("connection_failed", &[]);
                    }
                }
            }
        }

        // HOL blocking prevention doesn't require additional polling
        // Worker thread handles async operations
    }

    fn close(&mut self) {
        // Only log if we were actually connected
        if self.connection_status != 0 {
            godot_print!("ZenohMultiplayerPeer connection closed");
        }
        self.connection_status = 0; // DISCONNECTED
                                    // Clear all packet queues
        self.packet_queues.lock().unwrap().clear();
        // Clear send queues on close
        // Note: Zenoh session will be dropped when async_bridge is dropped
    }

    fn disconnect_peer(&mut self, _peer_id: i32, _force: bool) {
        // Virtual channels handle packets, not peer connections
        godot_print!("Peer disconnection not applicable for virtual channels");
    }

    fn get_unique_id(&self) -> i32 {
        self.unique_id as i32
    }

    fn get_connection_status(&self) -> ConnectionStatus {
        match self.connection_status {
            0 => ConnectionStatus::DISCONNECTED,
            1 => ConnectionStatus::CONNECTING,
            2 => ConnectionStatus::CONNECTED,
            _ => ConnectionStatus::DISCONNECTED,
        }
    }
}

#[godot_api]
impl ZenohMultiplayerPeer {
    #[func]
    fn get_zid(&self) -> String {
        let zid_str = self.zid.to_string();
        godot_print!("get_zid() returning: '{}'", zid_str);
        zid_str
    }

    #[func]
    fn connection_status(&self) -> i32 {
        self.connection_status
    }

    #[func]
    fn transfer_mode(&self) -> i32 {
        self.transfer_mode
    }

    #[func]
    fn set_transfer_mode_int(&mut self, mode: i32) -> Error {
        self.transfer_mode = mode;
        godot_print!("Transfer mode set to: {}", mode);
        Error::OK
    }

    #[func]
    fn transfer_channel(&self) -> i32 {
        self.current_channel
    }

    #[func]
    fn get_packet(&mut self) -> PackedByteArray {
        // Process packets by priority: always check lowest channel number first
        // Channels are ordered 0=highest priority, 255=lowest priority
        let mut queues = self.packet_queues.lock().unwrap();
        for priority in 0..=255 {
            if let Some(queue) = queues.get_mut(&priority) {
                if let Some(packet) = queue.pop_front() {
                    // Store sender peer ID for get_packet_peer() - TODO: parse from HLC
                    self.current_packet_peer = 0; // Placeholder until HLC parsing implemented
                    // Update current channel so get_packet_channel() returns the correct channel
                    self.current_channel = priority;

                    godot_print!(
                        "DEBUG: Retrieved packet from channel {} (size: {}, timestamp: {:?})",
                        priority,
                        packet.data.len(),
                        packet.timestamp
                    );
                    // Convert Vec<u8> directly to PackedByteArray
                    return PackedByteArray::from_iter(packet.data.iter().copied());
                }
            }
        }
        godot_print!("DEBUG: No packets available in any queue");
        PackedByteArray::new()
    }

    #[func]
    fn put_packet(&mut self, p_buffer: PackedByteArray) -> Error {
        godot_print!(
            "DEBUG: put_packet called with {} bytes on channel {}",
            p_buffer.len(),
            self.current_channel
        );
        self.put_packet_on_channel(p_buffer, self.current_channel)
    }

    #[func]
    fn put_packet_on_channel(&mut self, p_buffer: PackedByteArray, channel: i32) -> Error {
        // Use async bridge for sending packets
        if let Some(bridge) = &self.async_bridge {
            let data_vec = p_buffer.to_vec();
            if let Err(e) = bridge.send_command(ZenohCommand::SendPacket {
                data: data_vec,
                channel,
            }) {
                godot_error!("Failed to send packet via async bridge: {:?}", e);
                return Error::FAILED;
            }
            return Error::OK;
        }

        // No networking session available - cannot send packet
        godot_error!("No networking session available for packet transmission");
        Error::FAILED
    }

    #[func]
    fn create_client(&mut self, address: GodotString, port: i32) -> Error {
        godot_print!(
            "Creating Zenoh client asynchronously on {}:{}",
            address,
            port
        );

        // Close any existing connection first
        self.close();

        // Set status to CONNECTING before attempting connection
        self.connection_status = 1; // CONNECTING

        // Initialize async bridge if not exists
        if self.async_bridge.is_none() {
            self.async_bridge = Some(Box::new(ZenohAsyncBridge::new(
                Arc::clone(&self.packet_queues),
                self.game_id.clone(),
            )));
        }

        // Send async command to create client
        if let Some(bridge) = &mut self.async_bridge {
            if let Err(e) = bridge.send_command(ZenohCommand::CreateClient {
                address: address.to_string(),
                port,
            }) {
                godot_error!("Failed to send create client command: {:?}", e);
                self.connection_status = 0; // DISCONNECTED
                return Error::FAILED;
            }
        }

        godot_print!("Client creation initiated - status: CONNECTING");
        Error::OK
    }

    #[func]
    fn create_server(&mut self, port: i32, _max_clients: i32) -> Error {
        godot_print!("Creating Zenoh server asynchronously on port {}", port);

        // Close any existing connection first
        self.close();

        // Set status to CONNECTING before attempting connection
        self.connection_status = 1; // CONNECTING

        // Initialize async bridge if not exists
        if self.async_bridge.is_none() {
            self.async_bridge = Some(Box::new(ZenohAsyncBridge::new(
                Arc::clone(&self.packet_queues),
                self.game_id.clone(),
            )));
        }

        // Send async command to create server
        if let Some(bridge) = &mut self.async_bridge {
            if let Err(e) = bridge.send_command(ZenohCommand::CreateServer { port }) {
                godot_error!("Failed to send create server command: {:?}", e);
                self.connection_status = 0; // DISCONNECTED
                return Error::FAILED;
            }
        }

        godot_print!("Server creation initiated - status: CONNECTING");
        Error::OK
    }

    #[func]
    fn disconnect(&mut self) {
        self.close();
    }

    #[func]
    fn get_server_address(&self) -> String {
        if self.connection_status == 2 && self.unique_id == 1 {
            "localhost:7447".to_string() // Default server address
        } else {
            "".to_string()
        }
    }

    #[func]
    fn get_connected_clients_count(&self) -> i32 {
        // For now, return 0 as we don't track individual clients
        // In a full implementation, this would track connected peers
        0
    }

    #[func]
    fn get_network_info(&self) -> VarDictionary {
        let mut dict = VarDictionary::new();
        dict.set("status", self.connection_status());
        dict.set("unique_id", self.get_unique_id());
        dict.set("zid", self.get_zid());
        dict.set("is_server", self.is_server());
        dict.set("packet_count", self.get_available_packet_count());
        dict.set("server_address", self.get_server_address());
        dict.set("connected_clients", self.get_connected_clients_count());
        dict.set("elapsed", 0); // Dummy value for compatibility
        dict
    }

    #[func]
    fn get_channel_packet_count(&self, channel: i32) -> i32 {
        let queues = self.packet_queues.lock().unwrap();
        if let Some(queue) = queues.get(&channel) {
            queue.len() as i32
        } else {
            0
        }
    }

    #[func]
    fn get_channel_info(&self, channel: i32) -> VarDictionary {
        let mut dict = VarDictionary::new();
        dict.set("channel", channel);
        dict.set("packet_count", self.get_channel_packet_count(channel));
        dict.set(
            "priority",
            if channel == 0 {
                "highest"
            } else if channel <= 10 {
                "high"
            } else if channel <= 100 {
                "normal"
            } else {
                "low"
            },
        );
        dict.set("special", "");
        dict.set("elapsed", 0); // Dummy value for compatibility
        dict
    }
}
