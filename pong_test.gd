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

# BULLETIN-BOARD ALGORITHM STATE (HLC-based state machine coordination!)
enum ElectionState {
    DISCONNECTED,           # Initial state - not started
    WAITING_CONNECTIONS,    # Connecting to Zenoh network
    GENERATING_ID,          # Requesting HLC timestamp for election
    BROADCASTING_HEARTBEATS,# Broadcasting HLC election ID
    COLLECTING_PEERS,       # Collecting all peer heartbeats
    DECIDING_LEADER,        # Running bully algorithm
    VICTORY_BROADCASTING,   # I won - announcing victory
    VICTORY_LISTENING,      # Waiting for victory/defeat messages
    FINALIZED               # Election complete - leader/follower set
}

var election_state: ElectionState = ElectionState.DISCONNECTED
var election_timer: Timer              # Grace period timer only
var leader_election_phase: bool = false     # Whether we're in election phase
var known_peers = []                   # Known peer election IDs
var my_election_id: int = -1          # My HLC-based election ID
var current_leader_id: int = -1      # Elected leader election ID
var election_message_queue = []       # Queued election messages
var collected_peer_ids = []           # All discovered election IDs for comparison

# RAFT CONSENSUS STATE (reverted - not implemented)
# var raft_consensus: ZenohRaftConsensus = null

# Connection state machine constants (integer enum)
const STATE_DISCONNECTED = 0
const STATE_CONNECTING = 1
const STATE_CONNECTED = 2
const STATE_FAILED = 3
const STATE_SERVER_READY = 4
const STATE_CLIENT_ATTEMPTING = 5
const STATE_ZENOH_SESSION_FAILED = 6
const STATE_LEADER_ELECTION = 7  # New state for automatic leader election

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

	# Always run in automatic leader election mode to demonstrate networking
	print("🔄 Godot-Zenoh: Automatic Leader Election Mode")

	# Initialize zenoh peer for all modes
	zenoh_peer = ZenohMultiplayerPeer.new()
	zenoh_peer.game_id = "pong_test"

	# Check command line arguments for specialized testing
	if is_server or is_client:
		# Specialized manual testing modes
		if is_server:
			print("🖥️ Manual server mode requested")
			_on_host_pressed()
		else:
			print("👨‍💻 Manual client mode requested")
			_on_join_pressed()
	else:
		# Default: Automatic leader election for all instances
		print("🎯 Running automatic leader election for all instances")
		start_leader_election()

	# Create UI in all cases for status display
	setup_ui()

