// Copyright (c) 2026-present K. S. Ernest (iFire) Lee
// SPDX-License-Identifier: MIT

use godot::global::Error;
use godot::prelude::*;
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use zenoh::pubsub::Publisher;
use zenoh::pubsub::Subscriber;
use zenoh::time::Timestamp;

/// A received packet: raw bytes (with 8-byte peer_id header) and channel number.
pub struct ReceivedPacket {
    pub raw: Vec<u8>,
    pub channel: i32,
}

/// Zenoh networking session with channel-based topics - ASYNC IMPLEMENTATION
pub struct ZenohSession {
    session: Arc<zenoh::Session>,
    publishers: Arc<Mutex<HashMap<String, Arc<Publisher<'static>>>>>,
    /// Keep subscribers alive (dropping them would unsubscribe).
    _subscribers: Vec<Subscriber<()>>,
    receive_tx: mpsc::SyncSender<ReceivedPacket>,
    receive_rx: mpsc::Receiver<ReceivedPacket>,
    game_id: String,
    peer_id: i64,
}

impl ZenohSession {
    /// Create Zenoh networking client session (connects to server peer)
    pub async fn create_client(
        address: String,
        port: i32,
        game_id: String,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let connect_endpoint = format!("tcp/{}:{}", address, port);
        std::env::set_var("ZENOH_CONNECT", connect_endpoint);
        std::env::set_var("ZENOH_OPEN_TIMEOUT", "30000");
        std::env::set_var("ZENOH_CLOSE_TIMEOUT", "30000");
        std::env::set_var("ZENOH_KEEP_ALIVE", "10000");

        let session = Arc::new(
            zenoh::open(zenoh::Config::default())
                .await
                .map_err(|e| format!("Client session creation failed: {:?}", e))?,
        );

        let zid = session.zid().to_string();
        let peer_id = Self::peer_id_from_zid(&zid);

        let (tx, rx) = mpsc::sync_channel(4096);
        Ok(ZenohSession {
            session,
            publishers: Arc::new(Mutex::new(HashMap::new())),
            _subscribers: Vec::new(),
            receive_tx: tx,
            receive_rx: rx,
            game_id,
            peer_id,
        })
    }

    /// Create Zenoh networking server session (listens for client connections)
    pub async fn create_server(
        port: i32,
        game_id: String,
        connect_addr: Option<String>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let listen_endpoint = format!("tcp/127.0.0.1:{}", port);
        std::env::set_var("ZENOH_LISTEN", listen_endpoint);
        if let Some(addr) = connect_addr {
            std::env::set_var("ZENOH_CONNECT", addr);
        }
        std::env::set_var("ZENOH_OPEN_TIMEOUT", "30000");
        std::env::set_var("ZENOH_CLOSE_TIMEOUT", "30000");
        std::env::set_var("ZENOH_KEEP_ALIVE", "10000");

        let session = Arc::new(
            zenoh::open(zenoh::Config::default())
                .await
                .map_err(|e| format!("Server session creation failed: {:?}", e))?,
        );

        let (tx, rx) = mpsc::sync_channel(4096);
        Ok(ZenohSession {
            session,
            publishers: Arc::new(Mutex::new(HashMap::new())),
            _subscribers: Vec::new(),
            receive_tx: tx,
            receive_rx: rx,
            game_id,
            peer_id: 1,
        })
    }

    /// Send packet on specific topic-based channel (async).
    /// Prepends 8-byte little-endian peer_id header.
    pub async fn send_packet(&self, p_buffer: &[u8], game_id: String, channel: i32) -> Error {
        let topic = format!("godot/game/{}/channel{:03}", game_id, channel);
        let publisher = {
            let publishers = self.publishers.lock().unwrap();
            publishers.get(&topic).cloned()
        };

        if let Some(publisher) = publisher {
            let mut packet_data = Vec::with_capacity(8 + p_buffer.len());
            packet_data.extend_from_slice(&self.peer_id.to_le_bytes());
            packet_data.extend_from_slice(p_buffer);
            if let Err(e) = publisher.put(packet_data).await {
                eprintln!(
                    "Failed to send Zenoh packet on channel {}: {:?}",
                    channel, e
                );
                return Error::FAILED;
            }
        } else {
            godot_error!("No publisher available for channel {}", channel);
            return Error::FAILED;
        }
        Error::OK
    }

