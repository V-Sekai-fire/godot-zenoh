extends Node

const CoordinatorStateMachine = preload("res://godot_zenoh/core/election_genserver.gd")
const ConnectionStateMachine = preload("res://godot_zenoh/core/connection_genserver.gd")
const NetworkingStateMachine = preload("res://godot_zenoh/core/game_genserver.gd")

var zenoh_peer: ZenohMultiplayerPeer
var coordinator: CoordinatorStateMachine
var connection: ConnectionStateMachine
var networking: NetworkingStateMachine
var timer: Timer

var label: Label
var peer_id: String
var connected: bool = false

func _ready():
	coordinator = CoordinatorStateMachine.new()
	connection = ConnectionStateMachine.new()
	networking = NetworkingStateMachine.new()

	var c_init = coordinator.init({"id": 1})
	var conn_init = connection.init({})
	var net_init = networking.init({})

	if c_init[0] != "ok" or conn_init[0] != "ok" or net_init[0] != "ok":
		if label: label.text = "Init failed"
		return

	setup_ui()
	_connect_to_network()

	timer = Timer.new()
	timer.wait_time = 3.0
	timer.one_shot = false
	timer.connect("timeout", Callable(self, "_send_test_message"))
	add_child(timer)
	timer.start()

func _connect_to_network():
	zenoh_peer = ZenohMultiplayerPeer.new()
	zenoh_peer.game_id = "godot_zenoh_test"

	var rng = RandomNumberGenerator.new()
	rng.randomize()
	peer_id = "Peer_" + str(rng.randi_range(1000, 9999))

	var client_result = zenoh_peer.create_client("localhost", 7447)
	if client_result == 0:
		label.text = "✅ Connected as CLIENT\nPeer: " + peer_id
		connected = true
	else:
		var server_result = zenoh_peer.create_server(7447, 32)
		if server_result == 0:
			label.text = "✅ Started as SERVER\nPeer: " + peer_id
			connected = true
		else:
			label.text = "❌ Connection failed"

func _process(delta):
	if zenoh_peer:
		zenoh_peer.poll()
		var packet_count = zenoh_peer.get_available_packet_count()
		while packet_count > 0:
			var packet = zenoh_peer.get_packet()
			var msg = packet.get_string_from_utf8()
			# Silently acknowledge received messages (remove verbose logging)
			packet_count -= 1

func _send_test_message():
	if connected and zenoh_peer.get_connection_status() == 2:
		var message = peer_id + ":" + str(Time.get_ticks_msec())
		var data = PackedByteArray()
		data.append_array(message.to_utf8_buffer())

		zenoh_peer.set_transfer_channel(1)
		zenoh_peer.put_packet(data)
		# Remove verbose sent message logging

func _notification(what):
	if what == NOTIFICATION_EXIT_TREE:
		if timer: timer.stop()
		if zenoh_peer: zenoh_peer.close()

func setup_ui():
	var vbox = VBoxContainer.new()
	add_child(vbox)
	vbox.set_anchors_preset(Control.PRESET_FULL_RECT)

	label = Label.new()
	label.text = "Connecting..."
	label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	vbox.add_child(label)

	var desc = Label.new()
	desc.text = "Auto-connects peers via Zenoh network.\nCI/CD compatible, silent operation."
	desc.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	vbox.add_child(desc)
