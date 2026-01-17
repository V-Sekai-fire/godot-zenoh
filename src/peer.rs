use godot::classes::multiplayer_peer::{ConnectionStatus, TransferMode};
use godot::classes::IMultiplayerPeerExtension;
use godot::classes::MultiplayerPeerExtension;
use godot::prelude::*;

use godot::builtin::GString as GodotString;
use godot::global::Error;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use crate::networking::{Packet, ZenohSession};

// Async command types for Zenoh operations
#[derive(Debug)]
enum ZenohCommand {
    CreateServer { port: i32 },
    CreateClient { address: String, port: i32 },
    SendPacket { data: Vec<u8>, channel: i32 },
}

// State update results from async operations
enum ZenohStateUpdate {
    ServerCreated { zid: String },
    ClientConnected { zid: String, peer_id: i64 },
    ConnectionFailed { error: String },
}

// Async actor for Zenoh operations (properly thread-safe)
struct ZenohActor {
    session: Option<ZenohSession>,
    packet_queues: Arc<Mutex<HashMap<i32, VecDeque<Packet>>>>,
    game_id: GodotString,
}

impl ZenohActor {
    fn new(packet_queues: Arc<Mutex<HashMap<i32, VecDeque<Packet>>>>, game_id: GodotString) -> Self {
        Self {
            session: None,
            packet_queues,
            game_id,
        }
    }

    async fn handle_command(&mut self, cmd: ZenohCommand) -> Option<ZenohStateUpdate> {
        match cmd {
            ZenohCommand::CreateServer { port } => {
                match ZenohSession::create_server(port, Arc::clone(&self.packet_queues), self.game_id.clone()).await {
                    Ok(s) => {
                        let zid = s.get_zid();
                        self.session = Some(s);

                        // Auto-setup all 256 virtual channels for server
                        if let Some(sess) = &mut self.session {
                            for channel in 0..=255 {
                                let result = sess.setup_channel(channel);
                                if result != Error::OK {
                                    godot_error!("Failed to setup channel {}: {:?}", channel, result);
                                    return Some(ZenohStateUpdate::ConnectionFailed {
                                        error: format!("Channel setup failed for {}", channel)
                                    });
                                }
                            }
                            godot_print!("Server created with 256 virtual channels");
                        }

                        Some(ZenohStateUpdate::ServerCreated { zid })
                    },
                    Err(e) => Some(ZenohStateUpdate::ConnectionFailed { error: e.to_string() }),
                }
            },
            ZenohCommand::CreateClient { address, port } => {
                match ZenohSession::create_client(GString::from(address.as_str()), port, Arc::clone(&self.packet_queues), self.game_id.clone()).await {
                    Ok(s) => {
                        let zid = s.get_zid();
                        let peer_id = s.get_peer_id();
                        self.session = Some(s);

                        // Setup all 256 virtual channels for the client
                        if let Some(sess) = &mut self.session {
                            for channel in 0..=255 {
                                let result = sess.setup_channel(channel);
                                if result != Error::OK {
                                    godot_error!("Failed to setup channel {}: {:?}", channel, result);
                                    return Some(ZenohStateUpdate::ConnectionFailed {
                                        error: format!("Channel setup failed for {}", channel)
                                    });
                                }
                            }
                            godot_print!("Client created with 256 virtual channels");
                        }

                        Some(ZenohStateUpdate::ClientConnected { zid, peer_id })
                    },
                    Err(e) => Some(ZenohStateUpdate::ConnectionFailed { error: e.to_string() }),
                }
            },
            ZenohCommand::SendPacket { data, channel } => {
                if let Some(sess) = &mut self.session {
                    let result = sess.send_packet(&data, self.game_id.clone(), channel).await;
                    if result != Error::OK {
                        godot_error!("Failed to send packet: {:?}", result);
                    }
                }
                None // No event for successful send
            }
        }
    }
}

// Async bridge (single-threaded to respect Zenoh's thread-safety constraints)
struct ZenohAsyncBridge {
    command_tx: mpsc::Sender<ZenohCommand>,
    command_rx: mpsc::Receiver<ZenohCommand>,
    actor: ZenohActor,
}

impl ZenohAsyncBridge {
    fn new(packet_queues: Arc<Mutex<HashMap<i32, VecDeque<Packet>>>>, game_id: GodotString) -> Self {
        let (command_tx, command_rx) = mpsc::channel::<ZenohCommand>(32);

        let actor = ZenohActor::new(packet_queues, game_id);

        Self {
            command_tx,
            command_rx,
            actor,
        }
    }

