class_name ElectionGenServer
extends RefCounted

var current_state = "disconnected"
var state_data = {}

func init(initial_data = {}):
	state_data = initial_data.duplicate()
	return ["ok", self]

func handle_call(message, from, state_data):
	match current_state:
		"connected":
			if message == "start_election":
				return ["reply", "election_started", "collecting_peers", state_data]
		"collecting_peers":
			if message == "get_peer_count":
				return ["reply", state_data.get("peer_count", 0), "collecting_peers", state_data]
		"deciding_leader":
			if message.has("decide_winner"):
				var winner = _decide_election_winner(message.peers)
				state_data["leader"] = winner
				return ["reply", winner, "victory_broadcasting", state_data]
	return ["reply", "unhandled", current_state, state_data]

func handle_cast(message, state_data):
	match message.type:
		"peer_joined":
			var peers = state_data.get("peers", [])
			peers.append(message.peer_id)
			state_data["peers"] = peers
			state_data["peer_count"] = peers.size()
			return ["noreply", "collecting_peers", state_data]
		"victory_ack":
			var acks = state_data.get("victory_acks", 0) + 1
			state_data["victory_acks"] = acks
			if acks >= state_data.get("expected_acks", 0):
				return ["noreply", "finalized", state_data]
			return ["noreply", "victory_broadcasting", state_data]
	return ["noreply", current_state, state_data]

func handle_info(message, state_data):
	match message.type:
		"zenoh_connected":
			return ["noreply", "connected", state_data]
		"timeout":
			match current_state:
				"waiting_connections":
					state_data["leader"] = state_data.get("my_id", 1)
					return ["noreply", "finalized", state_data]
				"victory_broadcasting":
					return ["noreply", "finalized", state_data]
	return ["noreply", current_state, state_data]

func send_event(event_type, event_data = {}):
	# Public API for sending events to the state machine
	var message = event_data.duplicate()
	message["type"] = event_type
	handle_cast(message, state_data)

func get_state():
	return current_state

func get_data():
	return state_data.duplicate()

func _decide_election_winner(peers):
	# Lowest ID wins (deterministic bully algorithm)
	if peers.is_empty():
		return state_data.get("my_id", 1)
	var sorted_peers = peers.duplicate()
	sorted_peers.sort()
	return sorted_peers[0]
