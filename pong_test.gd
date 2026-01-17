# Ping pong countdown test between two Godot instances
extends Node

var zenoh_peer: ZenohMultiplayerPeer

var my_id: int = -1
var is_host: bool = false

var countdown_number: int = 10
var last_received_count: int = -1
var is_counting_down: bool = false
var transfer_mode: int = 0  # 0 = Server relay mode, 1 = Direct pub/sub

var button: Button
var label: Label
var host_button: Button
var join_button: Button

var countdown_timer: Timer

func _ready():
	print("Pong Test Starting...")

	# Create UI
	setup_ui()

	# Initialize zenoh peer
	zenoh_peer = ZenohMultiplayerPeer.new()
	zenoh_peer.game_id = "pong_test"

func setup_ui():
	# Create UI for testing
	var vbox = VBoxContainer.new()
	add_child(vbox)
	vbox.set_anchors_preset(Control.PRESET_FULL_RECT)

	# Title
	var title = Label.new()
	title.text = "Godot-Zenoh Ping Pong Test"
	vbox.add_child(title)

	# Connection buttons
	host_button = Button.new()
	host_button.text = "Host Game (Server)"
	host_button.connect("pressed", Callable(self, "_on_host_pressed"))
	vbox.add_child(host_button)

	join_button = Button.new()
	join_button.text = "Join Game (Client)"
	join_button.connect("pressed", Callable(self, "_on_join_pressed"))
	vbox.add_child(join_button)

	# Status label
	label = Label.new()
	label.text = "Choose Host or Join to start..."
	vbox.add_child(label)

	# Send button
	button = Button.new()
	button.text = "Send Count to Other Player"
	button.disabled = true
	button.connect("pressed", Callable(self, "_on_send_pressed"))
	vbox.add_child(button)

func _on_host_pressed():
	print("Starting as host...")
	is_host = true

	# Start server
	var result = zenoh_peer.create_server(7447, 32)
	if result == 0:
		my_id = 1
		var client_id = zenoh_peer.get_unique_id()
		label.text = "Hosting game - Player ID: " + str(client_id)
		print("Server Player ID: " + str(client_id))
		setup_networking()
	else:
		label.text = "Failed to host: " + str(result)

func _on_join_pressed():
	print("Joining as client...")
	is_host = false

	# Join server
	var result = zenoh_peer.create_client("localhost", 7447)
	if result == 0:
		my_id = 2
		var client_id = zenoh_peer.get_unique_id()
		label.text = "Joined game - Player ID: " + str(client_id)
		print("Client Player ID: " + str(client_id))
		print("DEBUG: Verifying client ID assignment...")
		setup_networking()
	else:
		label.text = "Failed to join: " + str(result)
		print("DEBUG: Client connection failed with result: " + str(result))

func setup_networking():
	print("Networking setup complete")

	# Start ping pong countdown after a brief delay
	var start_timer = Timer.new()
	start_timer.one_shot = true
	start_timer.wait_time = 2.0  # Wait 2 seconds after connecting
	start_timer.connect("timeout", Callable(self, "_on_ping_pong_start"))
	add_child(start_timer)
	start_timer.start()

	# Set up polling timer
	var timer = Timer.new()
	timer.autostart = true
	timer.wait_time = 0.1  # Poll every 100ms
	timer.connect("timeout", Callable(self, "_on_poll_timeout"))
	add_child(timer)

	# Set up countdown timer (2 second intervals)
	countdown_timer = Timer.new()
	countdown_timer.autostart = false
	countdown_timer.one_shot = true
	countdown_timer.wait_time = 2.0
	countdown_timer.connect("timeout", Callable(self, "_on_countdown_tick"))
	add_child(countdown_timer)

func _on_ping_pong_start():
	if is_host:
		# Host starts the countdown
		print("Starting ping pong countdown as host")
		label.text = "Starting countdown..."
		is_counting_down = true
		countdown_number = 10
		_send_count()
		countdown_timer.start()
	else:
		# Client waits to receive first message
		label.text = "Waiting for host to start..."