    fn send_command(&self, cmd: ZenohCommand) -> Result<(), mpsc::error::TrySendError<ZenohCommand>> {
        // Try to send command to queue
        match self.command_tx.try_send(cmd) {
            Ok(()) => {
                godot_print!("Command queued successfully");
                Ok(())
            }
            Err(e) => {
                godot_error!("Failed to queue command: {:?}", e);
                Err(e)
            }
        }
    }

    // Process commands immediately when called (not just in poll)
    fn process_pending_commands(&mut self) -> Vec<ZenohStateUpdate> {
        let mut events = Vec::new();
        // Process all pending commands
        while let Ok(cmd) = self.command_rx.try_recv() {
            godot_print!("Processing command: {:?}", cmd);
            // Create a new runtime for each command to avoid blocking
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let event = rt.block_on(async {
                self.actor.handle_command(cmd).await
            });
            if let Some(event) = event {
                events.push(event);
            }
        }
        events
    }
}

#[derive(GodotClass)]
#[class(base=MultiplayerPeerExtension, tool)]
pub struct ZenohMultiplayerPeer {
    #[export]
    game_id: GodotString,

    // Async bridge for Zenoh networking
    async_bridge: Option<Box<ZenohAsyncBridge>>,

    #[allow(dead_code)]
    connection_state: ConnectionState,
    #[allow(dead_code)]
    state_machine: ZenohConnectionStateMachine,

