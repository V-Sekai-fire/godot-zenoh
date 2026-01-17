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

# Connection state machine constants (integer enum)
const STATE_DISCONNECTED = 0
const STATE_CONNECTING = 1
const STATE_CONNECTED = 2
const STATE_FAILED = 3
const STATE_SERVER_READY = 4
const STATE_CLIENT_ATTEMPTING = 5
const STATE_ZENOH_SESSION_FAILED = 6

# Connection state machine variables
var connection_state: int = STATE_DISCONNECTED

# MERKLE HASH STATE TRACKING - for detecting peer state divergence
var received_messages_log: Array = []  # Messages received from peers
var response_messages_log: Array = []  # Messages sent in response
var state_hash_history: Array = []     # Hash of local state at each message
var hash_context = HashingContext.new()  # For cryptographically secure SHA-256 state hashing
var hash_divergence_count: int = 0     # How many times state diverged

func _ready():
	print("Pong Test Starting...")

	# Check command line arguments for automatic mode
	var args = OS.get_cmdline_args()
	var is_server = args.has("--server")
	var is_client = args.has("--client")

	if is_server or is_client:
		print("Running in automatic mode - skip UI")
		# Initialize zenoh peer
		zenoh_peer = ZenohMultiplayerPeer.new()
		zenoh_peer.game_id = "pong_test"

		if is_server:
			print("Auto-starting as server...")
			_on_host_pressed()
		else:
			print("Auto-starting as client...")
			_on_join_pressed()
	else:
		print("Running in interactive mode - setup UI")
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

	# STATE MACHINE: Prevent multiple hosts in the same session
	if connection_state != STATE_DISCONNECTED:
		if label:
			label.text = "ALREADY connected! Disconnect first (State: " + str(connection_state) + ")"
		print("Already connected - cannot start another host session")
		return

	# STATE MACHINE: Set connecting state before attempting connection
	connection_state = STATE_CONNECTING

	# Start server
	var result = zenoh_peer.create_server(7447, 32)
	if result == 0:
		var client_id = zenoh_peer.get_unique_id()
		if label:
			label.text = "Hosting game - Player ID: " + str(client_id)
		print("Server Player ID: " + str(client_id))

		# STATE MACHINE: Successfully hosting server
		connection_state = STATE_SERVER_READY
		setup_networking()
	else:
		# STATE MACHINE: Server creation failed
		connection_state = STATE_DISCONNECTED
		if label:
			label.text = "Failed to host: " + str(result)

