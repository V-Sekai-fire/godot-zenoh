class_name NetworkingStateMachine
extends RefCounted

var current_state = "disconnected"
var state_data = {}

func init(initial_data = {}):
	state_data = initial_data.duplicate()
	return ["ok", self]

func handle_call(message, from, state_data):
	match message:
		"get_state":
			return ["reply", current_state, current_state, state_data]
		"get_status":
			return ["reply", state_data.get("status", "unknown"), current_state, state_data]
	return ["reply", "unhandled", current_state, state_data]

func handle_cast(message, state_data):
	match message.type:
		"connect":
			return ["noreply", "connecting", state_data]
		"connection_success":
			state_data["status"] = "connected"
			return ["noreply", "connected", state_data]
		"connection_failed":
			state_data["status"] = "failed"
			return ["noreply", "error", state_data]
		"disconnect":
			return ["noreply", "disconnected", state_data]
	return ["noreply", current_state, state_data]

func handle_info(message, state_data):
	match message.type:
		"timeout":
			if current_state == "connecting":
				return ["noreply", "error", state_data]
	return ["noreply", current_state, state_data]

func send_event(event_type, event_data = {}):
	var message = event_data.duplicate()
	message["type"] = event_type
	handle_cast(message, state_data)

func get_state():
	return current_state

func get_data():
	return state_data.duplicate()