    // Peer management
    unique_id: i64,
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

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
enum ConnectionState {
    Disconnected,
    #[allow(dead_code)]
    Connecting,
    #[allow(dead_code)]
    Connected,
    #[allow(dead_code)]
    Failed,
}

impl Default for ConnectionState {
    fn default() -> Self {
        ConnectionState::Disconnected
    }
}

#[allow(dead_code)]
#[derive(Clone)]
struct ZenohConnectionStateMachine {
    // Track connection attempts and retries
    max_retries: i32,
    current_retry: i32,
    retry_delay: f64,
    last_retry_time: f64,
}

#[godot_api]
impl IMultiplayerPeerExtension for ZenohMultiplayerPeer {
    fn init(_base: Base<MultiplayerPeerExtension>) -> Self {
        godot_print!("ZenohMultiplayerPeer initialized");
        godot_print!("Priority channels: 0→255 packet ordering");
        godot_print!("256 virtual channels available");

        Self {
            game_id: GString::new(),
            async_bridge: None,
            connection_state: ConnectionState::Disconnected,
            state_machine: ZenohConnectionStateMachine {
                max_retries: 5,
                current_retry: 0,
                retry_delay: 2.0,
                last_retry_time: 0.0,
            },
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
        godot_print!("Transfer mode set to: {} (Zenoh pub/sub - best effort delivery)", self.transfer_mode);
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
        // Poll async bridge for completed commands
        if let Some(bridge) = &mut self.async_bridge {
            // Process any pending commands and handle their events
            let events = bridge.process_pending_commands();
            for event in events {
                match event {
                    ZenohStateUpdate::ClientConnected { zid, peer_id } => {
                        self.connection_status = 2; // CONNECTED
                        self.unique_id = peer_id;
                        self.zid = GString::from(zid.as_str());
                        godot_print!("CLIENT CONNECTED: ZID: {}, Peer ID: {}", zid, peer_id);
                        
                        // Emit connected_to_server signal for clients
                        self.base_mut().emit_signal("connected_to_server", &[]);
                    },
                    ZenohStateUpdate::ServerCreated { zid } => {
                        self.connection_status = 2; // CONNECTED
                        self.unique_id = 1; // Server is peer 1
                        self.zid = GString::from(zid.as_str());
                        godot_print!("SERVER CREATED: ZID: {}, Peer ID: {}", zid, self.unique_id);
                    },
                    ZenohStateUpdate::ConnectionFailed { error } => {
                        self.connection_status = 0; // DISCONNECTED
                        godot_error!("CONNECTION FAILED: {}", error);
                        
                        // Emit connection_failed signal
                        self.base_mut().emit_signal("connection_failed", &[]);
                    },
                }
            }
        }

        // HOL blocking prevention doesn't require additional polling
        // Protected mode is used by base class
    }

    fn close(&mut self) {
        // Only log if we were actually connected
        if self.connection_status != 0 {
            godot_print!("ZenohMultiplayerPeer connection closed");
        }
        self.connection_status = 0; // DISCONNECTED
        // Clear all packet queues
        self.packet_queues.lock().unwrap().clear();
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
    fn get_unique_id(&self) -> i64 {
        self.unique_id
    }

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
    fn set_transfer_mode(&mut self, mode: i32) -> Error {
        self.transfer_mode = mode;
        godot_print!("Transfer mode set to: {}", mode);
        Error::OK
    }

    #[func]
    fn transfer_channel(&self) -> i32 {
        self.current_channel
    }

    #[func]
    fn set_transfer_channel(&mut self, channel: i32) {
        self.current_channel = channel;
        godot_print!("Virtual channel set to: {}", channel);
    }

    #[func]
    fn get_packet(&mut self) -> PackedByteArray {
        // Process packets by priority: always check lowest channel number first
        // Channels are ordered 0=highest priority, 255=lowest priority
        let mut queues = self.packet_queues.lock().unwrap();
        for priority in 0..=255 {
            if let Some(queue) = queues.get_mut(&priority) {
                if let Some(packet) = queue.pop_front() {
                    // Store sender peer ID for get_packet_peer()
                    self.current_packet_peer = packet.from_peer as i32;
                    
                    godot_print!("DEBUG: Retrieved packet from channel {} (size: {}, from peer: {})", 
                               priority, packet.data.len(), packet.from_peer);
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
        godot_print!("DEBUG: put_packet called with {} bytes on channel {}", p_buffer.len(), self.current_channel);
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

        // Fallback: local queuing when no networking session available
        let mut queues = self.packet_queues.lock().unwrap();
        queues
            .entry(channel)
            .or_insert_with(VecDeque::new)
            .push_back(Packet {
                data: p_buffer.to_vec(),
                from_peer: 0, // Unknown sender for local fallback
            });
        Error::OK
    }

    #[func]
    fn get_available_packet_count(&self) -> i32 {
        let queues = self.packet_queues.lock().unwrap();
        queues.values().map(|q| q.len() as i32).sum()
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
    fn get_max_packet_size(&self) -> i32 {
        self.max_packet_size
    }

    #[func]
    fn poll(&mut self) -> Error {
        Error::OK
    }

    #[func]
    fn demo_hol_blocking_prevention(&mut self) -> PackedByteArray {
        // Get exclusive access to queues for modification scope
        {
            let mut queues = self.packet_queues.lock().unwrap();

            // DEMONSTRATION: Flood high channels (200-220) with packets
            // Then add one packet to channel 0 - it should be processed first
            godot_print!("HOL BLOCKING PREVENTION DEMO:");
            godot_print!("Flooding high channels with packets...");

            // Add many packets to high-number channels (should be blocked)
            for channel in 200..=220 {
                for i in 0..5 {
                    let data = vec![channel as u8, i as u8]; // Use Vec<u8> directly
                    let packet = Packet {
                        data,
                        from_peer: 999, // Demo packets from fake peer
                    };
                    queues
                        .entry(channel)
                        .or_insert_with(VecDeque::new)
                        .push_back(packet);
                }
            }

            // Add ONE critical packet to channel 0 (should be processed first)
            godot_print!("Adding critical packet to channel 0...");
            let critical_data = vec![0u8, 255u8]; // Use Vec<u8> directly - Channel 0 marker, critical flag
            let critical_packet = Packet {
                data: critical_data,
                from_peer: 999, // Demo packet from fake peer
            };
            queues
                .entry(0)
                .or_insert_with(VecDeque::new)
                .push_front(critical_packet);
        } // Release queues lock

        // HOL PREVENTION: get_packet() should return channel 0 first!
        let result = self.get_packet();
        if result.len() >= 2 {
            let channel_returned = result[0] as i32;
            if channel_returned == 0 {
                godot_print!("SUCCESS: Channel 0 critical packet processed FIRST!");
                godot_print!("HOL blocking prevention working correctly");
                godot_print!("High-channel packets properly blocked by low-channel priority");
            } else {
                godot_error!(
                    "FAILURE: Channel {} returned instead of channel 0",
                    channel_returned
                );
                godot_error!("HOL blocking prevention NOT working");
            }
        }

        result
    }

    #[func]
    fn create_client(&mut self, address: GodotString, port: i32) -> Error {
        godot_print!("Creating Zenoh client asynchronously on {}:{}", address, port);

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
    fn is_server(&self) -> bool {
        self.unique_id == 1
    }

    #[func]
    fn get_connection_list(&self) -> Array<i64> {
        Array::new()
    }

    #[func]
    fn close(&mut self) {
        // Only log if we were actually connected
        if self.connection_status != 0 {
            godot_print!("ZenohMultiplayerPeer connection closed");
        }
        self.connection_status = 0; // DISCONNECTED
        // Clear all packet queues
        self.packet_queues.lock().unwrap().clear();
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
        dict
    }

    #[func]
    fn get_channel_info(&self, channel: i32) -> VarDictionary {
        let mut dict = VarDictionary::new();
        dict.set("channel", channel);
        dict.set("packet_count", self.get_channel_packet_count(channel));
        dict.set("priority", if channel == 0 { "highest" } else if channel <= 10 { "high" } else if channel <= 100 { "normal" } else { "low" });
        dict.set("special", "");
        dict
    }
}
