extends Node

# Auto-connecting Zenoh networking with minimal state machines
const CoordinatorStateMachine = preload("res://godot_zenoh/core/election_genserver.gd")
const ConnectionStateMachine = preload("res://godot_zenoh/core/connection_genserver.gd")
const NetworkingStateMachine = preload("res://godot_zenoh/core/game_genserver.gd")

var zenoh_peer: ZenohMultiplayerPeer
var coordinator: CoordinatorStateMachine
var connection: ConnectionStateMachine
var networking: NetworkingStateMachine

var label: Label
var button: Button

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
		label.text = "❌ State Machine Init Failed"

	setup_ui()
	_start_auto_connection()

func _start_auto_connection():
	# Initialize Zenoh network auto-connection
	zenoh_peer = ZenohMultiplayerPeer.new()
	zenoh_peer.game_id = "godot_zenoh_state_machine_test"

	# Try client connection first (connects to existing server), fall back to server (becomes router)
	var client_result = zenoh_peer.create_client("localhost", 7447)
	if client_result == 0:
		label.text = "✅ Auto-connected as FOLLOWER/CLIENT\nCoordinator: " + coordinator.get_state() + "\nConnection: " + connection.get_state()
	else:
		# No server available, become the server (router)
		var server_result = zenoh_peer.create_server(7447, 32)
		if server_result == 0:
			label.text = "✅ Auto-connected as LEADER/SERVER\nCoordinator: " + coordinator.get_state() + "\nConnection: " + connection.get_state()
		else:
			label.text = "❌ Auto-connection failed - cannot create server or connect as client"

func _process(delta):
	if zenoh_peer:
		zenoh_peer.poll()

func setup_ui():
	var vbox = VBoxContainer.new()
	add_child(vbox)
	vbox.set_anchors_preset(Control.PRESET_FULL_RECT)

	label = Label.new()
	label.text = "State Machines Ready"
	vbox.add_child(label)

	button = Button.new()
	button.text = "Test State Machines"
	button.connect("pressed", Callable(self, "_test_state_machines"))
	vbox.add_child(button)

func _test_state_machines():
	var status = "State Machines:\n"
	status += "Coordinator: " + coordinator.get_state() + "\n"
	status += "Connection: " + connection.get_state() + "\n"
	status += "Networking: " + networking.get_state() + "\n"

	# Test state transitions
	connection.send_event("connect")
	status += "After connect: " + connection.get_state() + "\n"

	coordinator.send_event("start")
	status += "Coordinator after start: " + coordinator.get_state() + "\n"

	label.text = status
