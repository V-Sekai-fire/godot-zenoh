// Copyright (c) 2026-present K. S. Ernest (iFire) Lee
// SPDX-License-Identifier: MIT

use godot::classes::multiplayer_peer::{ConnectionStatus, TransferMode};
use godot::classes::IMultiplayerPeerExtension;
use godot::classes::MultiplayerPeerExtension;

use godot::prelude::*;

use godot::builtin::GString as GodotString;
use godot::global::Error;
use std::sync::{Arc, Mutex};
use std::thread;

type MessageQueue = Arc<Mutex<Vec<(Vec<u8>, i32, i32)>>>;

use crate::networking::ZenohSession;

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

    GetTimestamp,
}

impl std::fmt::Debug for ZenohCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ZenohCommand::CreateServer { port } => write!(f, "CreateServer {{ port: {} }}", port),
            ZenohCommand::CreateClient { address, port } => write!(
                f,
                "CreateClient {{ address: \"{}\", port: {} }}",
                address, port
            ),
            ZenohCommand::SendPacket { data, channel } => write!(
                f,
                "SendPacket {{ data: {} bytes, channel: {} }}",
                data.len(),
                channel
            ),
            ZenohCommand::GetTimestamp => write!(f, "GetTimestamp"),
        }
    }
}
enum ZenohStateUpdate {
    ServerCreated { zid: String },
    ClientConnected { zid: String, peer_id: i64 },
    ConnectionFailed { error: String },
    TimestampObtained { timestamp: i64 },
}

struct ZenohActor {
    session: Option<ZenohSession>,
    game_id: GodotString,
    message_queue: MessageQueue,
}

impl ZenohActor {
    fn new(game_id: GodotString, message_queue: MessageQueue) -> Self {
        Self {
            session: None,
            game_id,
            message_queue,
        }
    }

    async fn handle_command(&mut self, cmd: ZenohCommand) -> Option<ZenohStateUpdate> {
        match cmd {
            ZenohCommand::CreateServer { port } => {
                match ZenohSession::create_server(port, self.game_id.to_string(), None).await {
                    Ok(s) => {
                        let zid = s.get_zid();
                        self.session = Some(s);

                        if let Some(sess) = &mut self.session {
                            // FIXED: Set callback before setting up channels so subscribers use it
                            let message_queue_clone = self.message_queue.clone();
                            sess.set_message_callback(Box::new(
                                move |packet: PackedByteArray, channel: i32, sender_peer: i32| {
                                    let mut queue = message_queue_clone.lock().unwrap();
                                    queue.push((packet.to_vec(), channel, sender_peer));
                                },
                            ));

                            // FIXED: Setup channels - callback will be used by subscribers
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
                        }

                        Some(ZenohStateUpdate::ServerCreated { zid })
                    }
                    Err(e) => Some(ZenohStateUpdate::ConnectionFailed {
                        error: e.to_string(),
                    }),
                }
            }
            ZenohCommand::CreateClient { address, port } => {
                match ZenohSession::create_client(address, port, self.game_id.to_string()).await {
                    Ok(s) => {
                        let zid = s.get_zid();
                        let peer_id = s.get_peer_id();
                        self.session = Some(s);

                        if let Some(sess) = &mut self.session {
                            // FIXED: Set callback before setting up channels so subscribers use it
                            let message_queue_clone = self.message_queue.clone();
                            sess.set_message_callback(Box::new(
                                move |packet: PackedByteArray, channel: i32, sender_peer: i32| {
                                    let mut queue = message_queue_clone.lock().unwrap();
                                    queue.push((packet.to_vec(), channel, sender_peer));
                                },
                            ));

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
                        }

                        Some(ZenohStateUpdate::ClientConnected { zid, peer_id })
                    }
                    Err(e) => Some(ZenohStateUpdate::ConnectionFailed {
                        error: e.to_string(),
                    }),
                }
            }
            ZenohCommand::SendPacket { data, channel } => {
                if let Some(sess) = &mut self.session {
                    let _result = sess
                        .send_packet(&data, self.game_id.to_string(), channel)
                        .await;
                }
                None
            }
            ZenohCommand::GetTimestamp => {
                if let Some(sess) = &mut self.session {
                    // Use proper Zenoh HLC timestamp for distributed linearizability
                    let hlc_timestamp = sess.get_timestamp();
                    let nanos = hlc_timestamp.get_time().as_nanos() as i64;
                    Some(ZenohStateUpdate::TimestampObtained { timestamp: nanos })
                } else {
                    // No HLC available - router disconnection, fail with panic
                    panic!("No Zenoh session available for HLC timestamp - router disconnection");
                }
            }
        }
    }
}