func setup_ui():
	# Create UI for testing
	var vbox = VBoxContainer.new()
	add_child(vbox)
	vbox.set_anchors_preset(Control.PRESET_FULL_RECT)

	# Title
	var title = Label.new()
	title.text = "Godot-Zenoh Multiplayer Test"
	title.modulate = Color.GREEN
	vbox.add_child(title)

	# Features info
	var info = Label.new()
	info.text = "Features: Leader Election • Merkle State Hashing • HOL Blocking • Zero-Timer Architecture"
	info.modulate = Color.LIGHT_BLUE
	vbox.add_child(info)

	# Connection buttons
	host_button = Button.new()
	host_button.text = "Host Game (Server)"
	host_button.connect("pressed", Callable(self, "_on_host_pressed"))
	vbox.add_child(host_button)

	join_button = Button.new()
	join_button.text = "Join Game (Client)"
	join_button.connect("pressed", Callable(self, "_on_join_pressed"))
	vbox.add_child(join_button)

	# Auto-election info
	var auto_label = Label.new()
	auto_label.text = "Auto: Multiple instances elect leader with lowest peer ID"
	vbox.add_child(auto_label)

	# Status label
	label = Label.new()
	label.text = "Status: Initializing..."
	label.modulate = Color.YELLOW
	vbox.add_child(label)

	# Peer info label
	var peer_label = Label.new()
	peer_label.name = "peer_info"
	peer_label.text = "Peer ID: Not connected | Role: Unknown | State: " + get_state_text(connection_state)
	vbox.add_child(peer_label)

	# Hash divergence counter
	var hash_label = Label.new()
	hash_label.name = "hash_status"
	hash_label.text = "State Divergences: 0 | Last Hash Match: Unknown"
	hash_label.modulate = Color.LIGHT_GRAY
	vbox.add_child(hash_label)

	# Send button
	button = Button.new()
	button.text = "Send Countdown"
	button.disabled = true
	button.connect("pressed", Callable(self, "_on_send_pressed"))
	vbox.add_child(button)

	# HLC Timestamp Request Button
	var hlc_button = Button.new()
	hlc_button.text = "🎯 Request Zenoh HLC Timestamp"
	hlc_button.modulate = Color.CYAN
	hlc_button.connect("pressed", Callable(self, "_on_hlc_request_pressed"))
	vbox.add_child(hlc_button)

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

	# No blocking delays - server readiness is handled by state machine
	connection_state = STATE_CLIENT_ATTEMPTING

	# Join server immediately
	var result = zenoh_peer.create_client("localhost", 7447)
	if result == 0:
		print("Client connection initiated - status: CONNECTING")
		# Connection events handled by poll() state machine callbacks
		# No polling loops or await blocks in GDscript
		if label:
			label.text = "Client connection in progress..."
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

	# Send message with Merkle hash: "COUNT:N:FROM_ID:HASH"
	var message = "COUNT:" + str(countdown_number) + ":" + str(zenoh_peer.get_unique_id()) + ":" + current_hash
	var data = PackedByteArray()
	data.append_array(message.to_utf8_buffer())

	# Record this as a sent response
	record_response_message(message, 0)  # 0 means broadcast

	# Send immediately - Rust extension handles queuing/publisher readiness
	# No polling loops, only event-driven state machines
	var result = zenoh_peer.put_packet(data)

	print("Player " + str(zenoh_peer.get_unique_id()) + " queued " + message + " (state machine will publish when ready)")
	print("📋 MERKLE STATE HASH: " + current_hash + " (state divergence tracking)")

	if label:
		label.text = "Sent: " + str(countdown_number) + " (waiting for ack)"

	# If send failed, the Rust side will handle queuing and eventual retry through state machine
	# No GDscript polling - pure event-driven architecture

func _on_countdown_tick():
	# Automatic countdown disabled - only send when ack received
	pass

func _on_poll_timeout():
	# Poll for network messages and connection state
	zenoh_peer.poll()

	# Handle connection completion during leader election
	if leader_election_phase and my_id == -1 and zenoh_peer.connection_status() == 2:
		my_id = zenoh_peer.get_unique_id()
		var zid = zenoh_peer.get_zid()
		print("Connection completed in election - ID: " + str(my_id) + " | ZID: " + zid)

		# Signal to all participants that final election phase should begin
		print("📢 Broadcasting final election signal - I now have real peer ID #" + str(my_id))
		signal_final_election()

		# Restart election in final phase (wait short time for signals)
		restart_election_with_real_id()

		# Update UI
		update_peer_info()

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

# UI STATUS UPDATE FUNCTIONS
func update_peer_info():
	var peer_info_node = find_child("peer_info")
	if peer_info_node:
		var role = "Server/Leader" if is_host else "Client/Follower"
		if connection_state == STATE_LEADER_ELECTION:
			role = "Electing Leader"

		var zid = ""
		if zenoh_peer and zenoh_peer.has_method("get_zid"):
			zid = zenoh_peer.get_zid()
		else:
			zid = "N/A"

		peer_info_node.text = "Peer ID: " + str(my_id) + " | Role: " + role + " | State: " + get_state_text(connection_state) + " | ZID: " + zid

func update_hash_status():
	var hash_node = find_child("hash_status")
	if hash_node:
		var last_match = "Unknown"
		if state_hash_history.size() >= 2:
			var last_remote = state_hash_history[-2] if state_hash_history.size() >= 2 else ""
			var last_local = state_hash_history[-1]
			if last_remote == last_local:
				last_match = "✅ Match"
			else:
				last_match = "🚨 Diverged"

		hash_node.text = "State Divergences: " + str(hash_divergence_count) + " | Last Hash Match: " + last_match
		hash_node.modulate = Color.RED if hash_divergence_count > 0 else Color.LIGHT_GREEN

