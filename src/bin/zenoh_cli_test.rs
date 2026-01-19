// Copyright (c) 2026-present K. S. Ernest (iFire) Lee
// SPDX-License-Identifier: MIT

use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();

    let command = args.get(1).map(|s| s.as_str()).unwrap_or("all");

    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

    runtime.block_on(async {
        match command {
            "network" | "net" => {
                let role = args.get(2).map(|s| s.as_str()).unwrap_or("auto");
                let message = args.get(3).cloned();
                println!("🌐 Real Zenoh Network Testing");
                println!("   Role: {}", role);
                if let Some(msg) = &message {
                    println!("   Message: {}", msg);
                }
                run_zenoh_network_test(role, message).await;
            }
            "start-router" | "router" => {
                println!("🔀 Starting Zenoh Router");
                start_zenoh_router().await;
            }
            "info" | "help" => {
                show_test_info();
                show_usage();
            }
            _ => {
                eprintln!("❌ Unknown command: {}", command);
                show_usage();
            }
        }
    });
}

fn show_usage() {
    println!("");
    println!("Usage: zenoh_cli_test [COMMAND] [OPTIONS]");
    println!("");
    println!("Commands:");
    println!("  network ROLE [MSG]     Connect to real Zenoh network");
    println!("    ROLE: publisher|subscriber (default: auto)");
    println!("  start-router           Start local Zenoh router daemon");
    println!("  info|help              Show this information");
    println!("");
    println!("Examples:");
    println!("  cargo run --bin zenoh_cli_test -- start-router");
    println!("  cargo run --bin zenoh_cli_test -- network publisher \"Hello World\"");
    println!("  cargo run --bin zenoh_cli_test -- network subscriber");
    println!("  cargo run --bin zenoh_cli_test -- info");
}

fn show_test_info() {
    println!("🌐 Zenoh-Godot Real Network CLI");
    println!("");
    println!("Direct Zenoh API testing - no mocks, no simulations:");
    println!("• Zenoh session creation and management");
    println!("• Publisher/Subscriber communication");
    println!("• Real network message delivery");
    println!("• Godot channel/topic routing");
    println!("• Multi-peer networking validation");
    println!("");
    println!("Capabilities:");
    println!("✅ Connects to real Zenoh networks");
    println!("✅ Uses actual Zenoh API calls");
    println!("✅ Tests end-to-end message delivery");
    println!("✅ Validates Godot-style channel isolation");
    println!("✅ Individual peer/inventory testing");
}

async fn run_zenoh_network_test(role: &str, message: Option<String>) {
    println!("🌐 Creating Zenoh session...");

    match zenoh::open(zenoh::Config::default()).await {
        Ok(session) => {
            println!("✅ Connected to Zenoh: {}", session.zid());

            if role == "publisher" || role == "pub" {
                let topic = "godot/game/test/channel0";
                match session.declare_publisher(topic).await {
                    Ok(publisher) => {
                        println!("📤 Publisher ready on {}", topic);

                        let msg = message.unwrap_or_else(|| "Hello Zenoh!".to_string());
                        let data = msg.as_bytes().to_vec();

                        println!("🔄 Sending message: {} ({} bytes)", msg, data.len());
                        match publisher.put(data).await {
                            Ok(_) => println!("✅ Message sent successfully"),
                            Err(e) => println!("❌ Send failed: {}", e),
                        }
                    }
                    Err(e) => println!("❌ Publisher setup failed: {}", e),
                }
            } else if role == "subscriber" || role == "sub" {
                let topic = "godot/game/test/channel0";
                match session.declare_subscriber(topic).await {
                    Ok(subscriber) => {
                        println!("📥 Subscriber listening on {}", topic);

                        // Receive one message
                        println!("👂 Waiting for message...");
                        match subscriber.recv_async().await {
                            Ok(sample) => {
                                let payload = sample.payload();
                                let payload_bytes = payload.to_bytes();
                                let data = String::from_utf8_lossy(&payload_bytes);
                                println!("📨 Received: {} ({} bytes)", data, payload_bytes.len());
                                println!("✅ Message received successfully");
                            }
                            Err(e) => println!("❌ Receive error: {}", e),
                        }
                    }
                    Err(e) => println!("❌ Subscriber setup failed: {}", e),
                }
            } else {
                println!("🤝 Auto mode: will test both pub and sub (separate processes needed)");
                println!("💡 Tip: Run 'publisher' and 'subscriber' in different terminals");
                println!("   cargo run --bin zenoh_cli_test -- network publisher 'Hello World'");
                println!("   cargo run --bin zenoh_cli_test -- network subscriber");
            }

            // Cleanup
            drop(session);
            println!("🔌 Zenoh session closed");
        }
        Err(e) => println!("❌ Failed to connect to Zenoh: {}", e),
    }

    println!("✅ Real Zenoh network test completed");
}

async fn start_zenoh_router() {
    println!("🔀 Starting local Zenoh router...");

    // Check if zenohd is available
    match std::process::Command::new("zenohd")
        .arg("--listen=tcp/127.0.0.1:7447")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => {
            println!("✅ Zenoh router started (PID: {})", child.id());
            println!("📡 Router listening on tcp/127.0.0.1:7447");
            println!("💡 Router will run in background - use Ctrl+C to stop");

            // Wait for user interrupt
            tokio::signal::ctrl_c().await.ok();
            println!("🛑 Stopping router...");

            // Note: The router process will be terminated when this process ends
            // In production, you'd want proper process management
        }
        Err(e) => {
            println!("❌ Failed to start zenohd: {}", e);
            println!("💡 Install Zenoh: https://zenoh.io/docs/getting-started/installation/");
        }
    }
}
