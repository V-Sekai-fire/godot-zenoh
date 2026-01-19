# Copyright (c) 2026-present K. S. Ernest (iFire) Lee
# SPDX-License-Identifier: MIT

extends Node

var server_peer: ZenohMultiplayerPeer
var client_peers: Array[ZenohMultiplayerPeer] = []
var test_timer: Timer
var test_phase: int = 0
var start_time: int = 0
const NUM_CLIENTS = 10

func _ready():
    test_timer = Timer.new()
    test_timer.wait_time = 0.1
    test_timer.timeout.connect(_on_test_timer)
    add_child(test_timer)
    
    start_time = Time.get_ticks_msec()
    print("Starting Zenoh networking stack test with %d clients..." % NUM_CLIENTS)
    
    run_networking_test()

func run_networking_test():
    # Phase 1: Create server
    server_peer = ZenohMultiplayerPeer.new()
    server_peer.game_id = "test_game"
    var result = server_peer.create_server(7447, 32)
    assert(result == OK, "Server creation failed")
    assert(server_peer.connection_status() == MultiplayerPeer.CONNECTION_CONNECTING, "Server should be connecting")
    
    test_timer.start()

func _on_test_timer():
    var elapsed = Time.get_ticks_msec() - start_time
    if elapsed > 60000: # 60 second timeout for multiple clients
        assert(false, "Test timed out after 60 seconds")
        return
    
    match test_phase:
        0: # Wait for server connection
            server_peer.poll()
            if server_peer.connection_status() == MultiplayerPeer.CONNECTION_CONNECTED:
                assert(server_peer.is_server(), "Server should be server")
                assert(server_peer.get_unique_id() == 1, "Server ID should be 1")
                print("✓ Server connected (phase 0 -> 1)")
                
                # Phase 2: Create all clients
                for i in range(NUM_CLIENTS):
                    var client = ZenohMultiplayerPeer.new()
                    client.game_id = "test_game"
                    var result = client.create_client("localhost", 7447)
                    assert(result == OK, "Client %d creation failed" % i)
                    assert(client.connection_status() == MultiplayerPeer.CONNECTION_CONNECTING, "Client %d should be connecting" % i)
                    client_peers.append(client)
                
                print("✓ Created %d clients (phase 1 -> 2)" % NUM_CLIENTS)
                test_phase = 1
        
        1: # Wait for all clients to connect
            var all_connected = true
            for i in range(client_peers.size()):
                client_peers[i].poll()
                if client_peers[i].connection_status() != MultiplayerPeer.CONNECTION_CONNECTED:
                    all_connected = false
                    break
            
            if all_connected:
                # Verify all clients
                for i in range(client_peers.size()):
                    assert(!client_peers[i].is_server(), "Client %d should not be server" % i)
                    assert(client_peers[i].get_unique_id() != 1, "Client %d ID should not be 1" % i)
                
                print("✓ All %d clients connected (phase 2 -> 3)" % NUM_CLIENTS)
                
                # FIXME: TEST ONLY VERIFIES SENDING, NOT RECEIVING!
                # TODO: This test verifies put_packet_on_channel works but NEVER checks get_packet()
                # TODO: CRITICAL MISSING: No test for actual multi-peer message exchange
                # TODO: Linearizability test validates message reception, but this basic test ignores it
                # FIXME: Network test incomplete - should verify multi-peer communication loop

                # Phase 3: Test packet sending from each client
                var test_data = PackedByteArray([1, 2, 3, 4, 5])
                for i in range(client_peers.size()):
                    var send_result = client_peers[i].put_packet_on_channel(test_data, 0)
                    assert(send_result == OK, "Client %d packet send failed" % i)

                print("✓ All clients sent packets successfully (phase 3 -> 4)")
                print("⚠️ WARNING: Test verified sending only - message reception not tested!")
                
                # Phase 4: Disconnect all
                server_peer.disconnect()
                for client in client_peers:
                    client.disconnect()
                
                test_phase = 2
        
        2: # Wait for all disconnections
            server_peer.poll()
            var all_disconnected = server_peer.connection_status() == MultiplayerPeer.CONNECTION_DISCONNECTED
            
            for client in client_peers:
                client.poll()
                if client.connection_status() != MultiplayerPeer.CONNECTION_DISCONNECTED:
                    all_disconnected = false
                    break
            
            if all_disconnected:
                print("✓ Server and all %d clients disconnected (phase 4 -> complete)" % NUM_CLIENTS)
                print("🎉 Multi-client networking stack test PASSED!")
                test_timer.stop()
                get_tree().quit()

func _process(_delta):
    # Poll server and all clients continuously for state updates
    if server_peer:
        server_peer.poll()
    for client in client_peers:
        if client:
            client.poll()