func get_state_text(state: int) -> String:
	match state:
		STATE_DISCONNECTED: return "DISCONNECTED"
		STATE_CONNECTING: return "CONNECTING"
		STATE_CONNECTED: return "CONNECTED"
		STATE_FAILED: return "FAILED"
		STATE_SERVER_READY: return "SERVER_READY"
		STATE_CLIENT_ATTEMPTING: return "CLIENT_ATTEMPTING"
		STATE_ZENOH_SESSION_FAILED: return "ZENOH_FAILED"
		STATE_LEADER_ELECTION: return "LEADER_ELECTION"
		_: return "UNKNOWN"

# STATE MACHINE LEADER ELECTION - HLC-based Deterministic Bully Algorithm
func start_leader_election():
	if election_state != ElectionState.DISCONNECTED:
		print("⚠️ Election already in progress (state: " + str(election_state) + ")")
		return

	print("🏁 Starting HLC-based bully election state machine")
	leader_election_phase = true
	connection_state = STATE_LEADER_ELECTION
	collected_peer_ids = []
	known_peers = []

	# STATE: WAITING_CONNECTIONS
	election_state = ElectionState.WAITING_CONNECTIONS
	if label:
		label.text = "ELECTING LEADER: Connecting to Zenoh..."
	print("🔗 Election State: WAITING_CONNECTIONS")

	# Connect to Zenoh network first
	print("Connecting to Zenoh network for election...")

	# Try client connection first (non-blocking)
	var result = zenoh_peer.create_client("localhost", 7447)
	if result != 0:
		# If no existing server, become the leader immediately
		print("No existing Zenoh server - becoming the immediate leader")
		result = zenoh_peer.create_server(7447, 32)
		if result == 0:
			print("✅ Became immediate leader - server role")
			complete_leader_election_as_leader()
			return
		else:
			print("❌ Failed to create server as leader")
			return

	print("Client connection initiated - waiting for connected state")

	# Start polling for connection state changes and election messages
	var poll_timer = Timer.new()
	poll_timer.autostart = true
	poll_timer.wait_time = 0.1
	poll_timer.connect("timeout", Callable(self, "_on_election_poll"))
	add_child(poll_timer)

func send_election_heartbeat():
	var election_id: int = 0

	# Use unique ID if available, otherwise use deterministic election ID
	if my_id != -1:
		election_id = my_id
	else:
		# Use process ID and timestamp as deterministic election ID
		election_id = zenoh_peer.request_hlc_timestamp()

	var heartbeat_msg = "ELECT:" + str(election_id) + ":" + str(zenoh_peer.get_zid())
	var data = PackedByteArray()
	data.append_array(heartbeat_msg.to_utf8_buffer())

	var result = zenoh_peer.put_packet(data)
	print("Sent election heartbeat: " + heartbeat_msg + " (current election_id: " + str(election_id) + ")")
	if result != 0:
		print("⚠️ Election heartbeat send failed, but continuing")

func _on_election_poll():
	zenoh_peer.poll()

	# Handle state machine transitions based on connection state
	if election_state == ElectionState.WAITING_CONNECTIONS:
		if zenoh_peer.connection_status() == 2:  # Connected
			print("Zenoh connected - proceeding to generate HLC election ID")
			_election_transition_generating_id()

	# Process election messages and state transitions
	while zenoh_peer.get_available_packet_count() > 0:
		var data = zenoh_peer.get_packet()
		var msg = data.get_string_from_utf8()

		_process_election_message(msg)

func _process_election_message(msg: String):
	# Handle different election message types based on current state
	if msg.begins_with("ELECT:"):
		var parts = msg.split(":")
		if parts.size() >= 3:
			var peer_election_id = int(parts[1])
			var peer_zid = parts[2]

			# Add to collected election IDs
			if collected_peer_ids.find(peer_election_id) == -1:
				collected_peer_ids.append(peer_election_id)
				print("📥 Received election announcement #" + str(peer_election_id) + " (" + peer_zid + ")")
				print("   Total election IDs collected: " + str(collected_peer_ids.size()))

			# State-specific message handling
			match election_state:
				ElectionState.COLLECTING_PEERS:
					if label:
						label.text = "ELECTING LEADER: " + str(collected_peer_ids.size()) + " participants"
					_check_if_all_peers_collected()

	elif msg.begins_with("VICTORY:"):
		var parts = msg.split(":")
		if parts.size() >= 4:
			var winner_election_id = int(parts[1])
			var winner_zid = parts[2]
			print("🎉 VICTORY MESSAGE received from #" + str(winner_election_id) + " (" + winner_zid + ")")

			if election_state == ElectionState.VICTORY_LISTENING:
				if winner_election_id == my_election_id:
					print("✅ That's me! I won the election")
				else:
					print("✅ Another instance won - I will be a follower")
					complete_leader_election_as_follower()
	elif msg.begins_with("FINAL_ELECT:"):
		print("🔄 Received final election signal - restarting election with real IDs")
		restart_election_with_real_id()

