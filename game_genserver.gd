class_name GameGenServer
extends RefCounted

var current_state = "waiting_election"
var state_data = {
	"board": ["","","","","","","","",""],
	"current_player": "X",
	"game_over": false,
	"winner": "",
	"moves_made": 0
}

func init(initial_data = {}):
	state_data.merge(initial_data, true)
	return ["ok", self]

func handle_call(message, from, state_data):
	match message.action:
		"make_move":
			var result = _try_make_move(message.position, message.player, state_data)
			return ["reply", result, current_state, state_data]
		"get_board":
			return ["reply", state_data.board, current_state, state_data]
		"reset_game":
			state_data = _reset_game_state()
			return ["reply", "reset", "active", state_data]
	return ["reply", "unhandled", current_state, state_data]

func handle_cast(message, state_data):
	match message.type:
		"election_completed":
			state_data["my_symbol"] = "X" if message.i_am_leader else "O"
			return ["noreply", "active", state_data]
		"game_end":
			state_data.game_over = true
			state_data.winner = message.winner
			return ["noreply", "finished", state_data]
	return ["noreply", current_state, state_data]

func handle_info(message, state_data):
	match message.type:
		"hlc_timeout":
			# Handle HLC-based turn timeouts to prevent deadlocks
			if current_state == "active" and not state_data.game_over:
				_force_next_turn(state_data)
				return ["noreply", "active", state_data]
	return ["noreply", current_state, state_data]

func _try_make_move(position, player, state_data):
	if position < 0 or position >= 9 or state_data.board[position] != "" or state_data.game_over:
		return {"success": false, "reason": "invalid_move"}

	if state_data.current_player != player:
		return {"success": false, "reason": "wrong_turn"}

	state_data.board[position] = player
	state_data.moves_made += 1

	var winner = _check_winner(state_data.board)
	if winner != "":
		state_data.game_over = true
		state_data.winner = winner
		send_event("game_end", {"winner": winner})
		return {"success": true, "game_end": true, "winner": winner}
	elif state_data.moves_made >= 9:
		state_data.game_over = true
		state_data.winner = "DRAW"
		send_event("game_end", {"winner": "DRAW"})
		return {"success": true, "game_end": true, "winner": "DRAW"}

	state_data.current_player = "O" if state_data.current_player == "X" else "X"
	return {"success": true, "next_player": state_data.current_player}

func _check_winner(board):
	var lines = [
		[0,1,2], [3,4,5], [6,7,8], # rows
		[0,3,6], [1,4,7], [2,5,8], # columns
		[0,4,8], [2,4,6] # diagonals
	]

	for line in lines:
		if board[line[0]] != "" and board[line[0]] == board[line[1]] and board[line[1]] == board[line[2]]:
			return board[line[0]]

	if board.count("") == 0:
		return "DRAW"

	return ""

func _reset_game_state():
	return {
		"board": ["","","","","","","","",""],
		"current_player": "X",
		"game_over": false,
		"winner": "",
		"moves_made": 0
	}

func _force_next_turn(state_data):
	# Emergency function to break deadlocks - force turn progression
	print("HLC timeout - forcing turn to next player")
	state_data.current_player = "O" if state_data.current_player == "X" else "X"

func send_event(event_type, event_data = {}):
	var message = event_data.duplicate()
	message["type"] = event_type
	handle_cast(message, state_data)

func get_state():
	return current_state

func get_data():
	return state_data.duplicate()