func _on_send_pressed():
	# Send countdown number
	var message = "COUNT:" + str(countdown_number)
	var data = PackedByteArray()
	data.append_array(message.to_utf8_buffer())

	zenoh_peer.put_packet(data)
	print("Sent: " + message)

	# Decrement for next send
	if countdown_number > 0:
		countdown_number -= 1
		button.text = "Send " + str(countdown_number) + " to Other Player"

func _send_count():
	# Send current countdown number with sender ID
	var message = "COUNT:" + str(countdown_number) + ":" + str(zenoh_peer.get_unique_id())
	var data = PackedByteArray()
	data.append_array(message.to_utf8_buffer())

	# In Zenoh pub/sub: EVERY message published is automatically "relayed" to ALL subscribers
	# This provides the exact same functionality as server relay - no additional code needed!
	zenoh_peer.put_packet(data)
	print("Player " + str(zenoh_peer.get_unique_id()) + " published " + message + " (Zenoh auto-relays to all subscribers)")

	label.text = "Sent: " + str(countdown_number) + " (waiting for ack)"

func _on_countdown_tick():
	# Automatic countdown disabled - only send when ack received
	pass

func _on_poll_timeout():
	# Poll for network messages
	zenoh_peer.poll()

	# Check for received packets
	while zenoh_peer.get_available_packet_count() > 0:
		var data = zenoh_peer.get_packet()
		var data_string = data.get_string_from_utf8()
		print("DEBUG: Received packet with length " + str(data.size()) + " bytes")
		print("DEBUG: Packet content: '" + data_string + "'")

		# Handle countdown message from other player (format: "COUNT:N:FROM_ID")
		if data_string.begins_with("COUNT:"):
			var parts = data_string.split(":")
			var count = -1
			var from_player_id = -1

			if parts.size() >= 3:
				count = int(parts[1])
				from_player_id = int(parts[2])
				print("Player " + str(zenoh_peer.get_unique_id()) + " received COUNT:" + str(count) + " from Player " + str(from_player_id))
			else:
				# Fallback for old format
				var count_str = data_string.substr(6)
				count = int(count_str)
				from_player_id = get_other_player_id()
				print("Player " + str(zenoh_peer.get_unique_id()) + " received COUNT:" + str(count) + " from Player " + str(from_player_id) + " (legacy format)")

			# Filter out self-messages (don't process your own packets)
			my_id = zenoh_peer.get_unique_id()
			if from_player_id == my_id:
				print("Ignored self-message")
				continue  # Skip processing self-messages

			# Acknowledge receipt by decrementing and sending next number (after 1 second delay)
			if countdown_number > 0 and count >= 0:
				label.text = "Received: " + str(count) + " - Preparing response..."
				print("Player " + str(zenoh_peer.get_unique_id()) + " acknowledging receipt - will respond in 1 second with countdown: " + str(countdown_number))

				# Wait 1 second before responding (doesn't block the polling)
				var response_timer = Timer.new()
				response_timer.one_shot = true
				response_timer.wait_time = 1.0
				response_timer.connect("timeout", Callable(self, "_delayed_response"))
				add_child(response_timer)
				response_timer.start()
			else:
				label.text = "Game already finished"

func _delayed_response():
	# This runs after 1 second delay
	if countdown_number > 0:
		countdown_number -= 1
		if countdown_number == 0:
			label.text = "GAME OVER!"
			print("Countdown complete!")
		else:
			label.text = "Responding with: " + str(countdown_number)
			print("After 1 second - sending countdown: " + str(countdown_number))
			_send_count()
			# Wait for next receipt (no auto countdown)

func get_other_player_id():
	return 2 if my_id == 1 else 1

func _notification(what):
	if what == NOTIFICATION_EXIT_TREE:
		if zenoh_peer:
			print("Cleaning up zenoh peer...")
			zenoh_peer.close()
