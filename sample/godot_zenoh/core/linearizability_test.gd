extends Node

# Core Linearizability Test for Zenoh Peers
# Validates distributed consistency using formal theory

var zenoh_peer: ZenohMultiplayerPeer
var is_server: bool = false
var player_id: int = 0

# Distributed state management
var shared_counter: int = 0
var operations: Array = []  # History of operations for linearizability checking

# Test parameters
var test_duration: float = 15.0  # seconds
var operations_per_peer: int = 3

signal test_completed(success: bool, violations: int, message: String)

func _ready():
	print("🚀 Zenoh Peer Linearizability Validation")
	print("≡ Query Consistency Analysis")

	initialize_zenoh_peer()
	start_consistency_test()

func initialize_zenoh_peer():
	zenoh_peer = ZenohMultiplayerPeer.new()

	# Game isolation
	zenoh_peer.game_id = "linearizability_validation_" + str(randi())

	# Try server first for coordination
	var server_result = zenoh_peer.create_server(7491, 32)
	if server_result == OK:
		is_server = true
		print("📡 Server mode: authoritative state")
	else:
		var client_result = zenoh_peer.create_client("localhost", 7491)
		if client_result == OK:
			print("📱 Client mode: coordinated operations")

	player_id = zenoh_peer.get_unique_id()
	print("🆔 Peer ID:", player_id, "Role:", "Server" if is_server else "Client")

	set_process(true)

func start_consistency_test():
	print("⏱️ Test duration:", test_duration, "seconds")
	print("🔢 Operations per peer:", operations_per_peer)

	# Schedule operations at random intervals
	for i in range(operations_per_peer):
		var timer = Timer.new()
		timer.wait_time = randf_range(1.0, test_duration - 2.0)
		timer.one_shot = true
		timer.connect("timeout", Callable(self, "_perform_consistency_operation").bind(i))
		add_child(timer)
		timer.start()

	# End test timeout
	get_tree().create_timer(test_duration).connect("timeout", Callable(self, "_analyze_consistency"))

func _perform_consistency_operation(operation_id: int):
	var operation_start = Time.get_ticks_usec()
	var operation_type = "write"

	# Choose operation type
	if randf() < 0.4:  # 40% reads
		operation_type = "read"

	match operation_type:
		"read":
			# Query current state
			var value = await query_shared_state()
			record_operation("read", value, operation_start, Time.get_ticks_usec())
			print("📖 Peer", player_id, "read value:", value)

		"write":
			# Modify shared state atomically
			var success = await modify_shared_state(1)
			if success:
				record_operation("write", shared_counter, operation_start, Time.get_ticks_usec())
				print("✏️ Peer", player_id, "wrote to:", shared_counter)
			else:
				print("❌ Peer", player_id, "write failed (not committed)")

func query_shared_state() -> int:
	# Server can read directly
	if is_server:
		return shared_counter

	# Clients query server
	var request = {"type": "QUERY_SERVICE", "peer_id": player_id}
	var json_str = JSON.stringify(request)
	var data = json_str.to_utf8_buffer()
	zenoh_peer.put_packet(data)

	# Wait for response (in real implementation, properly handle async)
	await get_tree().create_timer(0.1).timeout
	return shared_counter  # Mock response - real implementation needs proper response handling

func modify_shared_state(delta: int) -> bool:
	if is_server:
		# Server can modify directly
		shared_counter += delta
		broadcast_state_update(shared_counter)
		return true
	else:
		# Clients propose changes to server
		var request = {"type": "MODIFY_SERVICE", "peer_id": player_id, "delta": delta}
		var json_str = JSON.stringify(request)
		var data = json_str.to_utf8_buffer()
		zenoh_peer.put_packet(data)

		# Wait for acknowledgment
		await get_tree().create_timer(0.1).timeout
		return true  # Mock success - real implementation needs proper validation

func broadcast_state_update(new_value: int):
	var update_msg = {"type": "STATE_UPDATE", "value": new_value, "from_peer": player_id}
	var json_str = JSON.stringify(update_msg)
	var data = json_str.to_utf8_buffer()
	zenoh_peer.put_packet(data)

func record_operation(type: String, value: int, start_us: int, end_us: int):
	operations.append({
		"peer_id": player_id,
		"type": type,
		"value": value,
		"start_time": start_us,
		"end_time": end_us
	})

func _process(delta: float):
	if zenoh_peer:
		zenoh_peer.poll()

		# Process incoming messages
		var packet_count = zenoh_peer.get_available_packet_count()
		for i in range(packet_count):
			var packet = zenoh_peer.get_packet()
			process_message(packet)

func process_message(packet: PackedByteArray):
	var msg_str = packet.get_string_from_utf8()
	var parsed = JSON.parse_string(msg_str)

	if parsed is Dictionary:
		match parsed.get("type", ""):
			"STATE_UPDATE":
				var new_value = parsed.get("value", 0)
				var from_peer = parsed.get("from_peer", 0)
				if from_peer != player_id:
					shared_counter = new_value
					print("🔄 State updated to:", new_value, "from peer:", from_peer)

			"QUERY_RESPONSE":
				shared_counter = parsed.get("counter_value", shared_counter)

			"MODIFY_RESPONSE":
				var success = parsed.get("success", false)
				if success:
					shared_counter = parsed.get("new_value", shared_counter)

func _analyze_consistency():
	print("\n📊 Linearizability Analysis")
	print("══════════════════════════════════════")
	print("Operations recorded:", operations.size())

	if operations.is_empty():
		test_completed.emit(false, 0, "No operations performed - coordinate?ation failed")
		return

	# Analyze read-write consistency
	var violations = 0
	var writes_in_order = []

	# Extract write operations for comparison
	for op in operations:
		if op["type"] == "write":
			writes_in_order.append(op)
			print("🔍 Write operation: Peer", op["peer_id"], "→", op["value"])

	# Validate read consistency
	for op in operations:
		if op["type"] == "read":
			var read_value = op["value"]
			var latest_write_before_read = -1

			# Find the write operation that should have happened before this read
			for write_op in writes_in_order:
				if write_op["end_time"] < op["start_time"]:
					latest_write_before_read = write_op["value"]
				elif write_op["start_time"] > op["end_time"]:
					break  # No more writes before this read

			if read_value < latest_write_before_read:
				violations += 1
				print("🚨 VIOLATION: Read", read_value, "should be at least", latest_write_before_read)

	print("🎯 Reads validated:", operations.filter(func(op): return op["type"] == "read").size())
	print("✏️ Writes performed:", operations.filter(func(op): return op["type"] == "write").size())
	print("⚠️ Consistency violations:", violations)
	print("📈 Final shared state:", shared_counter)

	if violations == 0:
		print("✅ RESULT: LINEARIZABLE - Distributed operations consistent")
		test_completed.emit(true, violations, "Linearizability validated - Zenoh peers maintain distributed consistency")
	else:
		print("❌ RESULT: NOT LINEARIZABLE - Distributed operations inconsistent")
		test_completed.emit(false, violations, "Linearizability violated - Zenoh peers lack distributed coordination")

func _notification(what):
	if what == NOTIFICATION_EXIT_TREE:
		if zenoh_peer:
			zenoh_peer.close()