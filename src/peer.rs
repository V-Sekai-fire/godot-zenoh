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

use crate::networking::ZenohSession;

#[derive(Debug)]
enum ZenohCommand {
    CreateServer {
        port: i32,
    },
    #[allow(dead_code)]
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
    ClientConnected { zid: String, peer_id: i64 },
    ConnectionFailed { error: String },
    PacketReceived { data: Vec<u8>, peer_id: i32, channel: i32 },
    /// A discovery beacon arrived — no packet data, just peer identity.
    PeerDiscovered { peer_id: i32 },
}

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
                match ZenohSession::create_server(port, self.game_id.to_string(), None).await {
                    Ok(mut s) => {
                        let zid = s.get_zid();
                        if let Err(e) = s.setup_discovery().await {
                            return Some(ZenohStateUpdate::ConnectionFailed {
                                error: format!("Server discovery setup failed: {}", e),
                            });
                        }
                        for channel in 0..=255 {
                            if let Err(_e) = s.setup_channel(channel).await {
                                return Some(ZenohStateUpdate::ConnectionFailed {
                                    error: format!("Server channel setup failed for {}", channel),
                                });
                            }
                        }
                        self.session = Some(s);
                        Some(ZenohStateUpdate::ServerCreated { zid })
                    }
                    Err(e) => Some(ZenohStateUpdate::ConnectionFailed {
                        error: e.to_string(),
                    }),
                }
            }
            ZenohCommand::CreateClient { address, port } => {
                match ZenohSession::create_client(address, port, self.game_id.to_string()).await {
                    Ok(mut s) => {
                        let zid = s.get_zid();
                        let peer_id = s.get_peer_id();
                        if let Err(e) = s.setup_discovery().await {
                            return Some(ZenohStateUpdate::ConnectionFailed {
                                error: format!("Client discovery setup failed: {}", e),
                            });
                        }
                        for channel in 0..=255 {
                            if let Err(_e) = s.setup_channel(channel).await {
                                return Some(ZenohStateUpdate::ConnectionFailed {
                                    error: format!("Client channel setup failed for {}", channel),
                                });
                            }
                        }
                        // Announce ourselves so the server (and other clients) discover us.
                        if let Err(e) = s.send_announce().await {
                            godot_error!("Failed to send client announcement: {}", e);
                        }
                        self.session = Some(s);
                        Some(ZenohStateUpdate::ClientConnected { zid, peer_id })
                    }
                    Err(e) => Some(ZenohStateUpdate::ConnectionFailed {
                        error: e.to_string(),
                    }),
                }
            }
            ZenohCommand::SendPacket { data, channel } => {
                if let Some(sess) = &mut self.session {
                    let game_id = self.game_id.to_string();
                    let _ = sess.send_packet(&data, game_id, channel).await;
                }
                None
            }
        }
    }

    /// Drain received packets from the session and return them as events.
    fn drain_received_packets(&mut self) -> Vec<ZenohStateUpdate> {
        let mut events = Vec::new();
        if let Some(sess) = &mut self.session {
            for pkt in sess.drain_packets() {
                if pkt.raw.len() < 8 {
                    continue;
                }
                let sender_bytes: [u8; 8] = pkt.raw[..8].try_into().unwrap();
                let sender_id = i64::from_le_bytes(sender_bytes) as i32;

                // Discovery beacons (channel == i32::MIN) carry only peer_id.
                // Emit peer discovery without forwarding to Godot's packet queue.
                if pkt.channel == i32::MIN {
                    events.push(ZenohStateUpdate::PeerDiscovered { peer_id: sender_id });
                    continue;
                }

                let data = pkt.raw[8..].to_vec();
                events.push(ZenohStateUpdate::PacketReceived {
                    data,
                    peer_id: sender_id,
                    channel: pkt.channel,
                });
            }
        }
        events
    }
}

struct ZenohAsyncBridge {
    command_queue: Arc<Mutex<Vec<ZenohCommand>>>,
    event_queue: Arc<Mutex<Vec<ZenohStateUpdate>>>,
    join_handle: Option<thread::JoinHandle<()>>,
    stop_flag: Arc<Mutex<bool>>,
}