func _on_join_pressed():
	print("Joining as client...")
	is_host = false

	# STATE MACHINE: Check if already connected
	if zenoh_peer.connection_status() == 2:  # Already connected?
		if label:
			label.text = "ALREADY connected! Disconnect first"
		print("Already connected - cannot join as another client")
		return

	# STATE MACHINE: Set connecting state (avoid modifying zenoh_peer state directly)
	connection_state = STATE_CONNECTING

	# Add delay for server to be ready (but continue attempting in background)
	print("⏳ Waiting 2 seconds for server to fully initialize...")
	await get_tree().create_timer(2.0).timeout

	connection_state = STATE_CLIENT_ATTEMPTING

	# Join server
	var result = zenoh_peer.create_client("localhost", 7447)
	if result == 0:
		# Wait for connection to complete (poll until connected or timeout)
		var connection_timeout = 5.0  # 5 second timeout
		var start_time = Time.get_unix_time_from_system()
		var elapsed = 0.0

		while elapsed < connection_timeout:
			zenoh_peer.poll()  # Process async commands

			if zenoh_peer.connection_status() == 2:  # CONNECTED
				var client_id = zenoh_peer.get_unique_id()
				var zid = ""
				if zenoh_peer.has_method("get_zid"):
					zid = zenoh_peer.get_zid()
				else:
					zid = "get_zid not available"

				connection_state = STATE_CONNECTED
				if label:
					label.text = "Player ID: " + str(client_id) + " | ZID: " + zid
				print("Client connected - ID: " + str(client_id) + " | ZID: " + zid)
				setup_networking()
				return

			await get_tree().create_timer(0.1).timeout  # Wait 100ms
			elapsed = Time.get_unix_time_from_system() - start_time

		# Timeout - check final status
		var final_status = zenoh_peer.connection_status()
		if final_status == 2:  # CONNECTED
			var client_id = zenoh_peer.get_unique_id()
			var zid = zenoh_peer.get_zid()
			connection_state = STATE_CONNECTED
			if label:
				label.text = "Player ID: " + str(client_id) + " | ZID: " + zid
			print("Client connected after timeout - ID: " + str(client_id) + " | ZID: " + zid)
			setup_networking()
		else:
			connection_state = STATE_ZENOH_SESSION_FAILED
			if label:
				label.text = "Connection timeout | Status: " + str(final_status)
			print("❌ Client connection timeout - Status: " + str(final_status))
	else:
		# STATE MACHINE: Complete failure
		connection_state = STATE_FAILED
		if label:
			label.text = "Failed to join: " + str(result)
		print("❌ Client create_client failed with error: " + str(result))

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
		if label:
			label.text = "Starting countdown..."
		is_counting_down = true
		countdown_number = 10
		_send_count()
		countdown_timer.start()
	else:
		# Client waits to receive first message
		if label:
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
	# Compute current state hash and update history
	var current_hash = compute_state_hash()
	state_hash_history.append(current_hash)

	# Wait for Zenoh publishers to be ready (polling loop instead of random delay)
	var wait_attempts = 0
	var max_attempts = 50  # 5 seconds max wait

	while wait_attempts < max_attempts:
		# Send message with Merkle hash: "COUNT:N:FROM_ID:HASH"
		var message = "COUNT:" + str(countdown_number) + ":" + str(zenoh_peer.get_unique_id()) + ":" + current_hash
		var data = PackedByteArray()
		data.append_array(message.to_utf8_buffer())

		# Record this as a sent response
		record_response_message(message, 0)  # 0 means broadcast

		# In Zenoh pub/sub: EVERY message published is automatically "relayed" to ALL subscribers
		# This provides the exact same functionality as server relay - no additional code needed!
		var result = zenoh_peer.put_packet(data)

		# Poll for publisher readiness
		zenoh_peer.poll()
		await get_tree().create_timer(0.1).timeout  # Poll every 100ms

		# Check if packet was actually published (not queued locally)
		# but after polling, if we still have available packets, it means they were published
		var packet_count_before = zenoh_peer.get_available_packet_count()
		zenoh_peer.poll()  # Another poll to ensure async updates
		var packet_count_after = zenoh_peer.get_available_packet_count()

		if result == 0 and packet_count_before < packet_count_after:  # Packet was published
			print("Player " + str(zenoh_peer.get_unique_id()) + " published " + message + " (Zenoh auto-relays to all subscribers)")
			print("📋 MERKLE STATE HASH: " + current_hash + " (state divergence tracking)")
			if label:
				label.text = "Sent: " + str(countdown_number) + " (waiting for ack)"
			return

		wait_attempts += 1
		print("Zenoh publisher not ready, retrying... (" + str(wait_attempts) + "/" + str(max_attempts) + ")")
		if label:
			label.text = "Initializing network... " + str(countdown_number) + " (attempt " + str(wait_attempts) + ")"

	# Timeout - send anyway and continue
	print("Publisher timeout - sending message anyway")
	if label:
		label.text = "Sent: " + str(countdown_number) + " (waiting for ack)"

	if label:
		label.text = "Force sent: " + str(countdown_number) + " (waiting for ack)"

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

		# Handle countdown message with Merkle hash: "COUNT:N:FROM_ID:HASH"
		if data_string.begins_with("COUNT:"):
			var parts = data_string.split(":")
			var count = -1
			var from_player_id = -1
			var received_hash = ""

			if parts.size() >= 4:  # New format with hash
				count = int(parts[1])
				from_player_id = int(parts[2])
				received_hash = parts[3]
				print("Player " + str(zenoh_peer.get_unique_id()) + " received COUNT:" + str(count) + " from Player " + str(from_player_id) + " (Merkle hash: " + received_hash + ")")

				# MERKLE HASH COMPARISON: Check for state divergence
				record_received_message(data_string, from_player_id)
				var local_hash = compute_state_hash()
				if received_hash != local_hash:
					hash_divergence_count += 1
					print("🚨 STATE DIVERGENCE DETECTED #" + str(hash_divergence_count) + "!")
					print("   Remote hash: " + received_hash)
					print("   Local hash:  " + local_hash)
					if label:
						label.text = "⚠️ STATE DIVERGED (hash mismatch)"
				else:
					print("✅ Merkle hash consensus maintained")

			elif parts.size() >= 3:  # Old format without hash
				count = int(parts[1])
				from_player_id = int(parts[2])
				print("Player " + str(zenoh_peer.get_unique_id()) + " received COUNT:" + str(count) + " from Player " + str(from_player_id))
				record_received_message(data_string, from_player_id)
			else:
				# Fallback for old format
				var count_str = data_string.substr(6)
				count = int(count_str)
				from_player_id = get_other_player_id()
				print("Player " + str(zenoh_peer.get_unique_id()) + " received COUNT:" + str(count) + " from Player " + str(from_player_id) + " (legacy format)")
				record_received_message(data_string, from_player_id)

			last_received_count = count

			# Acknowledge receipt by decrementing and sending next number (after 1 second delay)
			# Only respond to messages from other peers (not own messages)
			if countdown_number > 0 and count >= 0 and from_player_id != zenoh_peer.get_unique_id():
				if label:
					label.text = "Received: " + str(count) + " - Preparing response..."
				print("Player " + str(zenoh_peer.get_unique_id()) + " acknowledging receipt of " + str(count) + " from " + str(from_player_id) + " - will respond in 1 second with countdown: " + str(countdown_number))

				# In automatic mode, complete the exchange and exit
				var args = OS.get_cmdline_args()
				if args.has("--client") and count <= 1:  # Exit after complete minimal exchange
					print("Client test successful - completed packet exchange!")
					if hash_divergence_count == 0:
						print("🎉 Perfect! Zero state divergences detected")
					else:
						print("⚠️ Warning: " + str(hash_divergence_count) + " state divergences occurred")
					get_tree().quit()

				# Wait 1 second before responding (doesn't block the polling)
				var response_timer = Timer.new()
				response_timer.one_shot = true
				response_timer.wait_time = 1.0
				response_timer.connect("timeout", Callable(self, "_delayed_response"))
				add_child(response_timer)
				response_timer.start()
			else:
				if label:
					label.text = "Game already finished"

