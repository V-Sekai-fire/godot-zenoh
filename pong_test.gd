# Ping pong countdown test between two Godot instances with Elixir-style GenServer state machines
extends Node

# Simple preload of GenServer implementations
const ElectionGenServer = preload("res://election_genserver.gd")
const ConnectionGenServer = preload("res://connection_genserver.gd")
const GameGenServer = preload("res://game_genserver.gd")

# Network peer
var zenoh_peer: ZenohMultiplayerPeer

# GenServer instances
var election_server: ElectionGenServer
var connection_server: ConnectionGenServer
var game_server = GameGenServer.new()  # Already created instance

# Simple UI
var label: Label
var button: Button

# Basic state
var connected = false
var election_complete = false

func _ready():
	print("🎮 Godot-Zenoh Tic-Tac-Toe Demo Starting...")
	print("📚 Using Elixir GenServer-style state machines")

	# Initialize GenServers
	_initialize_genservers()

	# Setup basic UI
	setup_ui()

	# Start Zenoh networking
	zenoh_peer = ZenohMultiplayerPeer.new()
	zenoh_peer.game_id = "pong_test"

	# Start automatic testing
	start_automatic_test()

func _initialize_genservers():
	print("🏗️ Initializing GenServer state machines...")

	election_server = ElectionGenServer.new()
	connection_server = ConnectionGenServer.new()

	# Initialize state machines
	var election_init = election_server.init({
		"my_id": 0,
		"game_id": "pong_test"
	})

	var connection_init = connection_server.init({})

	print("✅ GenServers initialized successfully")

func setup_ui():
	var vbox = VBoxContainer.new()
	add_child(vbox)
	vbox.set_anchors_preset(Control.PRESET_FULL_RECT)

	var title = Label.new()
	title.text = "Godot-Zenoh GenServer Demo"
	title.modulate = Color.GREEN
	title.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	vbox.add_child(title)

	label = Label.new()
	label.text = "Initializing Elixir-style state machines..."
	label.modulate = Color.YELLOW
	label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	vbox.add_child(label)

	button = Button.new()
	button.text = "Test GenServer Call"
	button.modulate = Color.CYAN
	button.connect("pressed", Callable(self, "_test_genserver_call"))
	vbox.add_child(button)

	var info = Label.new()
	info.text = "\n🎯 Testing GenServer Architecture:\n- Election GenServer: Message-driven leader election\n- Connection GenServer: Network state management\n- Game GenServer: Tic-Tac-Toe logic with HLC timing"
	info.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	vbox.add_child(info)

func start_automatic_test():
	print("🔬 Starting automatic GenServer functionality test...")

	# Test GenServer get_state calls
	var election_state = election_server.get_state()
	var connection_state = connection_server.get_state()

	print("📊 Initial GenServer states:")
	print("   ElectionServer: " + str(election_state))
	print("   ConnectionServer: " + str(connection_state))

	# Update UI
	label.text = "🎯 GenServer Test Ready\nElection: " + str(election_state) + "\nConnection: " + str(connection_state)

func _test_genserver_call():
	if not election_server or not connection_server:
		print("❌ GenServers not initialized")
		return

	print("🧪 Testing GenServer synchronous calls...")

	# Test synchronous GenServer call
	var call_result = connection_server.handle_call("get_status", self, {})

	if call_result[0] == "reply":
		var status = call_result[1]
		print("✅ Connection GenServer call successful: " + str(status))
		label.text = "🎯 GenServer Call Result:\n" + str(status)
	else:
		print("❌ Connection GenServer call failed")
		label.text = "❌ GenServer Call Failed"

func _process(delta):
	# Poll for network messages every frame
	if zenoh_peer:
		zenoh_peer.poll()

func _notification(what):
	if what == NOTIFICATION_EXIT_TREE:
		if zenoh_peer:
			print("🧹 Cleaning up Zenoh peer connection")
			zenoh_peer.close()