impl ZenohAsyncBridge {
    fn new(game_id: GodotString) -> Self {
        let command_queue = Arc::new(Mutex::new(Vec::new()));
        let event_queue = Arc::new(Mutex::new(Vec::new()));
        let stop_flag = Arc::new(Mutex::new(false));

        let cmd_queue_clone = Arc::clone(&command_queue);
        let event_queue_clone = Arc::clone(&event_queue);
        let stop_flag_clone = Arc::clone(&stop_flag);

        let actor = ZenohActor::new(game_id);

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

                for cmd in cmds {
                    if let Some(event) = actor.handle_command(cmd).await {
                        event_queue.lock().unwrap().push(event);
                    }
                }

                // Drain received packets from session every loop.
                let recv_events = actor.drain_received_packets();
                if !recv_events.is_empty() {
                    event_queue.lock().unwrap().extend(recv_events);
                } else {
                    tokio::task::yield_now().await;
                }
            }
        });

        Ok(())
    }

    fn send_command(&self, cmd: ZenohCommand) -> Result<(), Box<dyn std::error::Error>> {
        self.command_queue.lock().unwrap().push(cmd);
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
        *self.stop_flag.lock().unwrap() = true;
        if let Some(join_handle) = self.join_handle.take() {
            let _ = join_handle.join();
        }
    }
}