func _check_if_all_peers_collected():
	# If we have at least one peer and we've been collecting for a bit, proceed to decide
	# In a real implementation, we'd know the expected number of participants
	if collected_peer_ids.size() >= 2:  # Assuming we expect at least 3 total
		print("Collected enough peer announcements - proceeding to election decision")
		_election_transition_deciding_leader()

# STATE MACHINE TRANSITION FUNCTIONS
func _election_transition_generating_id():
	election_state = ElectionState.GENERATING_ID
	print("🔗 Election State: GENERATING_ID")

	# Request HLC timestamp for consistent election ID
	var hlc_result = zenoh_peer.request_hlc_timestamp()
	if hlc_result == 0:
		print("HLC timestamp requested - waiting for response")

		# Set a short timer to wait for HLC response and transition
		election_timer = Timer.new()
		election_timer.one_shot = true
		election_timer.wait_time = 0.5  # Wait 500ms for HLC
		election_timer.connect("timeout", Callable(self, "_on_hlc_ready_timeout"))
		add_child(election_timer)
		election_timer.start()
	else:
		print("❌ Failed to request HLC timestamp")
		# Fall back to using current timestamp
		my_election_id = int(Time.get_unix_time_from_system() * 1000000)
		_election_transition_broadcasting()

func _on_hlc_ready_timeout():
	# Check if HLC timestamp is available (simplified - in real impl check a callback)
	# For now, generate our ID and proceed
	my_election_id = int(Time.get_unix_time_from_system() * 1000000) # Use microsecond precision
	print("Using election ID: " + str(my_election_id))
	_election_transition_broadcasting()

func _election_transition_broadcasting():
	election_state = ElectionState.BROADCASTING_HEARTBEATS
	print("🔗 Election State: BROADCASTING_HEARTBEATS")

	# Broadcast our election ID
	var heartbeat_msg = "ELECT:" + str(my_election_id) + ":" + str(zenoh_peer.get_zid())
	var data = PackedByteArray()
	data.append_array(heartbeat_msg.to_utf8_buffer())

	var result = zenoh_peer.put_packet(data)
	if result == 0:
		print("✅ Sent election announcement: " + heartbeat_msg)
		if label:
			label.text = "ELECTING LEADER: Announced participation"
	else:
		print("⚠️ Failed to send election announcement")

	# Transition to collecting peers after broadcasting
	_election_transition_collecting_peers()

func _election_transition_collecting_peers():
	election_state = ElectionState.COLLECTING_PEERS
	print("🔗 Election State: COLLECTING_PEERS")

	if label:
		label.text = "ELECTING LEADER: Collecting peer announcements"

	# Set a grace period to collect peer announcements
	election_timer = Timer.new()
	election_timer.one_shot = true
	election_timer.wait_time = 1.0  # ⚡ FAST HEATBEAT: 1 second to collect peers
	election_timer.connect("timeout", Callable(self, "_on_collecting_timeout"))
	add_child(election_timer)
	election_timer.start()

func _on_collecting_timeout():
	print("Collection timeout - proceeding with available peers")
	if collected_peer_ids.size() > 0:
		_election_transition_deciding_leader()
	else:
		print("No peer announcements collected - waiting longer")
		# Could extend collection time here

