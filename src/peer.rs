use godot::classes::multiplayer_peer::{ConnectionStatus, TransferMode};
use godot::classes::IMultiplayerPeerExtension;
use godot::classes::MultiplayerPeerExtension;
use godot::prelude::*;

use godot::builtin::GString as GodotString;
use godot::global::Error;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use crate::networking::{Packet, ZenohSession};
use tokio::runtime::Runtime;
// ZBuf moved to zenoh::bytes in 1.7.2

#[derive(GodotClass)]
#[class(base=MultiplayerPeerExtension, tool)]
pub struct ZenohMultiplayerPeer {
    #[export]
    game_id: GodotString,

    // Real Zenoh networking session
    zenoh_session: Option<Arc<Mutex<ZenohSession>>>,

    // Peer management
    unique_id: i64,
    connection_status: i32,
    transfer_mode: i32,

    // HOL Blocking Prevention: Virtual Channel System
    packet_queues: Arc<Mutex<HashMap<i32, VecDeque<Packet>>>>,
    current_channel: i32,
    max_packet_size: i32,

    base: Base<MultiplayerPeerExtension>,
}

#[godot_api]
impl IMultiplayerPeerExtension for ZenohMultiplayerPeer {
    fn init(_base: Base<MultiplayerPeerExtension>) -> Self {
        godot_print!("ZenohMultiplayerPeer initialized");

        Self {
            game_id: GString::new(),
            zenoh_session: None,
            unique_id: 1,
            connection_status: 0, // DISCONNECTED
            transfer_mode: 0,     // UNRELIABLE
            packet_queues: Arc::new(Mutex::new(HashMap::new())),
            current_channel: 0,
            max_packet_size: 1472, // UDP MTU - Zenoh overhead
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
        self.transfer_mode = match mode {
            TransferMode::UNRELIABLE => 0,
            TransferMode::UNRELIABLE_ORDERED => 1,
            TransferMode::RELIABLE => 2,
            _ => 0, // Default to UNRELIABLE for unknown modes
        };
        godot_print!("Transfer mode set to: {}", self.transfer_mode);
    }

    fn get_transfer_mode(&self) -> TransferMode {
        self.get_packet_mode()
    }

    fn set_target_peer(&mut self, _peer_id: i32) {
        // HOL blocking prevention doesn't use target peer concept
        godot_print!("Target peer setting not applicable for HOL blocking prevention");
    }

    fn get_packet_peer(&self) -> i32 {
        0 // Default - all packets are targeted
    }

    fn is_server(&self) -> bool {
        self.unique_id == 1
    }

    fn poll(&mut self) {
        // HOL blocking prevention doesn't require polling
        // Protected mode is used by base class
    }

    fn close(&mut self) {
        self.connection_status = 0; // DISCONNECTED
        godot_print!("ZenohMultiplayerPeer connection closed (HOL blocking prevention)");
        // Clear all packet queues
        self.packet_queues.lock().unwrap().clear();
    }

    fn disconnect_peer(&mut self, _peer_id: i32, _force: bool) {
        // HOL blocking prevention peer handles packets, not peer connections
        godot_print!("Peer disconnection not applicable for HOL blocking prevention");
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
        // 🛡️🏗️ HOL BLOCKING PREVENTION: Always process lowest priority first
        // Zenoh-native: Topics with priority 0 processed before higher priorities
        let mut queues = self.packet_queues.lock().unwrap();

        // Process priorities in HOL prevention order (0=highest, 255=lowest)
        for priority in 0..=255 {
            if let Some(queue) = queues.get_mut(&priority) {
                if let Some(packet) = queue.pop_front() {
                    godot_print!(
                        "✅ HOL BLOCKING PREVENTION: Processing packet on topic {} (priority: {})",
                        packet.topic,
                        packet.hol_priority
                    );

                    // Convert Vec<u8> directly to PackedByteArray
                    return PackedByteArray::from_iter(packet.data.iter().copied());
                }
            }
        }
        PackedByteArray::new()
    }

    #[func]
    fn put_packet(&mut self, p_buffer: PackedByteArray) -> Error {
        self.put_packet_on_channel(p_buffer, self.current_channel)
    }

    #[func]
    fn put_packet_on_channel(&mut self, p_buffer: PackedByteArray, channel: i32) -> Error {
        // 🛡️🏗️ HOL BLOCKING PREVENTION: Route through Zenoh networking when available
        if let Some(zenoh_session_arc) = &self.zenoh_session {
            // REAL NETWORKING: Send via Zenoh pub/sub
            let session_clone = Arc::clone(zenoh_session_arc);
            let data_vec = p_buffer.to_vec();
            let game_id = self.game_id.clone();
            let data_len = data_vec.len();

            std::thread::spawn(move || {
                let runtime = Runtime::new().unwrap();
                runtime.block_on(async {
                    let session = session_clone.lock().unwrap();
                    let error = session.send_packet(&data_vec, game_id, channel).await;
                    if error != Error::OK {
                        godot_error!("Zenoh packet send failed: {:?}", error);
                    }
                });
            });

            godot_print!(
                "📤 HOL PREVENTION: Packet sent via Zenoh on channel {} (size: {})",
                channel,
                data_len
            );
            return Error::OK;
        }

        // FALLBACK: Local queuing when no Zenoh session (HOL prevention still applies)
        let mut queues = self.packet_queues.lock().unwrap();
        let fallback_topic =
            GString::from(format!("game/{}/fallback/{}", self.game_id, channel).as_str());
        queues
            .entry(channel)
            .or_insert_with(VecDeque::new)
            .push_back(Packet {
                data: p_buffer.to_vec(), // Use Vec<u8> directly
                topic: fallback_topic,
                hol_priority: channel,
            });
        godot_print!(
            "🔄 HOL PREVENTION: Packet queued locally on channel {} (Zenoh session unavailable)",
            channel
        );
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
            godot_print!("🎯 HOL BLOCKING PREVENTION DEMO:");
            godot_print!("Flooding high channels with packets...");

            // Add many packets to high-number channels (should be blocked)
            for channel in 200..=220 {
                for i in 0..5 {
                    let data = vec![channel as u8, i as u8]; // Use Vec<u8> directly
                    let topic =
                        GString::from(format!("game/{}/demo/high_priority", self.game_id).as_str());
                    let packet = Packet {
                        data,
                        topic,
                        hol_priority: channel,
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
            let critical_topic =
                GString::from(format!("game/{}/demo/critical", self.game_id).as_str());
            let critical_packet = Packet {
                data: critical_data,
                topic: critical_topic,
                hol_priority: 0,
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
                godot_print!("✅ SUCCESS: Channel 0 critical packet processed FIRST!");
                godot_print!("✅ HOL blocking prevention working correctly");
                godot_print!("✅ High-channel packets properly blocked by low-channel priority");
            } else {
                godot_error!(
                    "❌ FAILURE: Channel {} returned instead of channel 0",
                    channel_returned
                );
                godot_error!("❌ HOL blocking prevention NOT working");
            }
        }

        result
    }

    #[func]
    fn create_client(&mut self, address: GodotString, port: i32) -> Error {
        godot_print!("🚀 Creating Zenoh CLIENT with FULL networking + HOL prevention...");
        let runtime = Runtime::new().unwrap();
        let packet_queues = Arc::clone(&self.packet_queues);

        let session_result = runtime.block_on(async {
            ZenohSession::create_client(address, port, packet_queues, self.game_id.clone()).await
        });

        match session_result {
            Ok(session) => {
                let session_arc = Arc::new(Mutex::new(session));
                self.zenoh_session = Some(Arc::clone(&session_arc));

                // Wire up all virtual channels 0-255 to Zenoh
                let session_for_channels = Arc::clone(&session_arc);
                match runtime.block_on(async move {
                    let session_guard = session_for_channels.lock().unwrap();
                    for channel in 0..=255 {
                        let result = session_guard.setup_channel(channel);
                        if result != Error::OK {
                            return Err(format!("Channel setup failed for {}", channel));
                        }
                    }
                    Ok(())
                }) {
                    Ok(_) => {
                        godot_print!("✅ All 256 HOL virtual channels wired to Zenoh networking")
                    }
                    Err(e) => {
                        godot_error!("❌ Failed to setup Zenoh channels: {:?}", e);
                        return Error::FAILED;
                    }
                }

                self.unique_id = (rand::random::<u32>() % 999) as i64 + 2; // 2-1000 range
                self.connection_status = 2; // CONNECTED
                godot_print!("✅ Zenoh client with 256-channel HOL prevention ACTIVE!");
                godot_print!("📡 Real peer-to-peer networking + HOL protection enabled");
                Error::OK
            }
            Err(e) => {
                godot_error!("❌ Failed to create Zenoh client: {:?}", e);
                Error::FAILED
            }
        }
    }

    #[func]
    fn create_server(&mut self, port: i32, max_clients: i32) -> Error {
        godot_print!("🚀 Creating Zenoh SERVER with FULL networking + HOL prevention...");
        let runtime = Runtime::new().unwrap();
        let packet_queues = Arc::clone(&self.packet_queues);

        let session_result = runtime.block_on(async {
            ZenohSession::create_server(port, max_clients, packet_queues, self.game_id.clone())
                .await
        });

        match session_result {
            Ok(session) => {
                let session_arc = Arc::new(Mutex::new(session));
                self.zenoh_session = Some(Arc::clone(&session_arc));

                // Wire up all virtual channels 0-255 to Zenoh
                let session_for_channels = Arc::clone(&session_arc);
                match runtime.block_on(async move {
                    let session_guard = session_for_channels.lock().unwrap();
                    for channel in 0..=255 {
                        let result = session_guard.setup_channel(channel);
                        if result != Error::OK {
                            return Err(format!("Channel setup failed for {}", channel));
                        }
                    }
                    Ok(())
                }) {
                    Ok(_) => {
                        godot_print!("✅ All 256 HOL virtual channels wired to Zenoh networking")
                    }
                    Err(e) => {
                        godot_error!("❌ Failed to setup Zenoh channels: {:?}", e);
                        return Error::FAILED;
                    }
                }

                self.unique_id = 1; // Server gets ID 1
                self.connection_status = 2; // CONNECTED
                godot_print!("✅ Zenoh server with 256-channel HOL prevention ACTIVE!");
                godot_print!("🎯 Peer-to-peer networking + HOL protection enabled");
                Error::OK
            }
            Err(e) => {
                godot_error!("❌ Failed to create Zenoh server: {:?}", e);
                Error::FAILED
            }
        }
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
        self.connection_status = 0; // DISCONNECTED
        godot_print!("ZenohMultiplayerPeer connection closed (HOL blocking prevention)");
        // Clear all packet queues
        self.packet_queues.lock().unwrap().clear();
    }
}
