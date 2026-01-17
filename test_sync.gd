# Automatic Synchronization Test for Zenoh Multiplayer Peer
# This demonstrates Godot's built-in synchronization using MultiplayerSynchronizer
# No manual sync protocol needed - Godot handles it automatically!
extends Node

var zenoh_peer: ZenohMultiplayerPeer
var synchronizer: MultiplayerSynchronizer
var is_host: bool = false
var my_id: int = -1

# Test synchronized properties
@export var game_score: int = 0:
	set(value):
		game_score = value
		if has_node("ScoreLabel"):
			get_node("ScoreLabel").text = "Score: " + str(game_score)

@export var player_count: int = 1:
	set(value):
		player_count = value
		if has_node("PlayerCountLabel"):
			get_node("PlayerCountLabel").text = "Players: " + str(player_count)

func _ready():
	print("=== Automatic Synchronization Test Starting ===")

	# Check command line args for auto-start
	var args = OS.get_cmdline_args()
	var auto_start = false
	var is_server = false

	for arg in args:
		if arg == "--server":
			auto_start = true
			is_server = true
		elif arg == "--client":
			auto_start = true
			is_server = false

	if auto_start:
		# Auto-start mode for testing
		zenoh_peer = ZenohMultiplayerPeer.new()
		zenoh_peer.game_id = "auto_sync_test"

		setup_networking()

		if is_server:
			print("🚀 Auto-starting as SERVER")
			start_server()
		else:
			print("🚀 Auto-starting as CLIENT")
			start_client()
	else:
		# Manual UI mode
		setup_ui()

func setup_networking():
	# Connect to peer signals
	zenoh_peer.peer_connected.connect(_on_peer_connected)
	zenoh_peer.peer_disconnected.connect(_on_peer_disconnected)
	zenoh_peer.connected_to_server.connect(_on_connected_to_server)
	zenoh_peer.connection_failed.connect(_on_connection_failed)

	# Set up MultiplayerSynchronizer for automatic sync
	synchronizer = MultiplayerSynchronizer.new()
	synchronizer.root_path = get_path()  # Sync properties of this node
	add_child(synchronizer)

	# Configure what properties to sync
	var config = SceneReplicationConfig.new()
	config.add_property(str(get_path()) + ":game_score")
	config.add_property(str(get_path()) + ":player_count")
	synchronizer.replication_config = config

func setup_ui():
	# Create UI for manual testing
	var vbox = VBoxContainer.new()
	vbox.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(vbox)

	var title = Label.new()
	title.text = "Zenoh Multiplayer - Automatic Sync Test"
	vbox.add_child(title)

	var host_btn = Button.new()
	host_btn.text = "Start Server"
	host_btn.connect("pressed", Callable(self, "start_server"))
	vbox.add_child(host_btn)

	var client_btn = Button.new()
	client_btn.text = "Start Client"
	client_btn.connect("pressed", Callable(self, "start_client"))
	vbox.add_child(client_btn)

	var score_btn = Button.new()
	score_btn.text = "Increase Score"
	score_btn.connect("pressed", Callable(self, "increase_score"))
	vbox.add_child(score_btn)

	var status_label = Label.new()
	status_label.name = "StatusLabel"
	status_label.text = "Not connected"
	vbox.add_child(status_label)

	var score_label = Label.new()
	score_label.name = "ScoreLabel"
	score_label.text = "Score: 0"
	vbox.add_child(score_label)

	var player_label = Label.new()
	player_label.name = "PlayerCountLabel"
	player_label.text = "Players: 1"
	vbox.add_child(player_label)

func start_server():
	print("🌐 Starting Zenoh server...")
	var result = zenoh_peer.create_server(7447, 32)
	if result != OK:
		print("❌ Failed to create server: ", result)
		return

	multiplayer.multiplayer_peer = zenoh_peer
	is_host = true
	my_id = multiplayer.get_unique_id()
	print("✅ Server started with ID: ", my_id)

	# Update UI if it exists
	if has_node("StatusLabel"):
		get_node("StatusLabel").text = "Server (ID: " + str(my_id) + ")"

func start_client():
	print("🌐 Starting Zenoh client...")
	var result = zenoh_peer.create_client("127.0.0.1", 7447)
	if result != OK:
		print("❌ Failed to create client: ", result)
		return

	multiplayer.multiplayer_peer = zenoh_peer
	is_host = false
	print("✅ Client started, connecting...")

	# Update UI if it exists
	if has_node("StatusLabel"):
		get_node("StatusLabel").text = "Connecting as client..."

func increase_score():
	if multiplayer.multiplayer_peer.get_connection_status() == MultiplayerPeer.CONNECTION_CONNECTED:
		game_score += 10
		print("📈 Score increased to: ", game_score)
	else:
		print("❌ Not connected - cannot modify synced properties")

func _on_peer_connected(id: int):
	print("🔗 Peer connected: ", id)
	player_count += 1

	if is_host:
		print("📤 Server: New peer connected, MultiplayerSynchronizer will automatically sync current state")

func _on_peer_disconnected(id: int):
	print("📤 Peer disconnected: ", id)
	player_count -= 1

func _on_connected_to_server():
	my_id = multiplayer.get_unique_id()
	print("✅ Connected to server with ID: ", my_id)

	if has_node("StatusLabel"):
		get_node("StatusLabel").text = "Client (ID: " + str(my_id) + ")"

	print("📥 Client: Connected to server, MultiplayerSynchronizer will automatically receive current state")

func _on_connection_failed():
	print("❌ Connection failed")

	if has_node("StatusLabel"):
		get_node("StatusLabel").text = "Connection failed"

func _process(_delta: float):
	# Poll the multiplayer system
	multiplayer.poll()