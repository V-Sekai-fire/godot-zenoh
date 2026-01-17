class_name ConnectionGenServer
extends RefCounted

var current_state = "disconnected"
var state_data = {}

func init(initial_data = {}):
	state_data = initial_data.duplicate()
	return ["ok", self]

func handle_call(message, from, state_data):
	match message:
		"get_status":
			return ["reply", current_state, current_state, state_data]
		"force_disconnect":
			return ["reply", "ok", "disconnected", {}]
	return ["reply", "unhandled", current_state, state_data]

func handle_cast(message, state_data):
	match message.type:
		"connect":
			match message.mode:
				"server":
					return ["noreply", "connecting_server", state_data]
				"client":
					return ["noreply", "connecting_client", state_data]
		"connection_success":
			return ["noreply", "connected", state_data]
		"connection_failed":
			state_data["error"] = message.error
			return ["noreply", "failed", state_data]
	return ["noreply", current_state, state_data]

func handle_info(message, state_data):
	match message.type:
		"timeout":
			if current_state.begins_with("connecting"):
				print("Connection timeout - retrying...")
				# Could implement retry logic here
				return ["noreply", "failed", state_data]
	return ["noreply", current_state, state_data]

func send_event(event_type, event_data = {}):
	var message = event_data.duplicate()
	message["type"] = event_type
	handle_cast(message, state_data)