struct ZenohAsyncBridge {
    command_queue: Arc<Mutex<Vec<ZenohCommand>>>,
    event_queue: Arc<Mutex<Vec<ZenohStateUpdate>>>,
    join_handle: Option<thread::JoinHandle<()>>,
    stop_flag: Arc<Mutex<bool>>,
}

impl ZenohAsyncBridge {
    fn new(game_id: GodotString, message_queue: MessageQueue) -> Self {
        let command_queue = Arc::new(Mutex::new(Vec::new()));
        let event_queue = Arc::new(Mutex::new(Vec::new()));
        let stop_flag = Arc::new(Mutex::new(false));

        let cmd_queue_clone = Arc::clone(&command_queue);
        let event_queue_clone = Arc::clone(&event_queue);
        let stop_flag_clone = Arc::clone(&stop_flag);

        let actor = ZenohActor::new(game_id, message_queue);

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
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;

        rt.block_on(async {
            loop {
                if *stop_flag.lock().unwrap() {
                    break;
                }

                let cmds = {
                    let mut queue = command_queue.lock().unwrap();
                    std::mem::take(&mut *queue)
                };

                if !cmds.is_empty() {
                    for cmd in cmds {
                        if let Some(event) = actor.handle_command(cmd).await {
                            event_queue.lock().unwrap().push(event);
                        }
                    }
                } else {
                    tokio::task::yield_now().await;
                }
            }
        });

        Ok(())
    }

    fn send_command(&self, cmd: ZenohCommand) -> Result<(), Box<dyn std::error::Error>> {
        let mut queue = self.command_queue.lock().unwrap();
        queue.push(cmd);
        // godot_print!("Command queued for worker thread");
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
        // godot_print!("Dropping ZenohAsyncBridge");
        *self.stop_flag.lock().unwrap() = true;
        if let Some(join_handle) = self.join_handle.take() {
            let _ = join_handle.join();
        }
    }
}

/// A Zenoh-based multiplayer peer implementation for Godot.
///
/// This struct provides a custom multiplayer peer that uses the Zenoh protocol
/// for distributed communication between game clients. It extends Godot's
/// MultiplayerPeerExtension to integrate with the high-level multiplayer API.
///
/// Based on proven zenoh-tetris multiplayer architecture:
/// - Server publishes game state, subscribes to client actions
/// - Clients subscribe to game state, publish actions
/// - Topic hierarchy: godot/game/{game_id}/channel{NNN}
#[derive(GodotClass)]
#[class(base=MultiplayerPeerExtension, tool)]
pub struct ZenohMultiplayerPeer {
    #[export]
    game_id: GodotString,

    async_bridge: Option<Box<ZenohAsyncBridge>>,
    unique_id: i32,
    connection_status: i32,
    transfer_mode: i32,

    current_channel: i32,
    max_packet_size: i32,

    current_packet_peer: i32,

    zid: GodotString,

    // Distributed timestamp from Zenoh HLC
    current_timestamp: i64,

    // FIXED: CRITICAL BUG - Message callback now properly wires networking.rs subscribers to peer.rs message_queue
    // FIXED: MessageCallback set in ZenohActor.handle_command before channel setup so subscribers use the peer queue
    // FIXED: Messages from ZenohSession subscribers now deliver directly to get_packet() via shared Arc message_queue
    message_queue: MessageQueue,

