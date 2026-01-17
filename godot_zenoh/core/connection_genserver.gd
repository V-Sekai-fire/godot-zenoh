class_name ConnectionStateMachine
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
		"ping":
			return ["reply", "pong", current_state, state_data]
	return ["reply", "unhandled", current_state, state_data]

func handle_cast(message, state_data):
	match message.type:
		"connect":
			return ["noreply", "connecting", state_data]
		"disconnect":
			return ["noreply", "disconnected", state_data]
	return ["noreply", current_state, state_data]

func handle_info(message, state_data):
	match message.type:
		"timeout":
			if current_state == "connecting":
				return ["noreply", "error", state_data]
	return ["noreply", current_state, state_data]

func get_state():
	return current_state
