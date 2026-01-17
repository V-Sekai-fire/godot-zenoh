# Simple Godot-Zenoh Tic-Tac-Toe with GenServer State Machines
extends Node

const ElectionGenServer = preload("res://election_genserver.gd")
const ConnectionGenServer = preload("res://connection_genserver.gd")
const GameGenServer = preload("res://game_genserver.gd")

var label: Label
var button: Button

var zenoh_peer: ZenohMultiplayerPeer
var election_server: ElectionGenServer
var connection_server: ConnectionGenServer
var game_server = GameGenServer.new()

func _ready():
	print("🎮 Godot-Zenoh GenServer Demo - Auto-connecting...")

	# Initialize GenServers
	election_server = ElectionGenServer.new()
	connection_server = ConnectionGenServer.new()

	# Test that GenServers initialized
	var election_init = election_server.init({"my_id": 123})
	var connection_init = connection_server.init({})

	if election_init[0] == "ok" and connection_init[0] == "ok":
		print("✅ GenServers initialized successfully")

		# Create Zenoh peer and auto-connect
		zenoh_peer = ZenohMultiplayerPeer.new()
		zenoh_peer.game_id = "genserver_demo"

		# Try server first, fall back to client
		var server_result = zenoh_peer.create_server(7447, 32)
		if server_result == 0:
			print("✅ Auto-connected as SERVER/LEADER")
		else:
			var client_result = zenoh_peer.create_client("localhost", 7447)
			if client_result == 0:
				print("✅ Auto-connected as CLIENT/FOLLOWER")
			else:
				print("⚠️ Auto-connect failed - manual mode")
	else:
		print("❌ GenServer initialization failed")

	# Call setup_ui in _ready
	setup_ui()

func _process(delta):
	if zenoh_peer:
		zenoh_peer.poll()

func setup_ui():
	var vbox = VBoxContainer.new()
	add_child(vbox)
	vbox.set_anchors_preset(Control.PRESET_FULL_RECT)

	var title = Label.new()
	title.text = "Godot-Zenoh GenServer Test"
	title.modulate = Color.GREEN
	title.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	vbox.add_child(title)

	label = Label.new()
	label.text = "Initializing..."
	label.modulate = Color.YELLOW
	label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	vbox.add_child(label)

	button = Button.new()
	button.text = "Test GenServer Call"
	button.modulate = Color.CYAN
	button.connect("pressed", Callable(self, "_test_genserver_call"))
	vbox.add_child(button)

	print("🖥️ UI setup complete")

func _notification(what):
	if what == NOTIFICATION_EXIT_TREE:
		if zenoh_peer:
			print("🧹 Cleaning up Zenoh connection")
			zenoh_peer.close()
