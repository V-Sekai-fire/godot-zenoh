extends Node

const ElectionGenServer = preload("res://election_genserver.gd")
const ConnectionGenServer = preload("res://connection_genserver.gd")
const GameGenServer = preload("res://game_genserver.gd")

var zenoh_peer: ZenohMultiplayerPeer
var election_server: ElectionGenServer
var connection_server: ConnectionGenServer

var label: Label
var button: Button

func _ready():
	election_server = ElectionGenServer.new()
	connection_server = ConnectionGenServer.new()

	var election_init = election_server.init({"my_id": 123})
	var connection_init = connection_server.init({})

	if election_init[0] != "ok" or connection_init[0] != "ok":
		push_error("GenServer initialization failed")
		return

	zenoh_peer = ZenohMultiplayerPeer.new()
	zenoh_peer.game_id = "genserver_demo"

	var server_result = zenoh_peer.create_server(7447, 32)
	if server_result != 0:
		var client_result = zenoh_peer.create_client("localhost", 7447)
		if client_result != 0:
			push_error("Auto-connect failed")

	setup_ui()

func _process(delta):
	if zenoh_peer:
		zenoh_peer.poll()

func setup_ui():
	var vbox = VBoxContainer.new()
	add_child(vbox)
	vbox.set_anchors_preset(Control.PRESET_FULL_RECT)

	label = Label.new()
	label.text = "Ready"
	vbox.add_child(label)

	button = Button.new()
	button.text = "Test"
	button.connect("pressed", Callable(self, "_test_genserver_call"))
	vbox.add_child(button)

func _test_genserver_call():
	if not connection_server:
		return
	var result = connection_server.handle_call("get_status", self, {})
	if result[0] == "reply":
		label.text = "Status: " + str(result[1])
	else:
		push_error("GenServer call failed")

func _notification(what):
	if what == NOTIFICATION_EXIT_TREE and zenoh_peer:
		zenoh_peer.close()
