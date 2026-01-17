class_name CoordinatorStateMachine
extends RefCounted

var current_state = "inactive"
var state_data = {}

func init(initial_data = {}):
	state_data = initial_data.duplicate()
	return ["ok", self]

func handle_call(message, from, state_data):
	match message:
		"get_state":
			return ["reply", current_state, current_state, state_data]
		"get_leader":
			return ["reply", state_data.get("leader", "none"), current_state, state_data]
	return ["reply", "unhandled", current_state, state_data]

func handle_cast(message, state_data):
	match message.type:
		"start":
			return ["noreply", "active", state_data]
		"elect_leader":
			var leader_id = randi() % 100 + 1
			state_data["leader"] = leader_id
			return ["noreply", "coordinated", state_data]
		"stop":
			return ["noreply", "inactive", state_data]
	return ["noreply", current_state, state_data]

func handle_info(message, state_data):
	match message.type:
		"heartbeat":
			return ["noreply", "active", state_data]
	return ["noreply", current_state, state_data]

func get_state():
	return current_state