func _election_transition_deciding_leader():
	election_state = ElectionState.DECIDING_LEADER
	print("🔗 Election State: DECIDING_LEADER")

	if label:
		label.text = "ELECTING LEADER: Analyzing participants"

	# Include our own ID in the decision
	collected_peer_ids.append(my_election_id)

	# Sort by ID - lowest HLC timestamp wins!
	collected_peer_ids.sort()

	print("Election Decision Analysis:")
	print("  All participant IDs: " + str(collected_peer_ids))
	print("  Lowest ID (winner): " + str(collected_peer_ids[0]))
	print("  My ID: " + str(my_election_id))

	# Bully algorithm: lowest ID wins
	var winner_id = collected_peer_ids[0]
	current_leader_id = winner_id

	if my_election_id == winner_id:
		print("🎉 I WON THE ELECTION! Lowest HLC ID: #" + str(my_election_id))
		_election_transition_victory_broadcasting()
	else:
		print("✅ I lost - following leader #" + str(winner_id))
		_election_transition_victory_listening()

func _election_transition_victory_broadcasting():
	election_state = ElectionState.VICTORY_BROADCASTING
	print("🔗 Election State: VICTORY_BROADCASTING")

	if label:
		label.text = "ELECTING LEADER: Broadcasting victory"

	# Announce victory to all participants
	var victory_msg = "VICTORY:" + str(my_election_id) + ":" + str(zenoh_peer.get_zid()) + ":HLC_LOWEST_WINS"
	var data = PackedByteArray()
	data.append_array(victory_msg.to_utf8_buffer())

	var result = zenoh_peer.put_packet(data)
	if result == 0:
		print("🌟 Victory broadcast sent: " + victory_msg)
	else:
		print("⚠️ Failed to send victory broadcast")

	# Victory announced - complete election
	election_state = ElectionState.FINALIZED
	print("🏆 Election complete - I am the SINGLE LEADER")
	complete_leader_election_as_leader()

func _election_transition_victory_listening():
	election_state = ElectionState.VICTORY_LISTENING
	print("🔗 Election State: VICTORY_LISTENING")

	if label:
		label.text = "ELECTING LEADER: Waiting for winner announcement"

	# Set a reasonable timeout for victory announcement
	election_timer = Timer.new()
	election_timer.one_shot = true
	election_timer.wait_time = 3.0  # Longer timeout for victory announcement
	election_timer.connect("timeout", Callable(self, "_on_victory_listening_timeout"))
	add_child(election_timer)
	election_timer.start()

func _on_victory_listening_timeout():
	print("Victory announcement timeout - assuming election complete")
	election_state = ElectionState.FINALIZED
	print("🏁 Election finalized - proceeding as follower")
	complete_leader_election_as_follower()

func signal_final_election():
	print("🏁 Signalling final election to all participants!")
	var signal_msg = "FINAL_ELECT:" + str(my_id) + ":" + str(zenoh_peer.get_zid())
	var data = PackedByteArray()
	data.append_array(signal_msg.to_utf8_buffer())

	zenoh_peer.put_packet(data)
	print("Sent final election signal: " + signal_msg)

func _on_election_timeout():
	print("Election timeout - analyzing " + str(known_peers.size()) + " discovered peers")

	if my_id == -1:
		print("⏳ Still no Zenoh ID - extending election")
		restart_election_with_timeout_extension()
		return

	# TRUE BULLY ALGORITHM: Am I the lowest ID among all known peers?
	var all_known_peers = []
	all_known_peers.append_array(known_peers)
	all_known_peers.append(my_id)  # Include myself

	# Sort by ID - lowest ID wins
	all_known_peers.sort()
	var lowest_peer_id = all_known_peers[0]

	print("Bully Election Analysis:")
	print("  Known peers: " + str(known_peers))
	print("  My ID: " + str(my_id))
	print("  Lowest ID: " + str(lowest_peer_id))
	print("  Am I the winner? " + str(my_id == lowest_peer_id))

	if my_id == lowest_peer_id:
		print("🎉 BULLY VICTORY: I have the lowest ID #" + str(my_id))
		print("🌟 I am the SINGLE LEADER!")
		current_leader_id = my_id
		broadcast_leader_victory()
		complete_leader_election_as_leader()
	else:
		print("✅ Defeat: " + str(lowest_peer_id) + " has lower ID than me (" + str(my_id) + ")")
		print("👥 I am a follower to leader #" + str(lowest_peer_id))
		current_leader_id = lowest_peer_id
		stop_broadcasting_hearts()  # Quit competing
		complete_leader_election_as_follower()