    base: Base<MultiplayerPeerExtension>,
}

#[godot_api]
impl IMultiplayerPeerExtension for ZenohMultiplayerPeer {
    fn init(_base: Base<MultiplayerPeerExtension>) -> Self {
        Self {
            game_id: GString::new(),
            async_bridge: None,
            unique_id: 1,
            connection_status: 0,
            transfer_mode: 0,
            current_channel: 0,
            max_packet_size: 1472,
            current_packet_peer: 0,
            zid: GString::from(""),
            current_timestamp: 0,
            message_queue: Arc::new(Mutex::new(Vec::new())),
            base: _base,
        }
    }
    fn get_available_packet_count(&self) -> i32 {
        if let Ok(queue) = self.message_queue.try_lock() {
            queue.len() as i32
        } else {
            0
        }
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
        // godot_print!("Virtual channel set to: {}", channel);
    }

    fn get_transfer_channel(&self) -> i32 {
        self.current_channel
    }

    fn set_transfer_mode(&mut self, mode: TransferMode) {
        self.transfer_mode = match mode {
            TransferMode::UNRELIABLE => 0,
            TransferMode::UNRELIABLE_ORDERED => 1,
            TransferMode::RELIABLE => 1,
            _ => 0,
        };
        // godot_print!(
        //     "Transfer mode set to: {} (Zenoh pub/sub - best effort delivery)",
        //     self.transfer_mode
        // );
    }

    fn get_transfer_mode(&self) -> TransferMode {
        self.get_packet_mode()
    }

    fn set_target_peer(&mut self, _peer_id: i32) {
        // godot_print!("Target peer setting not applicable for virtual channels");
    }

    fn get_packet_peer(&self) -> i32 {
        self.current_packet_peer
    }

    fn is_server(&self) -> bool {
        self.unique_id == 1
    }

    fn poll(&mut self) {
        if let Some(bridge) = &self.async_bridge {
            let events = bridge.get_events();
            for event in events {
                match event {
                    ZenohStateUpdate::ClientConnected { zid, peer_id } => {
                        self.connection_status = 2;
                        self.unique_id = peer_id as i32;
                        self.zid = GString::from(zid.as_str());
                        godot_print!("CLIENT CONNECTED: ZID: {}, Peer ID: {}", zid, peer_id);

                        self.base_mut().emit_signal("connected_to_server", &[]);
                    }
                    ZenohStateUpdate::ServerCreated { zid } => {
                        self.connection_status = 2;
                        self.unique_id = 1;
                        self.zid = GString::from(zid.as_str());
                        godot_print!("SERVER CREATED: ZID: {}, Peer ID: {}", zid, self.unique_id);
                    }
                    ZenohStateUpdate::ConnectionFailed { error } => {
                        self.connection_status = 0;
                        godot_error!("CONNECTION FAILED: {}", error);

                        self.base_mut().emit_signal("connection_failed", &[]);
                    }
                    ZenohStateUpdate::TimestampObtained { timestamp } => {
                        // Store the distributed HLC timestamp for linearizability testing
                        self.current_timestamp = timestamp;
                        godot_print!("HLC timestamp obtained: {}", timestamp);
                    }
                }
            }
        } else {
            // godot_print!("No async bridge available for polling");
        }

        // HOL blocking prevention doesn't require additional polling
        // Worker thread handles async operations
    }

    fn close(&mut self) {
        if self.connection_status != 0 {
            // godot_print!("ZenohMultiplayerPeer connection closed");
        }
        self.connection_status = 0;
    }

    fn disconnect_peer(&mut self, _peer_id: i32, _force: bool) {
        // godot_print!("Peer disconnection not applicable for virtual channels");
    }

    fn get_unique_id(&self) -> i32 {
        self.unique_id
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
        // godot_print!("get_zid() returning: '{}'", self.zid.to_string());
        self.zid.to_string()
    }