    /// Setup publisher and subscriber for a topic-based channel.
    pub async fn setup_channel(
        &mut self,
        channel: i32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let topic: &'static str = Box::leak(
            format!("godot/game/{}/channel{:03}", self.game_id, channel).into_boxed_str(),
        );

        if !self.publishers.lock().unwrap().contains_key(topic) {
            let publisher = self.session.declare_publisher(topic).await?;
            self.publishers
                .lock()
                .unwrap()
                .insert(topic.to_string(), Arc::new(publisher));
        }

        // Subscriber: push received bytes into the mpsc channel.
        let tx = self.receive_tx.clone();
        let subscriber = self
            .session
            .declare_subscriber(topic)
            .callback(move |sample| {
                let raw: Vec<u8> = sample.payload().to_bytes().into_owned();
                let _ = tx.try_send(ReceivedPacket { raw, channel });
            })
            .await?;
        self._subscribers.push(subscriber);

        Ok(())
    }

    /// Drain all buffered received packets, filtering out our own reflected messages.
    pub fn drain_packets(&mut self) -> Vec<ReceivedPacket> {
        let my_peer_id = self.peer_id;
        let mut out = Vec::new();
        while let Ok(pkt) = self.receive_rx.try_recv() {
            // Skip packets we sent ourselves (Zenoh delivers to local subscribers too).
            if pkt.raw.len() >= 8 {
                let sender_bytes: [u8; 8] = pkt.raw[..8].try_into().unwrap();
                if i64::from_le_bytes(sender_bytes) == my_peer_id {
                    continue;
                }
            }
            out.push(pkt);
        }
        out
    }

    /// Send a zero-payload discovery beacon so remote peers learn our peer_id.
    ///
    /// The beacon travels on the well-known discovery topic for this game_id.
    /// `drain_packets()` returns these with an empty `raw[8..]` payload so that
    /// `ZenohActor` can emit `peer_connected` without forwarding them to Godot's
    /// packet queue.
    pub async fn send_announce(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let topic = Self::discovery_topic(&self.game_id);
        let publisher = {
            let publishers = self.publishers.lock().unwrap();
            publishers.get(topic).cloned()
        };
        if let Some(pub_) = publisher {
            let mut data = Vec::with_capacity(8);
            data.extend_from_slice(&self.peer_id.to_le_bytes());
            pub_.put(data).await?;
        }
        Ok(())
    }

    /// Setup publisher + subscriber for the discovery beacon topic.
    pub async fn setup_discovery(
        &mut self,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let topic: &'static str = Box::leak(
            Self::discovery_topic(&self.game_id)
                .to_string()
                .into_boxed_str(),
        );
        if !self.publishers.lock().unwrap().contains_key(topic) {
            let publisher = self.session.declare_publisher(topic).await?;
            self.publishers
                .lock()
                .unwrap()
                .insert(topic.to_string(), Arc::new(publisher));
        }
        let tx = self.receive_tx.clone();
        let subscriber = self
            .session
            .declare_subscriber(topic)
            .callback(move |sample| {
                let raw: Vec<u8> = sample.payload().to_bytes().into_owned();
                // Discovery beacons use channel i32::MIN as a sentinel.
                let _ = tx.try_send(ReceivedPacket {
                    raw,
                    channel: i32::MIN,
                });
            })
            .await?;
        self._subscribers.push(subscriber);
        Ok(())
    }

    fn discovery_topic(game_id: &str) -> &str {
        Box::leak(format!("godot/game/{}/discovery", game_id).into_boxed_str())
    }

    /// Derive a valid Godot peer ID (positive i32, ≥ 2) from a Zenoh ZID string.
    pub fn peer_id_from_zid(zid: &str) -> i64 {
        let hex = if zid.len() >= 8 {
            &zid[zid.len() - 8..]
        } else {
            zid
        };
        let raw = u32::from_str_radix(hex, 16).unwrap_or(2);
        // Clamp to [2, i32::MAX] so the ID is a valid positive Godot peer ID.
        (raw % (i32::MAX as u32 - 1) + 2) as i64
    }

    pub fn get_peer_id(&self) -> i64 {
        self.peer_id
    }

    pub fn get_zid(&self) -> String {
        self.session.zid().to_string()
    }

    pub fn get_timestamp(&self) -> Timestamp {
        self.session.new_timestamp()
    }
}
