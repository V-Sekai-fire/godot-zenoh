# Copyright (c) 2026-present K. S. Ernest (iFire) Lee
# SPDX-License-Identifier: MIT

extends Node

var zenoh_peer: ZenohMultiplayerPeer
var timer: Timer

var label: Label
var peer_id: String
var connected: bool = false

func _ready():
	setup_ui()
	_connect_to_network()

	timer = Timer.new()
	timer.wait_time = 3.0
	timer.one_shot = false
	timer.connect("timeout", Callable(self, "_send_test_message"))
	add_child(timer)
	timer.start()

func _connect_to_network():
	zenoh_peer = ZenohMultiplayerPeer.new()
	zenoh_peer.game_id = "godot_zenoh_test"

	var port = int(OS.get_environment("ZENOH_PORT")) if OS.get_environment("ZENOH_PORT") else 7447

	var rng = RandomNumberGenerator.new()
	rng.randomize()
	peer_id = "Peer_" + str(rng.randi_range(1000, 9999))

	var server_result = zenoh_peer.create_server(port, 32)
	if server_result == 0:
		label.text = "OK Started as SERVER on port " + str(port) + "\nPeer: " + peer_id
		connected = true
	else:
		label.text = "ERROR Server creation failed on port " + str(port)

func _process(delta):
	if zenoh_peer:
		zenoh_peer.poll()
		var packet_count = zenoh_peer.get_available_packet_count()
		while packet_count > 0:
			var packet = zenoh_peer.get_packet()
			var msg = packet.get_string_from_utf8()
			# Silently acknowledge received messages (remove verbose logging)
			packet_count -= 1

func _send_test_message():
	if connected and zenoh_peer.get_connection_status() == 2:
		var message = peer_id + ":" + str(Time.get_ticks_msec())
		var data = PackedByteArray()
		data.append_array(message.to_utf8_buffer())

		zenoh_peer.set_transfer_channel(1)
		zenoh_peer.put_packet(data)
		# Remove verbose sent message logging

func _notification(what):
	if what == NOTIFICATION_EXIT_TREE:
		if timer: timer.stop()
		if zenoh_peer: zenoh_peer.close()

func setup_ui():
	var window = Window.new()
	add_child(window)
	window.position = Vector2(400, 300)
	window.size = Vector2(320, 120)
	window.title = "Zenoh Demo"
	
	var margin = MarginContainer.new()
	window.add_child(margin)
	margin.position = Vector2(10, 10)
	margin.size = Vector2(300, 100)
	
	label = Label.new()
	label.text = "Connecting..."
	label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	label.size = Vector2(280, 80)
	margin.add_child(label)
