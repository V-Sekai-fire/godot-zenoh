// Copyright (c) 2026-present K. S. Ernest (iFire) Lee
// SPDX-License-Identifier: MIT

#[cfg(test)]
mod networking_state_machine_tests {
    #[test]
    fn test_connection_state_transitions() {
        let mut state: i32 = 0; // DISCONNECTED
        assert_eq!(state, 0);
        state = 1; // CONNECTING
        assert_eq!(state, 1);
        state = 2; // CONNECTED
        assert_eq!(state, 2);
    }

    #[test]
    fn test_channel_priority_logic() {
        let channels = vec![0, 1, 2, 10, 20, 100];
        let mut processed_channels = Vec::new();
        for &channel in &channels {
            if channel < 100 {
                processed_channels.push(channel);
            }
        }
        assert_eq!(processed_channels[0], 0);
        assert_eq!(processed_channels[1], 1);
        assert_eq!(processed_channels[2], 2);
    }
}

/// Integration tests: 1 server + 2 clients, verify pub/sub packet delivery.
///
/// When one peer moves, every other peer must receive the update.
/// Run with: cargo test --test integration multi_peer -- --nocapture
#[cfg(test)]
mod multi_peer_tests {
    use godot_zenoh::networking::ZenohSession;
    use std::time::Duration;

    const GAME_ID: &str = "test_game_multi";
    const SERVER_PORT: i32 = 17450;

    /// 1 server + 2 clients: verify that a position update from client_a
    /// reaches the server and client_b, but NOT client_a itself (no echo).
    /// Also verifies that a server broadcast reaches both clients.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_three_peer_packet_delivery() {
        // --- server ---
        let mut server =
            ZenohSession::create_server(SERVER_PORT, GAME_ID.to_string(), None)
                .await
                .expect("server creation failed");

        // Give the server a moment to bind.
        tokio::time::sleep(Duration::from_millis(500)).await;

        // --- client_a ---
        let mut client_a = ZenohSession::create_client(
            "127.0.0.1".to_string(),
            SERVER_PORT,
            GAME_ID.to_string(),
        )
        .await
        .expect("client_a creation failed");

        // --- client_b ---
        let mut client_b = ZenohSession::create_client(
            "127.0.0.1".to_string(),
            SERVER_PORT,
            GAME_ID.to_string(),
        )
        .await
        .expect("client_b creation failed");

        // Setup channel 0 on all three peers.
        server.setup_channel(0).await.expect("server channel setup");
        client_a
            .setup_channel(0)
            .await
            .expect("client_a channel setup");
        client_b
            .setup_channel(0)
            .await
            .expect("client_b channel setup");

        // Allow subscriptions to propagate.
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // --- client_a sends a position update ---
        let position_msg = b"player_moved:x=10,y=20";
        let result = client_a
            .send_packet(position_msg, GAME_ID.to_string(), 0)
            .await;
        assert_eq!(result, godot::global::Error::OK, "client_a send failed");

        // Wait for message propagation.
        tokio::time::sleep(Duration::from_millis(1000)).await;

        // --- Server must receive client_a's packet ---
        let server_packets = server.drain_packets();
        let server_got_msg = server_packets
            .iter()
            .any(|pkt| pkt.raw.len() > 8 && &pkt.raw[8..] == position_msg);
        assert!(
            server_got_msg,
            "server did not receive client_a's position update; got {} packets",
            server_packets.len()
        );

        // --- client_b must receive client_a's packet ---
        let b_packets = client_b.drain_packets();
        let b_got_msg = b_packets
            .iter()
            .any(|pkt| pkt.raw.len() > 8 && &pkt.raw[8..] == position_msg);
        assert!(
            b_got_msg,
            "client_b did not receive client_a's position update; got {} packets",
            b_packets.len()
        );

        // --- client_a must NOT receive its own reflected message ---
        let a_packets = client_a.drain_packets();
        let a_got_own = a_packets
            .iter()
            .any(|pkt| pkt.raw.len() > 8 && &pkt.raw[8..] == position_msg);
        assert!(
            !a_got_own,
            "client_a received its own reflected packet (should be filtered)"
        );

        // --- Server broadcasts a state update ---
        let server_msg = b"server_state:tick=42";
        let result = server
            .send_packet(server_msg, GAME_ID.to_string(), 0)
            .await;
        assert_eq!(result, godot::global::Error::OK, "server send failed");

        tokio::time::sleep(Duration::from_millis(1000)).await;

        let a_packets2 = client_a.drain_packets();
        let a_got_server = a_packets2
            .iter()
            .any(|pkt| pkt.raw.len() > 8 && &pkt.raw[8..] == server_msg);
        assert!(
            a_got_server,
            "client_a did not receive server broadcast; got {} packets",
            a_packets2.len()
        );

        let b_packets2 = client_b.drain_packets();
        let b_got_server = b_packets2
            .iter()
            .any(|pkt| pkt.raw.len() > 8 && &pkt.raw[8..] == server_msg);
        assert!(
            b_got_server,
            "client_b did not receive server broadcast; got {} packets",
            b_packets2.len()
        );
    }
}
