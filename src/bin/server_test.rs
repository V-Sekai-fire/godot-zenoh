// Copyright (c) 2026-present K. S. Ernest (iFire) Lee
// SPDX-License-Identifier: MIT

use godot_zenoh::networking::ZenohSession;
use std::env;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        eprintln!("Usage: {} <listen_port> <connect_addr> <game_id>", args[0]);
        std::process::exit(1);
    }

    let listen_port: i32 = match args[1].parse() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("Invalid port");
            std::process::exit(1);
        }
    };
    let connect_addr = if args[2] == "none" {
        None
    } else {
        Some(args[2].clone())
    };
    let game_id = args[3].clone();

    println!(
        "Starting Zenoh server test on port {} connecting to {:?}...",
        listen_port, connect_addr
    );

    let game_id_g = game_id.clone();

    // Create server session
    let session = match ZenohSession::create_server(listen_port, game_id_g, connect_addr).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to create server: {:?}", e);
            std::process::exit(1);
        }
    };
    println!("Server created with peer ID: {}", session.get_peer_id());

    // Setup channel 0
    if let Err(e) = session.setup_channel(0).await {
        eprintln!("Failed to setup channel: {:?}", e);
        std::process::exit(1);
    }
    println!("Channel 0 setup complete");

    // Send a test message
    let test_data = format!("Hello from Rust server on port {}!", listen_port).into_bytes();
    let err = session.send_packet(&test_data, game_id, 0).await;
    if err != godot::global::Error::OK {
        eprintln!("Failed to send packet: {:?}", err);
    } else {
        println!("Test message sent");
    }

    // Wait a bit
    sleep(Duration::from_secs(10)).await;

    println!("Server test completed");
}
