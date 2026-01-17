# Ping pong countdown test between two Godot instances
extends Node

var zenoh_peer: ZenohMultiplayerPeer

var my_id: int = -1
var is_host: bool = false

var countdown_number: int = 10
var last_received_count: int = -1
var is_counting_down: bool = false

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
		label.text = "Hosting game - Player 1 (Server)"
		my_id = 1
		setup_networking()
	else:
		label.text = "Failed to host: " + str(result)

func _on_join_pressed():
	print("Joining as client...")
	is_host = false

	# Join server
	var result = zenoh_peer.create_client("localhost", 7447)
	if result == 0:
		label.text = "Joining game - Player 2 (Client)"
		my_id = 2
		setup_networking()
	else:
		label.text = "Failed to join: " + str(result)

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
	# Send current countdown number
	var message = "COUNT:" + str(countdown_number)
	var data = PackedByteArray()
	data.append_array(message.to_utf8_buffer())

	zenoh_peer.put_packet(data)
	print("Sent: " + message)
	label.text = "Sent: " + str(countdown_number) + " to Player " + str(get_other_player_id())

func _on_countdown_tick():
	if countdown_number > 0 and is_counting_down:
		countdown_number -= 1
		_send_count()
		countdown_timer.start()  # Continue countdown
	else:
		# Countdown finished
		is_counting_down = false
		print("Countdown finished!")
		label.text = "Countdown finished! Ping pong complete."

func _on_poll_timeout():
	# Poll for network messages
	zenoh_peer.poll()

	var packet_count = zenoh_peer.get_available_packet_count()
	if packet_count > 0:
		print("DEBUG: " + str(packet_count) + " packets available")

	# Check for received packets
	while zenoh_peer.get_available_packet_count() > 0:
		var data = zenoh_peer.get_packet()
		var data_string = data.get_string_from_utf8()
		print("DEBUG: Received packet with length " + str(data.size()) + " bytes")
		print("DEBUG: Packet content: '" + data_string + "'")

		# Handle countdown message
		if data_string.begins_with("COUNT:"):
			var count_str = data_string.substr(6)
			var count = int(count_str)
			last_received_count = count

			print("DEBUG: Parsed count = " + str(count) + " from '" + count_str + "'")

			# Reset and start counting down from 10
			countdown_number = 10
			is_counting_down = true

			label.text = "Received: " + str(count) + " - Resetting to 10..."
			print("Received count, resetting countdown to 10")

			# Stop any existing countdown and start new one
			countdown_timer.stop()
			_send_count()
			countdown_timer.start()
		else:
			print("DEBUG: Received non-COUNT message: '" + data_string + "'")
	else:
		# Debug poll - remove this after testing
		if not is_host:
			print("DEBUG: Client polling, no packets available")

func get_other_player_id():
	return 2 if my_id == 1 else 1

func _notification(what):
	if what == NOTIFICATION_EXIT_TREE:
		if zenoh_peer:
			print("Cleaning up zenoh peer...")
			zenoh_peer.close()
