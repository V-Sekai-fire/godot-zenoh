use godot::prelude::*;
use godot::classes::MultiplayerPeerExtension;
use godot::classes::IMultiplayerPeerExtension;
use godot::classes::multiplayer_peer::{TransferMode, ConnectionStatus};

use godot::global::Error;
use godot::builtin::GString as GodotString;
use std::collections::{VecDeque, HashMap};
use std::sync::{Arc, Mutex};

use crate::networking::{ZenohSession, Packet};
use tokio::runtime::Runtime;



#[derive(GodotClass)]
#[class(base=MultiplayerPeerExtension, tool)]
pub struct ZenohMultiplayerPeer {
    #[export]
    game_id: GodotString,

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
        godot_print!("ZenohMultiplayerPeer initializing with HOL blocking prevention");
        godot_print!("🎯 HOL BLOCKING PREVENTION ACTIVE: Channels processed 0→255 priority order");
        godot_print!("🛡️ Critical packets on low channels always processed first");

        Self {
            game_id: GString::new(),
            unique_id: 1,
            connection_status: 0, // DISCONNECTED
            transfer_mode: 0, // UNRELIABLE
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
    fn set_transfer_mode(&mut self, mode: i32) {
        self.transfer_mode = mode;
        godot_print!("Transfer mode set to: {}", mode);
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
        // 🛡️🏗️ HOL BLOCKING PREVENTION: Always process lowest channel number first
        // This prevents higher-channel packets from blocking lower-priority ones
        let mut queues = self.packet_queues.lock().unwrap();
        for channel in 0..=255 {  // HOL Prevention: 0→255 priority order
            if let Some(queue) = queues.get_mut(&channel) {
                if let Some(packet) = queue.pop_front() {
                    godot_print!("✅ HOL BLOCKING PREVENTION: Processing packet from channel {}", channel);
                    return PackedByteArray::from_iter(packet.data.into_iter());
                }
            }
        }
        PackedByteArray::new()
    }

    #[func]
    fn put_packet(&mut self, p_buffer: PackedByteArray) -> Error {
        // Add packet to current channel queue
        let mut queues = self.packet_queues.lock().unwrap();
        queues.entry(self.current_channel)
            .or_insert_with(VecDeque::new)
            .push_back(Packet { data: p_buffer.to_vec() });
        godot_print!("Packet queued on virtual channel {}", self.current_channel);
        Error::OK
    }

    #[func]
    fn put_packet_on_channel(&mut self, p_buffer: PackedByteArray, channel: i32) -> Error {
        // Allow direct channel specification for testing HOL prevention
        let mut queues = self.packet_queues.lock().unwrap();
        queues.entry(channel)
            .or_insert_with(VecDeque::new)
            .push_back(Packet { data: p_buffer.to_vec() });
        godot_print!("Packet queued on channel {} (HOL prevention active)", channel);
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
                    let data = vec![channel as u8, i as u8];
                    let packet = Packet { data };
                    queues.entry(channel).or_insert_with(VecDeque::new).push_back(packet);
                }
            }

            // Add ONE critical packet to channel 0 (should be processed first)
            godot_print!("Adding critical packet to channel 0...");
            let critical_data = vec![0u8, 255u8]; // Channel 0 marker, critical flag
            let critical_packet = Packet { data: critical_data };
            queues.entry(0).or_insert_with(VecDeque::new).push_front(critical_packet);
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
                godot_error!("❌ FAILURE: Channel {} returned instead of channel 0", channel_returned);
                godot_error!("❌ HOL blocking prevention NOT working");
            }
        }

        result
    }

    #[func]
    fn create_client(&mut self, _address: GodotString, _port: i32) -> Error {
        // Note: with Zenoh removed, this is stubbed for demonstration
        godot_print!("Zenoh client creation (HOL blocking prevention ready)");
        self.unique_id = (rand::random::<u32>() % 999) as i64 + 2; // 2-1000 range
        self.connection_status = 1; // CONNECTING
        Error::OK
    }

    #[func]
    fn create_server(&mut self, _port: i32, _max_clients: i32) -> Error {
        // Note: with Zenoh removed, this is stubbed for demonstration
        godot_print!("Zenoh server creation (HOL blocking prevention ready)");
        self.unique_id = 1; // Server gets ID 1
        self.connection_status = 2; // CONNECTED
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
        self.connection_status = 0; // DISCONNECTED
        godot_print!("ZenohMultiplayerPeer connection closed (HOL blocking prevention)");
        // Clear all packet queues
        self.packet_queues.lock().unwrap().clear();
    }
}