    #[func]
    fn get_hlc_timestamp(&mut self) -> i64 {
        if let Some(bridge) = &self.async_bridge {
            // Request timestamp from Zenoh HLC via async bridge
            if let Err(_e) = bridge.send_command(ZenohCommand::GetTimestamp) {
                // Fail loudly if HLC unavailable - router disconnection issue
                panic!("Failed to request HLC timestamp - router disconnection");
            }

            // Poll to update timestamp (this would ideally be event-driven)
            self.poll();

            // Return the distributed HLC timestamp
            self.current_timestamp
        } else {
            // No bridge available - router disconnection, fail loudly
            panic!("No network bridge available for HLC timestamp - router disconnection");
        }
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
        // godot_print!("Transfer mode set to: {}", mode);
        Error::OK
    }

    #[func]
    fn transfer_channel(&self) -> i32 {
        self.current_channel
    }

    #[func]
    fn get_packet(&mut self) -> PackedByteArray {
        if let Ok(mut queue) = self.message_queue.try_lock() {
            if !queue.is_empty() {
                let (data_vec, channel, peer_id) = queue.remove(0);
                self.current_channel = channel;
                self.current_packet_peer = peer_id;
                godot_print!(
                    "RECEIVED: packet with {} bytes on channel {}",
                    data_vec.len(),
                    channel
                );
                return PackedByteArray::from(data_vec);
            }
        }

        // No packets available
        godot_print!("RECEIVED: no packets available");
        PackedByteArray::new()
    }

    #[func]
    fn put_packet(&mut self, p_buffer: PackedByteArray) -> Error {
        godot_print!("SENT:");
        self.put_packet_on_channel(p_buffer, self.current_channel)
    }

    #[func]
    fn put_packet_on_channel(&mut self, p_buffer: PackedByteArray, channel: i32) -> Error {
        // godot_print!(
        //     "put_packet_on_channel called: {} bytes on channel {}",
        //     p_buffer.len(),
        //     channel
        // );
        // Use async bridge for sending packets
        if let Some(bridge) = &self.async_bridge {
            let data_vec = p_buffer.to_vec();
            // godot_print!("Sending packet via async bridge: {} bytes", data_vec.len());
            if let Err(e) = bridge.send_command(ZenohCommand::SendPacket {
                data: data_vec,
                channel,
            }) {
                godot_error!("Failed to send packet via async bridge: {:?}", e);
                return Error::FAILED;
            }
            // godot_print!("Packet queued for sending on channel {}", channel);
            return Error::OK;
        }

        // No networking session available - cannot send packet
        godot_error!("No networking session available for packet transmission");
        Error::FAILED
    }

    /// Creates a Zenoh client that connects to a server.
    ///
    /// # Arguments
    /// * `address` - The server address to connect to
    /// * `port` - The port number to connect to
    ///
    /// # Returns
    /// Error::OK on success, or an error code on failure
    #[func]
    fn create_client(&mut self, address: GodotString, port: i32) -> Error {
        // godot_print!("create_client called: {}:{}", address, port);
        // Close any existing connection first
        self.close();

        // Set status to CONNECTING before attempting connection
        self.connection_status = 1; // CONNECTING
                                    // godot_print!("Status set to CONNECTING");

        // Initialize async bridge if not exists
        if self.async_bridge.is_none() {
            // godot_print!("Initializing async bridge for client");
            self.async_bridge = Some(Box::new(ZenohAsyncBridge::new(
                self.game_id.clone(),
                self.message_queue.clone(),
            )));
        }

        // Send async command to create client
        if let Some(bridge) = &mut self.async_bridge {
            // godot_print!("Sending create client command to async bridge");
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

    /// Creates a Zenoh server that listens for client connections.
    ///
    /// # Arguments
    /// * `port` - The port number to listen on
    /// * `_max_clients` - Maximum number of clients (currently unused)
    ///
    /// # Returns
    /// Error::OK on success, or an error code on failure
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
                self.game_id.clone(),
                self.message_queue.clone(),
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
    fn get_channel_packet_count(&self, _channel: i32) -> i32 {
        0
    }
}