func _delayed_response():
	# This runs after 1 second delay
	if countdown_number > 0:
		countdown_number -= 1
		if countdown_number == 0:
			if label:
				label.text = "GAME OVER!"
			print("Countdown complete!")
		else:
			if label:
				label.text = "Responding with: " + str(countdown_number)
			print("After 1 second - sending countdown: " + str(countdown_number))
			_send_count()
			# Wait for next receipt (no auto countdown)

func get_other_player_id():
	return 2 if my_id == 1 else 1

# MERKLE HASH STATE COMPUTATION - for state divergence detection using Godot's HashingContext
func compute_state_hash() -> String:
	# Create state object representing SHARED game state (exclude identity for consensus)
	var state = {
		"countdown": countdown_number,
		"connection_state": connection_state,
		"last_received": last_received_count,
		"is_counting_DOWN": str(is_counting_down),  # Convert bool to string for hashing
		"divergences_found": hash_divergence_count,
		# Recent message logs (chronological order) - core game state history
		"received_recent": received_messages_log.slice(max(0, received_messages_log.size()-10)),
		"response_recent": response_messages_log.slice(max(0, response_messages_log.size()-10))
	}

	# Convert state to JSON string for consistent hashing
	var state_json = JSON.stringify(state)

	# Use Godot's HashingContext for cryptographically secure SHA-256
	hash_context.start(HashingContext.HashType.HASH_SHA256)
	hash_context.update(state_json.to_utf8_buffer())
	var hash_bytes = hash_context.finish()

	# Convert to hex string for consistent representation
	return hash_bytes.hex_encode()

func record_received_message(message: String, from_id: int):
	# Track received messages for state computation
	var record = {
		"msg": message,
		"from": from_id,
		"time": Time.get_unix_time_from_system()
	}
	received_messages_log.append(record)

func record_response_message(message: String, to_id: int):
	# Track sent responses for state computation
	var record = {
		"msg": message,
		"to": to_id,
		"time": Time.get_unix_time_from_system()
	}
	response_messages_log.append(record)

func _notification(what):
	if what == NOTIFICATION_EXIT_TREE:
		if zenoh_peer:
			print("Cleaning up zenoh peer...")
			zenoh_peer.close()