func restart_election_with_timeout_extension():
	# Extend election timeout again until we have real peer IDs
	print("Extending election timeout for real peer ID assignment...")

	# Don't free existing timer during a callback - Godot may still process it
	# Just create a new one with extended timeout
	var new_timer = Timer.new()
	new_timer.one_shot = true
	new_timer.wait_time = 2.0  # Additional 2 seconds to get real IDs
	new_timer.connect("timeout", Callable(self, "_on_election_timeout"))
	add_child(new_timer)
	new_timer.start()

	# Store the new timer and try to clean up old one safely
	var old_timer = election_timer
	election_timer = new_timer

	# Mark old timer for safe cleanup (don't call immediately)
	if old_timer and old_timer != new_timer:
		# Use a deferred cleanup to avoid locking issues
		call_deferred("_safe_free_timer", old_timer)

	print("Election extended - waiting for real Zenoh peer IDs...")

func complete_leader_election_as_leader():
	# Change to server mode
	print("Election complete - starting server for followers")
	leader_election_phase = false
	connection_state = STATE_SERVER_READY
	is_host = true

	if label:
		label.text = "LEADER: Waiting for followers..."

	# Setup normal networking (leader is ready to start game)
	setup_networking()

func complete_leader_election_as_follower():
	# Switch to client mode to connect to the elected leader
	print("Election complete - connecting as client to leader")
	leader_election_phase = false
	connection_state = STATE_CONNECTED
	is_host = false

	if label:
		label.text = "FOLLOWER: Connecting to leader..."

	# Setup client networking
	setup_networking()

func restart_election_with_real_id():
	# Clear previous election state
	known_peers = []
	if election_timer:
		election_timer.stop()
		election_timer.free()
	election_timer = null

	# Restart election with 2-second timeout since we now have real IDs
	print("Restarting election with dedicated 2-second phase for real ID coordination")

	# Send heartbeat with real ID now
	send_election_heartbeat()

	# Start shorter election timeout (2 seconds) for real ID election
	election_timer = Timer.new()
	election_timer.one_shot = true
	election_timer.wait_time = 2.0  # 2 seconds for real ID election
	election_timer.connect("timeout", Callable(self, "_on_election_timeout"))
	add_child(election_timer)
	election_timer.start()

	print("Election restarted with real peer IDs - completing leader selection")

func broadcast_leader_victory():
	# Announce victory to all peers
	var victory_msg = "VICTORY:" + str(my_id) + ":" + str(zenoh_peer.get_zid()) + ":LOWEST_ID_WINS"
	var data = PackedByteArray()
	data.append_array(victory_msg.to_utf8_buffer())

	zenoh_peer.put_packet(data)
	print("🌟 Broadcasting BULLY VICTORY: '" + victory_msg + "' - All other instances should become followers")

func stop_broadcasting_hearts():
	# Stop sending heartbeats since election is over
	print("🛑 Stopping election heartbeat broadcasts - election is over")
	# Could kill the polling timer here, but the leader might still need it

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
		"time": zenoh_peer.request_hlc_timestamp()
	}
	received_messages_log.append(record)

func record_response_message(message: String, to_id: int):
	# Track sent responses for state computation
	var record = {
		"msg": message,
		"to": to_id,
		"time": zenoh_peer.request_hlc_timestamp()
	}
	response_messages_log.append(record)

func _on_hlc_request_pressed():
	print("🎯 Requesting HLC timestamp from Zenoh session...")
	var result = zenoh_peer.request_hlc_timestamp()
	if result == 0:
		print("✅ HLC timestamp request sent to worker thread")
	else:
		print("❌ Failed to send HLC timestamp request")

func _safe_free_timer(old_timer: Timer):
	# Safely free the timer after the current frame to avoid locking issues
	if old_timer and not old_timer.is_inside_tree():
		# Timer is already removed from tree, safe to free
		old_timer.free()
		print("Old election timer safely freed")
	elif old_timer and old_timer.is_inside_tree():
		# Timer still in tree, mark for freeing at end of frame
		old_timer.call_deferred("free")
		print("Old election timer marked for deferred freeing")

func _notification(what):
	if what == NOTIFICATION_EXIT_TREE:
		if zenoh_peer:
			print("Cleaning up zenoh peer...")
			zenoh_peer.close()