/// A Zenoh-based multiplayer peer implementation for Godot.
///
/// Extends MultiplayerPeerExtension so Godot's high-level multiplayer API
/// (including RPCs) routes packets through Zenoh pub/sub.
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
    current_packet_channel: i32,

    /// Buffered packets waiting to be consumed via get_packet_script / get_packet.
    packet_queue: Vec<(Vec<u8>, i32, i32)>, // (data, peer_id, channel)

    /// Peer IDs that have been announced via peer_connected signal.
    known_peers: std::collections::HashSet<i32>,

    zid: GodotString,

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
            transfer_mode: 2, // default RELIABLE
            current_channel: 0,
            max_packet_size: 65536,
            current_packet_peer: 0,
            current_packet_channel: 0,
            packet_queue: Vec::new(),
            known_peers: std::collections::HashSet::new(),
            zid: GString::from(""),
            base: _base,
        }
    }

    fn get_available_packet_count(&self) -> i32 {
        self.packet_queue.len() as i32
    }

    fn get_max_packet_size(&self) -> i32 {
        self.max_packet_size
    }

    fn get_packet_channel(&self) -> i32 {
        self.current_packet_channel
    }

    fn get_packet_mode(&self) -> TransferMode {
        match self.transfer_mode {
            0 => TransferMode::UNRELIABLE,
            1 => TransferMode::UNRELIABLE_ORDERED,
            _ => TransferMode::RELIABLE,
        }
    }

    fn set_transfer_channel(&mut self, channel: i32) {
        self.current_channel = channel;
    }

    fn get_transfer_channel(&self) -> i32 {
        self.current_channel
    }

    fn set_transfer_mode(&mut self, mode: TransferMode) {
        self.transfer_mode = match mode {
            TransferMode::UNRELIABLE => 0,
            TransferMode::UNRELIABLE_ORDERED => 1,
            TransferMode::RELIABLE => 2,
            _ => 2,
        };
    }

    fn get_transfer_mode(&self) -> TransferMode {
        self.get_packet_mode()
    }

    fn set_target_peer(&mut self, _peer_id: i32) {
        // Zenoh pub/sub is broadcast; Godot's MultiplayerAPI filters by destination
        // encoded in the packet payload, so no per-peer routing is needed here.
    }

    fn get_packet_peer(&self) -> i32 {
        self.current_packet_peer
    }

    fn is_server(&self) -> bool {
        self.unique_id == 1
    }

    fn poll(&mut self) {
        if let Some(bridge) = &self.async_bridge {
            for event in bridge.get_events() {
                match event {
                    ZenohStateUpdate::ClientConnected { zid, peer_id } => {
                        self.connection_status = 2;
                        self.unique_id = peer_id as i32;
                        self.zid = GString::from(zid.as_str());
                        godot_print!("CLIENT CONNECTED: ZID: {}, Peer ID: {}", zid, peer_id);
                        // Tell SceneMultiplayer the connection succeeded (triggers
                        // multiplayer.connected_to_server signal in GDScript).
                        self.base_mut().emit_signal("connection_succeeded", &[]);
                        // Announce server as a known peer (id=1 by Godot convention).
                        self.known_peers.insert(1);
                        self.base_mut().emit_signal("peer_connected", &[1i64.to_variant()]);
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
                    ZenohStateUpdate::PacketReceived { data, peer_id, channel } => {
                        // Announce any previously-unseen peer (fallback discovery via data).
                        self.announce_peer_if_new(peer_id);
                        self.packet_queue.push((data, peer_id, channel));
                    }
                    ZenohStateUpdate::PeerDiscovered { peer_id } => {
                        self.announce_peer_if_new(peer_id);
                    }
                }
            }
        }
    }

    /// Called by Godot's MultiplayerAPI to read the next available packet.
    fn get_packet_script(&mut self) -> PackedByteArray {
        if let Some((data, peer_id, channel)) = self.packet_queue.first().cloned() {
            self.packet_queue.remove(0);
            self.current_packet_peer = peer_id;
            self.current_packet_channel = channel;
            PackedByteArray::from(data.as_slice())
        } else {
            PackedByteArray::new()
        }
    }

    /// Called by Godot's MultiplayerAPI to send a packet.
    fn put_packet_script(&mut self, p_buffer: PackedByteArray) -> Error {
        let channel = self.current_channel;
        if let Some(bridge) = &self.async_bridge {
            if let Err(e) = bridge.send_command(ZenohCommand::SendPacket {
                data: p_buffer.to_vec(),
                channel,
            }) {
                godot_error!("Failed to queue packet: {:?}", e);
                return Error::FAILED;
            }
            return Error::OK;
        }
        godot_error!("No networking session available");
        Error::FAILED
    }

    fn close(&mut self) {
        self.connection_status = 0;
        self.packet_queue.clear();
        self.known_peers.clear();
    }

    fn disconnect_peer(&mut self, _peer_id: i32, _force: bool) {}

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

impl ZenohMultiplayerPeer {
    fn announce_peer_if_new(&mut self, peer_id: i32) {
        if self.known_peers.insert(peer_id) {
            godot_print!("PEER CONNECTED: {}", peer_id);
            self.base_mut()
                .emit_signal("peer_connected", &[(peer_id as i64).to_variant()]);
        }
    }
}

#[godot_api]
impl ZenohMultiplayerPeer {
    #[func]
    fn get_zid(&self) -> String {
        self.zid.to_string()
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
        Error::OK
    }

    #[func]
    fn transfer_channel(&self) -> i32 {
        self.current_channel
    }

    /// GDScript-callable get_packet that also pops from the packet queue.
    #[func]
    fn get_packet(&mut self) -> PackedByteArray {
        self.get_packet_script()
    }

    /// GDScript-callable put_packet.
    #[func]
    fn put_packet(&mut self, p_buffer: PackedByteArray) -> Error {
        self.put_packet_script(p_buffer)
    }

    #[func]
    fn put_packet_on_channel(&mut self, p_buffer: PackedByteArray, channel: i32) -> Error {
        let saved = self.current_channel;
        self.current_channel = channel;
        let result = self.put_packet_script(p_buffer);
        self.current_channel = saved;
        result
    }

    #[func]
    fn create_client(&mut self, address: GodotString, port: i32) -> Error {
        self.close();
        self.connection_status = 1;

        if self.async_bridge.is_none() {
            self.async_bridge = Some(Box::new(ZenohAsyncBridge::new(self.game_id.clone())));
        }

        if let Some(bridge) = &self.async_bridge {
            if let Err(e) = bridge.send_command(ZenohCommand::CreateClient {
                address: address.to_string(),
                port,
            }) {
                godot_error!("Failed to send create client command: {:?}", e);
                self.connection_status = 0;
                return Error::FAILED;
            }
        }

        godot_print!("Client creation initiated - status: CONNECTING");
        Error::OK
    }

    #[func]
    fn create_server(&mut self, port: i32, _max_clients: i32) -> Error {
        godot_print!("Creating Zenoh server asynchronously on port {}", port);
        self.close();
        self.connection_status = 1;

        if self.async_bridge.is_none() {
            self.async_bridge = Some(Box::new(ZenohAsyncBridge::new(self.game_id.clone())));
        }

        if let Some(bridge) = &self.async_bridge {
            if let Err(e) = bridge.send_command(ZenohCommand::CreateServer { port }) {
                godot_error!("Failed to send create server command: {:?}", e);
                self.connection_status = 0;
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
            "localhost:7447".to_string()
        } else {
            "".to_string()
        }
    }

    #[func]
    fn get_connected_clients_count(&self) -> i32 {
        0
    }

    #[func]
    fn get_channel_packet_count(&self, _channel: i32) -> i32 {
        0
    }
}
