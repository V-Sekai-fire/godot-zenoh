extends Node

# Auto-demo Zenoh networking with peer communication demonstration
const CoordinatorStateMachine = preload("res://godot_zenoh/core/election_genserver.gd")
const ConnectionStateMachine = preload("res://godot_zenoh/core/connection_genserver.gd")
const NetworkingStateMachine = preload("res://godot_zenoh/core/game_genserver.gd")

var zenoh_peer: ZenohMultiplayerPeer
var coordinator: CoordinatorStateMachine
var connection: ConnectionStateMachine
var networking: NetworkingStateMachine
var demo_timer: Timer

var label: Label
var peer_id: String
var messages_sent: int = 0
var messages_received: int = 0

func _ready():
	# Initialize state machines
	coordinator = CoordinatorStateMachine.new()
	connection = ConnectionStateMachine.new()
	networking = NetworkingStateMachine.new()

	var c_init = coordinator.init({"id": 1})
	var conn_init = connection.init({})
	var net_init = networking.init({})

	if c_init[0] != "ok" or conn_init[0] != "ok" or net_init[0] != "ok":
		push_error("State machine initialization failed")
		if label: label.text = "❌ State Machine Init Failed"
		return

	setup_ui()
	_start_auto_connection()

	# Set up demo timer for automated peer communication
	demo_timer = Timer.new()
	demo_timer.wait_time = 2.0  # Send message every 2 seconds
	demo_timer.autostart = true
	demo_timer.connect("timeout", Callable(self, "_send_peer_message"))
	add_child(demo_timer)

func _start_auto_connection():
	# Initialize Zenoh network auto-connection
	zenoh_peer = ZenohMultiplayerPeer.new()
	zenoh_peer.game_id = "godot_zenoh_state_machine_test"

	# Generate unique peer identity for this instance
	var rng = RandomNumberGenerator.new()
	rng.randomize()
	peer_id = "Peer_" + str(rng.randi_range(1000, 9999))

	# Try client connection first (connects to existing server), fall back to server
	var client_result = zenoh_peer.create_client("localhost", 7447)
	if client_result == 0:
		label.text = "✅ CLIENT CONNECTED to Zenoh Network\nPeer ID: " + peer_id + "\nState: FOLLOWER"
		print("CLIENT: ", peer_id, " connected to network")
	else:
		# No server available, become the server (coordinator)
		var server_result = zenoh_peer.create_server(7447, 32)
		if server_result == 0:
			label.text = "✅ SERVER CREATED - Network Coordinator\nPeer ID: " + peer_id + "\nState: LEADER"
			print("SERVER: ", peer_id, " created network - acting as coordinator")
		else:
			label.text = "❌ Network connection failed"
			print("ERROR: ", peer_id, " failed to join network")

func _process(delta):
	if zenoh_peer:
		zenoh_peer.poll()

		# Check for incoming messages from other peers
		if zenoh_peer.get_available_packet_count() > 0:
			messages_received += 1
			var packet = zenoh_peer.get_packet()
			var msg = packet.get_string_from_utf8()
			print("📨 RECEIVED from peer: ", msg)
			update_status()

func _send_peer_message():
	if zenoh_peer and zenoh_peer.get_connection_status() == 2:  # CONNECTED
		messages_sent += 1
		var message = peer_id + "_msg_" + str(messages_sent) + "_time_" + str(Time.get_ticks_msec())
		var data = PackedByteArray()
		data.append_array(message.to_utf8_buffer())

		# Set channel first, then send packet
		zenoh_peer.set_transfer_channel(1)
		if zenoh_peer.put_packet(data) == 0:
			print("📤 SENT: ", message)
		else:
			print("⚠️  Failed to send message")

		update_status()

func update_status():
	var status = "Peer ID: " + peer_id + "\n"
	status += "Messages Sent: " + str(messages_sent) + "\n"
	status += "Messages Received: " + str(messages_received) + "\n"

	# Show connected peer count (estimate based on connection type)
	var peer_count = 1  # This peer
	if zenoh_peer and zenoh_peer.get_connection_status() == 2:
		peer_count = 2  # At least server + this client
	status += "Peers in Network: " + str(peer_count) + "\n"

	status += "Coordinator State: " + coordinator.get_state() + "\n"
	status += "Connection State: " + connection.get_state() + "\n"
	status += "Network State: " + networking.get_state()

	if label:
		label.text = status

func _notification(what):
	if what == NOTIFICATION_EXIT_TREE:
		if demo_timer:
			demo_timer.stop()
		if zenoh_peer:
			zenoh_peer.close()
		print("DEMO: ", peer_id, " disconnected")

func setup_ui():
	var vbox = VBoxContainer.new()
	add_child(vbox)
	vbox.set_anchors_preset(Control.PRESET_FULL_RECT)

	label = Label.new()
	label.text = "Initializing Zenoh Network Demo..."
	label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	vbox.add_child(label)

	var info = Label.new()
	info.text = "\nThis demo automatically connects multiple peers.\nCheck console for message exchange.\nRuns autonomously for CI/CD testing."
	info.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	vbox.add_child(info)

	# Auto-update status periodically
	var status_timer = Timer.new()
	status_timer.wait_time = 1.0
	status_timer.autostart = true
	status_timer.connect("timeout", Callable(self, "update_status"))
	add_child(status_timer)
