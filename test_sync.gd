# Late Joiner Synchronization Test for Zenoh Multiplayer Peer
# This demonstrates sync functionality using ONLY the standard MultiplayerPeer trait APIs
#
# Sync Protocol (pure trait methods only):
# - Server broadcasts sync data on channel 255 using put_packet() → automatically stored internally
# - Client requests sync by sending on channel 254 using put_packet()
# - Server automatically responds on channel 255 with stored sync data
# - Client receives sync data via normal get_packet() calls
# - NO custom API methods used - 100% trait compliant
extends Node

var zenoh_peer: ZenohMultiplayerPeer
var is_host: bool = false
var my_id: int = -1

var button: Button
var label: Label
var host_button: Button
var join_button: Button
var sync_button: Button
var request_sync_button: Button

# Test sync data
var game_score: int = 0
var player_count: int = 1

func _ready():
	print("=== Late Joiner Sync Test Starting ===")

	# Create UI
	setup_ui()

	# Initialize zenoh peer
	zenoh_peer = ZenohMultiplayerPeer.new()
	zenoh_peer.game_id = "sync_test"

func setup_ui():
	var vbox = VBoxContainer.new()
	add_child(vbox)
	vbox.set_anchors_preset(Control.PRESET_FULL_RECT)

	# Title
	var title = Label.new()
	title.text = "Zenoh Late Joiner Sync Test"
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

	# Sync buttons
	sync_button = Button.new()
	sync_button.text = "Broadcast Sync Data (Server)"
	sync_button.disabled = true
	sync_button.connect("pressed", Callable(self, "_on_set_sync_pressed"))
	vbox.add_child(sync_button)

	request_sync_button = Button.new()
	request_sync_button.text = "Request Sync (Client)"
	request_sync_button.disabled = true
	request_sync_button.connect("pressed", Callable(self, "_on_request_sync_pressed"))
	vbox.add_child(request_sync_button)

	# Status label
	label = Label.new()
	label.text = "Choose Host or Join to start..."
	vbox.add_child(label)

func _on_host_pressed():
	print("Starting as host...")
	is_host = true

	var result = zenoh_peer.create_server(7448, 32)
	if result == OK:
		my_id = zenoh_peer.get_unique_id()
		label.text = "Hosting - Player ID: " + str(my_id)
		print("Server started - ID: " + str(my_id))

		# Enable server-only buttons
		sync_button.disabled = false
		request_sync_button.disabled = true

		setup_networking()
	else:
		label.text = "Failed to host: " + str(result)

func _on_join_pressed():
	print("Joining as client...")
	is_host = false

	# Wait a bit for server to be ready
	await get_tree().create_timer(2.0).timeout

	var result = zenoh_peer.create_client("localhost", 7448)
	if result == OK:
		my_id = zenoh_peer.get_unique_id()
		label.text = "Joined - Player ID: " + str(my_id)
		print("Client connected - ID: " + str(my_id))

		# Enable client-only buttons
		sync_button.disabled = true
		request_sync_button.disabled = false

		setup_networking()
	else:
		label.text = "Failed to join: " + str(result)

func _on_set_sync_pressed():
	# Update game state
	game_score += 10
	player_count += 1

	# Create sync data
	var sync_data = {
		"score": game_score,
		"players": player_count,
		"timestamp": Time.get_unix_time_from_system()
	}

	var data_bytes = var_to_bytes(sync_data)

	# Broadcast sync data on channel 255 using standard put_packet()
	zenoh_peer.set_transfer_channel(255)
	var result = zenoh_peer.put_packet(data_bytes)
	zenoh_peer.set_transfer_channel(0) # Reset to default

	if result == OK:
		label.text = "Broadcasted sync: Score=" + str(game_score) + ", Players=" + str(player_count)
		print("✅ Broadcasted sync data on channel 255")
	else:
		label.text = "Failed to broadcast sync: " + str(result)

func _on_request_sync_pressed():
	# Request sync by sending a packet on channel 254
	var request_data = PackedByteArray()
	request_data.append_array("SYNC_REQUEST".to_utf8_buffer())
	zenoh_peer.set_transfer_channel(254)
	var result = zenoh_peer.put_packet(request_data)
	zenoh_peer.set_transfer_channel(0) # Reset to default
	if result == OK:
		label.text = "Requested sync from server..."
		print("📡 Requested sync from server")
	else:
		label.text = "Failed to request sync: " + str(result)

func setup_networking():
	print("Networking setup complete")

	# Set up polling timer
	var timer = Timer.new()
	timer.autostart = true
	timer.wait_time = 0.1  # Poll every 100ms
	timer.connect("timeout", Callable(self, "_on_poll_timeout"))
	add_child(timer)

func _on_poll_timeout():
	# Poll for packets
	if zenoh_peer:
		zenoh_peer.poll()

		# Check for incoming packets
		while zenoh_peer.get_available_packet_count() > 0:
			var packet = zenoh_peer.get_packet()
			var channel = zenoh_peer.get_packet_channel()

			if channel == 255:
				# Sync data from server (received via standard get_packet())
				print("📨 Received sync data (", packet.size(), " bytes) on channel 255")
				var sync_data = bytes_to_var(packet)
				if typeof(sync_data) == TYPE_DICTIONARY:
					label.text = "Received sync: Score=" + str(sync_data.get("score", 0)) + ", Players=" + str(sync_data.get("players", 1))
					print("✅ Received sync data: ", sync_data)
				else:
					print("⚠️ Received invalid sync data format")
			else:
				# Regular game packets on other channels
				print("📨 Received packet on channel ", channel, " (", packet.size(), " bytes)")