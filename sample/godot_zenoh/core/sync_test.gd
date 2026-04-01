# Copyright (c) 2026-present K. S. Ernest (iFire) Lee
# SPDX-License-Identifier: MIT
#
# Tests Godot's built-in MultiplayerSynchronizer and @rpc via ZenohMultiplayerPeer.
#
# Usage:
#   ZENOH_ROLE=server  ZENOH_PORT=7449  godot --headless --path sample/
#   ZENOH_ROLE=client  ZENOH_PORT=7449  godot --headless --path sample/

extends Node

## Replicated by MultiplayerSynchronizer.  @export required for property discovery.
@export var sync_position: Vector2 = Vector2.ZERO

var zenoh_peer: ZenohMultiplayerPeer
var role: String = ""
var connected: bool = false
var rpc_received: bool = false
var start_msec: int = 0

# Timeouts (wall-clock ms)
const SERVER_EXIT_AFTER_MS  := 8000   # server quits after 8 s
const CLIENT_TIMEOUT_MS     := 10000  # client fails if not done in 10 s

func _ready() -> void:
	role = OS.get_environment("ZENOH_ROLE")
	var port: int = int(OS.get_environment("ZENOH_PORT")) \
		if OS.get_environment("ZENOH_PORT") != "" else 7449

	if role == "":
		print("SKIP: ZENOH_ROLE not set.")
		get_tree().quit(0)
		return

	start_msec = Time.get_ticks_msec()

	zenoh_peer = ZenohMultiplayerPeer.new()
	zenoh_peer.game_id = "sync_test_%d" % port

	if role == "server":
		zenoh_peer.create_server(port, 10)
		multiplayer.multiplayer_peer = zenoh_peer
		multiplayer.peer_connected.connect(_on_peer_connected)
		connected = true
		print("SERVER: peer_id=%d" % multiplayer.get_unique_id())
	else:
		zenoh_peer.create_client("127.0.0.1", port)
		multiplayer.multiplayer_peer = zenoh_peer
		multiplayer.connected_to_server.connect(_on_connected)
		print("CLIENT: connecting to port %d" % port)

	# --- MultiplayerSynchronizer -----------------------------------------
	var sync_config := SceneReplicationConfig.new()
	sync_config.add_property(NodePath(".:sync_position"))

	var synchronizer := MultiplayerSynchronizer.new()
	synchronizer.root_path = NodePath("..")   # parent = this node
	synchronizer.replication_config = sync_config
	add_child(synchronizer)

func _on_connected() -> void:
	connected = true
	print("CLIENT: connected, peer_id=%d" % multiplayer.get_unique_id())

func _on_peer_connected(id: int) -> void:
	print("SERVER: peer %d connected, sending rpc_id + sync" % id)
	# Target each client individually so late joiners get the message.
	rpc_id(id, "rpc_ping", 42)
	sync_position = Vector2(3.0, 7.0)

func _process(_delta: float) -> void:
	zenoh_peer.poll()

	var elapsed := Time.get_ticks_msec() - start_msec

	if role == "server":
		if elapsed >= SERVER_EXIT_AFTER_MS:
			print("SERVER: PASS done (elapsed %d ms)" % elapsed)
			get_tree().quit(0)

	else: # client
		if elapsed >= CLIENT_TIMEOUT_MS:
			_fail("TIMEOUT after %d ms (rpc=%s sync=%s)" \
				% [elapsed, rpc_received, sync_position])
			return

		if not connected:
			return

		var sync_ok := sync_position != Vector2.ZERO
		if rpc_received and sync_ok:
			print("CLIENT: PASS rpc=true sync_position=%s (elapsed %d ms)" \
				% [sync_position, elapsed])
			get_tree().quit(0)

# ---- RPC ----------------------------------------------------------------

@rpc("authority", "call_remote")
func rpc_ping(value: int) -> void:
	print("CLIENT: rpc_ping value=%d" % value)
	if value == 42:
		rpc_received = true

# ---- helpers ------------------------------------------------------------

func _fail(reason: String) -> void:
	print("%s: FAIL %s" % [role.to_upper(), reason])
	get_tree().quit(1)
