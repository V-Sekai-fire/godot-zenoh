# Ping pong countdown test between two Godot instances
extends Node

var zenoh_peer: ZenohMultiplayerPeer

var my_id: int = -1
var is_host: bool = false

var countdown_number: int = 10
var last_received_count: int = -1

var button: Button
var label: Label
var host_button: Button
var join_button: Button

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
	button.disabled = false

	# Set up polling timer
	var timer = Timer.new()
	timer.autostart = true
	timer.wait_time = 0.1  # Poll every 100ms
	timer.connect("timeout", Callable(self, "_on_poll_timeout"))
	add_child(timer)

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

func _on_poll_timeout():
	# Poll for network messages
	zenoh_peer.poll()

	# Check for received packets
	while zenoh_peer.get_available_packet_count() > 0:
		var data = zenoh_peer.get_packet()

		# Convert bytes to string
		var message = data.get_string_from_utf8()
		print("Received: " + message)

		# Update display
		label.text = "Received: " + message

		# Handle countdown message
		if message.begins_with("COUNT:"):
			var count_str = message.substr(6)
			var count = int(count_str)
			last_received_count = count
			label.text = "Received count: " + str(count) + " from Player " + str(get_other_player_id())

func get_other_player_id():
	return 2 if my_id == 1 else 1

func _notification(what):
	if what == NOTIFICATION_EXIT_TREE:
		if zenoh_peer:
			print("Cleaning up zenoh peer...")
			zenoh_peer.close()
