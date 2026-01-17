# Ping pong countdown test between two Godot instances
extends Node

var zenoh_peer: ZenohMultiplayerPeer

# LEGACY STATE MACHINE VARIABLES (for backward compatibility)
var election_state: int = 0  # Will map to new state machines
var election_timer: Timer
var leader_election_phase: bool = false
var known_peers = []
var my_election_id: int = -1
var current_leader_id: int = -1
var election_message_queue = []
var collected_peer_ids = []
var victory_acknowledgments = 0
var expected_acknowledgments = 0

var my_id: int = -1
var is_host: bool = false

# GenServer Instances
var election_server = ElectionGenServer.new()
var connection_server = ConnectionGenServer.new()
var game_server = GameGenServer.new()

# 🔥 DISTRIBUTED TIC-TAC-TOE: Concurrency Demo Game
var game_mode: int = 1  # 0 = Countdown, 1 = TicTacToe Demo

# Tic-Tac-Toe Game State
var board = ["","","","","","","","",""]  # 3x3 board: ""=empty, "X", "O"
var current_player: String = "X"         # "X" or "O"
var game_over: bool = false
var winner: String = ""                  # "X", "O", or "DRAW"
var my_symbol: String = ""               # Assigned during game start
var moves_made: int = 0                  # Total moves played

# HLC-based turn coordination to prevent race conditions
var last_x_move_hlc_timestamp = 0        # HLC timestamp of last X move received
var hlc_turn_threshold = 50000           # 50ms buffer for network/HLC timing delays

# Legacy variables for disabled countdown code (prevent parser errors)
var countdown_number: int = 10
var last_received_count: int = -1

var button: Button
var label: Label
var host_button: Button
var join_button: Button

var countdown_timer: Timer

# ELIXIR GENSERVER-STYLE STATE MACHINES
# Each state machine has: init(), handle_call(), handle_cast(), handle_info()

# Election GenServer State Machine
class_name ElectionGenServer
var current_state = "disconnected"
var state_data = {}

func init(initial_data = {}):
	# Return {ok, state}
	state_data = initial_data.duplicate()
	return ["ok", self]

func handle_call(message, from, state_data):
	# Synchronous calls - {reply, response, new_state, new_data}
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
	# Asynchronous casts - {noreply, new_state, new_data}
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
	# System messages and timeouts - {noreply, new_state, new_data}
	match message.type:
		"zenoh_connected":
			return ["noreply", "connected", state_data]
		"timeout":
			match current_state:
				"waiting_connections":
					print("Election timeout - forcing single participant mode")
					state_data["leader"] = state_data.get("my_id", 1)
					return ["noreply", "finalized", state_data]
				"victory_broadcasting":
					print("Victory ack timeout - proceeding anyway")
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

# Connection GenServer State Machine
class_name ConnectionGenServer
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

# Game GenServer State Machine
class_name GameGenServer
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

# RAFT CONSENSUS STATE (reverted - not implemented)
# var raft_consensus: ZenohRaftConsensus = null

# Connection state machine constants (integer enum)
const STATE_DISCONNECTED = 0
const STATE_CONNECTING = 1
const STATE_CONNECTED = 2
const STATE_FAILED = 3
const STATE_SERVER_READY = 4
const STATE_CLIENT_ATTEMPTING = 5
const STATE_ZENOH_SESSION_FAILED = 6
const STATE_LEADER_ELECTION = 7  # New state for automatic leader election

# Connection state machine variables
var connection_state: int = STATE_DISCONNECTED

# CLEANED: Removed Merkle hash state tracking for simpler implementation

func _ready():
	print("Pong Test Starting...")

	# Check command line arguments for automatic mode
	var args = OS.get_cmdline_args()
	var is_server = args.has("--server")
	var is_client = args.has("--client")

	# Always run in automatic leader election mode to demonstrate networking
	print("🔄 Godot-Zenoh: Automatic Leader Election Mode")

	# Initialize zenoh peer for all modes
	zenoh_peer = ZenohMultiplayerPeer.new()
	zenoh_peer.game_id = "pong_test"

	# Check command line arguments for specialized testing
	if is_server or is_client:
		# Specialized manual testing modes
		if is_server:
			print("🖥️ Manual server mode requested")
			_on_host_pressed()
		else:
			print("👨‍💻 Manual client mode requested")
			_on_join_pressed()
	else:
		# Default: Automatic leader election for all instances
		print("🎯 Running automatic leader election for all instances")
		start_leader_election()

	# Create UI in all cases for status display
	setup_ui()

func setup_ui():
	# Create UI for testing
	var vbox = VBoxContainer.new()
	add_child(vbox)
	vbox.set_anchors_preset(Control.PRESET_FULL_RECT)

	# Title
	var title = Label.new()
	title.text = "Godot-Zenoh Multiplayer Test"
	title.modulate = Color.GREEN
	vbox.add_child(title)

	# Features info
	var info = Label.new()
	info.text = "Features: Leader Election • HOL Blocking • Zero-Timer Architecture"
	info.modulate = Color.LIGHT_BLUE
	vbox.add_child(info)

	# Connection buttons
	host_button = Button.new()
	host_button.text = "Host Game (Server)"
	host_button.connect("pressed", Callable(self, "_on_host_pressed"))
	vbox.add_child(host_button)

	join_button = Button.new()
	join_button.text = "Join Game (Client)"
	join_button.connect("pressed", Callable(self, "_on_join_pressed"))
	vbox.add_child(join_button)

	# Auto-election info
	var auto_label = Label.new()
	auto_label.text = "Auto: Multiple instances elect leader with lowest peer ID"
	vbox.add_child(auto_label)

	# Status label
	label = Label.new()
	label.text = "Status: Initializing..."
	label.modulate = Color.YELLOW
	vbox.add_child(label)

	# Peer info label
	var peer_label = Label.new()
	peer_label.name = "peer_info"
	peer_label.text = "Peer ID: Not connected | Role: Unknown | State: " + get_state_text(connection_state)
	vbox.add_child(peer_label)

	# Send button - now for Tic-Tac-Toe moves
	button = Button.new()
	button.text = "Make Tic-Tac-Toe Move"
	button.disabled = true
	button.connect("pressed", Callable(self, "_on_make_move"))
	vbox.add_child(button)

	# Instructions label
	var instructions = Label.new()
	instructions.text = "🎮 Tic-Tac-Toe: Leader is X, others are O\nWait for election to complete, then make moves in turn order"
	instructions.modulate = Color.ORANGE
	vbox.add_child(instructions)

	# HLC Timestamp Request Button
	var hlc_button = Button.new()
	hlc_button.text = "🎯 Request Zenoh HLC Timestamp"
	hlc_button.modulate = Color.CYAN
	hlc_button.connect("pressed", Callable(self, "_on_hlc_request_pressed"))
	vbox.add_child(hlc_button)

func _on_host_pressed():
	print("Starting as host...")
	is_host = true

	# STATE MACHINE: Prevent multiple hosts in the same session
	if connection_state != STATE_DISCONNECTED:
		if label:
			label.text = "ALREADY connected! Disconnect first (State: " + str(connection_state) + ")"
		print("Already connected - cannot start another host session")
		return

	# STATE MACHINE: Set connecting state before attempting connection
	connection_state = STATE_CONNECTING

	# Start server
	var result = zenoh_peer.create_server(7447, 32)
	if result == 0:
		var client_id = zenoh_peer.get_unique_id()
		if label:
			label.text = "Hosting game - Player ID: " + str(client_id)
		print("Server Player ID: " + str(client_id))

		# STATE MACHINE: Successfully hosting server
		connection_state = STATE_SERVER_READY
		setup_networking()
	else:
		# STATE MACHINE: Server creation failed
		connection_state = STATE_DISCONNECTED
		if label:
			label.text = "Failed to host: " + str(result)

func _on_join_pressed():
	print("Joining as client...")
	is_host = false

	# STATE MACHINE: Check if already connected
	if zenoh_peer.connection_status() == 2:  # Already connected?
		if label:
			label.text = "ALREADY connected! Disconnect first"
		print("Already connected - cannot join as another client")
		return

	# No blocking delays - server readiness is handled by state machine
	connection_state = STATE_CLIENT_ATTEMPTING

	# Join server immediately
	var result = zenoh_peer.create_client("localhost", 7447)
	if result == 0:
		print("Client connection initiated - status: CONNECTING")
		# Connection events handled by poll() state machine callbacks
		# No polling loops or await blocks in GDscript
		if label:
			label.text = "Client connection in progress..."
	else:
		# STATE MACHINE: Complete failure
		connection_state = STATE_FAILED
		if label:
			label.text = "Failed to join: " + str(result)
		print("❌ Client create_client failed with error: " + str(result))

func setup_networking():
	print("Networking setup complete")

	# Start ping pong countdown after a brief delay
	var start_timer = Timer.new()
	start_timer.one_shot = true
	start_timer.wait_time = 2.0  # Wait 2 seconds after connecting
	start_timer.connect("timeout", Callable(self, "_on_ping_pong_start"))
	add_child(start_timer)
	start_timer.start()

	# Set up polling timer
	var timer = Timer.new()
	timer.autostart = true
	timer.wait_time = 0.1  # Poll every 100ms
	timer.connect("timeout", Callable(self, "_on_poll_timeout"))
	add_child(timer)

	# Set up countdown timer (2 second intervals)
	countdown_timer = Timer.new()
	countdown_timer.autostart = false
	countdown_timer.one_shot = true
	countdown_timer.wait_time = 2.0
	countdown_timer.connect("timeout", Callable(self, "_on_countdown_tick"))
	add_child(countdown_timer)

func _on_ping_pong_start():
	if is_host:
		# Host starts the countdown (legacy mode disabled)
		print("Legacy ping pong countdown disabled - using Tic-Tac-Toe mode")
		if label:
			label.text = "Election complete - ready for Tic-Tac-Toe..."
	else:
		# Client waits (legacy mode disabled)
		if label:
			label.text = "Waiting in Tic-Tac-Toe game mode..."

func _on_send_pressed():
	# Send countdown number
	var message = "COUNT:" + str(countdown_number)
	var data = PackedByteArray()
	data.append_array(message.to_utf8_buffer())

	zenoh_peer.put_packet(data, 1)
	print("Sent: " + message)

	# Decrement for next send
	if countdown_number > 0:
		countdown_number -= 1
		button.text = "Send " + str(countdown_number) + " to Other Player"

func setup_tic_tac_toe_networking():
	print("🎮 Tic-Tac-Toe networking setup complete")

	# Initialize Tic-Tac-Toe game state
	reset_tic_tac_toe_game()

	# Start polling for game messages
	var timer = Timer.new()
	timer.autostart = true
	timer.wait_time = 0.1  # Poll every 100ms
	timer.connect("timeout", Callable(self, "_on_tic_tac_toe_poll"))
	add_child(timer)

	# Leader starts the game by announcing X goes first
	if is_host:
		_announce_game_start()

func _announce_game_start():
	print("🎯 Announcing Tic-Tac-Toe game start - X goes first")
	var start_msg = "GAME_START:" + str(current_leader_id) + ":LEADER_IS_X:FOLLOWERS_ARE_O"
	var data = PackedByteArray()
	data.append_array(start_msg.to_utf8_buffer())
	zenoh_peer.put_packet(data, 1)

	print("🎮 LEADER MAKES FIRST MOVE IMMEDIATELY")
	# 🔥 CRITICAL FIX: Leader makes first X move immediately after announcing game!
	call_deferred("_make_first_move_immediately")

func reset_tic_tac_toe_game():
	# Reset game state
	board = ["","","","","","","","",""]
	current_player = "X"
	game_over = false
	winner = ""
	moves_made = 0
	print("🔄 Tic-Tac-Toe game reset")
	print_board()

func _on_tic_tac_toe_poll():
	zenoh_peer.poll()

	# Handle peer assignments based on ELECTION results, not connection type
	if my_symbol == "" or my_symbol == "WAITING":
		if my_election_id == current_leader_id:
			my_symbol = "X"  # I won the election, I start as X
			if label:
				label.text = "LEADER: I am X - starting the game"
		else:
			my_symbol = "O"  # I lost the election, I am O
			if label:
				label.text = "FOLLOWER: I am O - waiting for X to move"

	# Process game messages - CHECK SUBSCRIPTION ISSUES
	var packet_count = zenoh_peer.get_available_packet_count(1)
	if packet_count > 0:
		print("📨 TIC-TAC-TOE POLL: " + str(packet_count) + " packets available for processing")
		while zenoh_peer.get_available_packet_count(1) > 0:
			var data = zenoh_peer.get_packet(1)
			var msg = data.get_string_from_utf8()
			print("🎮 Processing Tic-Tac-Toe message: '" + msg.left(50) + "...'")
			_process_tic_tac_toe_message(msg)
	else:
		# Debug: Show we're running but no messages
		print("🔄 TIC-TAC-TOE POLL: No packets available (normal if not my turn)")

func _process_tic_tac_toe_message(msg: String):
	if msg.begins_with("GAME_START:"):
		var parts = msg.split(":")
		print("🎯 Game start received! My symbol is: " + my_symbol)
		print_board()

	elif msg.begins_with("GAME_MOVE:"):
		var parts = msg.split(":")
		if parts.size() >= 4:
			var move_player = parts[1]
			var move_position = int(parts[2])
			var from_id = parts[3]

			print("📝 Received move from " + str(from_id) + ": " + move_player + " at position " + str(move_position))

			# HLC-based turn coordination: Track X move timestamps for timing validation
			if move_player == "X":
				last_x_move_hlc_timestamp = get_current_hlc_timestamp()
				print("⏱️  Captured X move HLC timestamp: " + str(last_x_move_hlc_timestamp))

			# Apply the move and update game state
			if _apply_game_move(move_player, move_position):
				print("✅ Move applied successfully")
				print_board()

				# Check for game end
				if game_over:
					_handle_game_end()
				else:
					# Switch turns
					current_player = "O" if current_player == "X" else "X"
					print("Next turn: " + current_player)

					if label:
						label.text = "TURN: " + current_player + " to move"

					# 🔥 FORCE AUTO-PLAY FOR NEXT PLAYER! (call every time to ensure O moves)
					print("🔥 DEBUG: About to call auto-move check - ensuring O plays!")
					_check_and_make_auto_move()
					print("🔥 DEBUG: Auto-move check completed")

func _apply_game_move(player_symbol: String, position: int) -> bool:
	# Validate move
	if position < 0 or position >= 9:
		print("❌ Invalid position: " + str(position))
		return false
	if board[position] != "":
		print("❌ Position already occupied: " + str(position))
		return false
	if game_over:
		print("❌ Game is over")
		return false
	if current_player != player_symbol:
		print("❌ Wrong turn - expected " + current_player + ", got " + player_symbol)
		return false

	# Apply move
	board[position] = player_symbol
	moves_made += 1
	print("🔄 Applied move: " + player_symbol + " at position " + str(position))

	# Check for winner
	var game_result = check_winner()
	if game_result != "":
		game_over = true
		winner = game_result
		print("🏆 Game Over: " + game_result)

	return true

func check_winner() -> String:
	# Check rows
	for i in range(3):
		if board[i*3] != "" and board[i*3] == board[i*3+1] and board[i*3+1] == board[i*3+2]:
			return board[i*3]

	# Check columns
	for i in range(3):
		if board[i] != "" and board[i] == board[i+3] and board[i+3] == board[i+6]:
			return board[i]

	# Check diagonals
	if board[0] != "" and board[0] == board[4] and board[4] == board[8]:
		return board[0]
	if board[2] != "" and board[2] == board[4] and board[4] == board[6]:
		return board[2]

	# Check for draw
	if moves_made >= 9:
		return "DRAW"

	return ""  # No winner yet

func print_board():
	print("=== TIC-TAC-TOE BOARD (moves: " + str(moves_made) + ", current turn: " + current_player + ") ===")
	print("   |   |   ")
	print(" " + _board_char(board[0]) + " | " + _board_char(board[1]) + " | " + _board_char(board[2]) + " ")
	print("-----------")
	print("   |   |   ")
	print(" " + _board_char(board[3]) + " | " + _board_char(board[4]) + " | " + _board_char(board[5]) + " ")
	print("-----------")
	print("   |   |   ")
	print(" " + _board_char(board[6]) + " | " + _board_char(board[7]) + " | " + _board_char(board[8]) + " ")
	print("   |   |   ")
	print("=====================================")

func _board_char(piece: String) -> String:
	return piece if piece != "" else " "

func _handle_game_end():
	# 🎯 CLEAR GAME END DETERMINATION: Make all endings distinct and visible
	var end_reason = ""
	var winner_symbol = winner.to_upper()
	var loser_symbol = "O" if winner_symbol == "X" else "X"

	if winner == "DRAW":
		print("🏆 TIE GAME: Board is full with no winner")
		end_reason = "TIE - All squares filled, no winner emerges"

	elif winner_symbol == my_symbol:
		print("🎉 VICTORY FOR " + winner_symbol + ": " + ("You" if winner_symbol == my_symbol else "Opponent"))
		if my_symbol == "X":
			end_reason = "X WINS the Tic-Tac-Toe championship!"
		else:
			end_reason = "O WINS against all odds!"

	else:
		print("💔 DEFEAT FOR " + loser_symbol + ": " + winner_symbol + " claims the victory")
		if my_symbol == "X":
			end_reason = "X LOST - O takes the game!"
		else:
			end_reason = "O CONCEEDS - X victorious!"

	# Clear game completion status for all
	print("🏁 FINAL BOARD (winner: " + winner + "):")
	print_board()
	print(("🎯 RESULT: " + end_reason).to_upper())

	if label:
		label.text = "GAME END: " + end_reason

	# Broadcast specific game end type
	var end_msg = ""
	if winner == "DRAW":
		end_msg = "GAME_TIE:BOARD_FULL:NO_WINNER"
	elif winner_symbol == "X":
		end_msg = "GAME_WIN:X_DEFEATS_O:THREE_IN_ROW"
	elif winner_symbol == "O":
		end_msg = "GAME_WIN:O_DEFEATS_X:BLOCK_COMPLETED"
	else:
		end_msg = "GAME_ERROR:INVALID_END_STATE"  # Should never happen

	var data = PackedByteArray()
	data.append_array(end_msg.to_utf8_buffer())
	zenoh_peer.put_packet(data, 1)

	print("📢 Game end broadcasted: " + end_msg)
	print("🏆 Official game result: " + winner_symbol + " victory, " + loser_symbol + " defeat, or honorable tie!")

func _on_make_move():
	# 🔥 DEMO: Make a Tic-Tac-Toe move (leader coordinates the game)
	if game_over:
		print("❌ Game is over - no more moves")
		return

	# Simulate a reasonable move (best available position)
	var move_position = _get_best_move()
	if move_position == -1:
		print("❌ No valid moves available")
		return

	# Broadcast the move to all participants
	var move_msg = "GAME_MOVE:" + my_symbol + ":" + str(move_position) + ":" + str(zenoh_peer.get_unique_id())
	var data = PackedByteArray()
	data.append_array(move_msg.to_utf8_buffer())

	var result = zenoh_peer.put_packet(data, 1)
	if result == 0:
		print("✅ Sent Tic-Tac-Toe move: " + my_symbol + " at position " + str(move_position))
		button.disabled = true

		# Apply the move locally immediately for responsiveness
		_apply_game_move(my_symbol, move_position)

		if not game_over:
			print_board()
			if label:
				label.text = "TURN: " + current_player + " (You sent to " + ("X" if my_symbol == "O" else "O") + ")"

func _make_first_move_immediately():
	# 🔥 CRITICAL FIX: Leader makes first X move immediately after announcing game!
	print("🎮 LEADER: Making first move as X...")
	_on_make_move()

func _check_and_make_auto_move():
	# 🔥 HLC-BASED TURN VALIDATION: Prevent race conditions with timestamp checks
	print("🔍 DEBUG: Checking auto-move - current_player=" + current_player + ", my_symbol=" + my_symbol + ", game_over=" + str(game_over))

	# Basic turn check - is it my turn?
	if current_player != my_symbol or game_over:
		button.disabled = true
		if not game_over:
			print("⏳ Waiting for " + current_player + " to move - current player is " + current_player + ", I am " + my_symbol)
		else:
			print("🏆 Game finished")
		return

	# 🔥 HLC TIMESTAMP VALIDATION: For O players, ensure sufficient time since X's last move
	# Prevents O from auto-playing immediately after receiving X's move (race condition)
	if my_symbol == "O":
		var current_hlc = get_current_hlc_timestamp()
		var elapsed_since_x = current_hlc - last_x_move_hlc_timestamp

		print("🕐 HLC Validation - Current: " + str(current_hlc) + ", Last X: " + str(last_x_move_hlc_timestamp) +
			  ", Elapsed: " + str(elapsed_since_x) + ", Threshold: " + str(hlc_turn_threshold))

		if elapsed_since_x < hlc_turn_threshold:
			print("⏳ TOO SOON: O waiting for HLC timing before auto-play (" + str(hlc_turn_threshold - elapsed_since_x) + " remaining)")
			return  # Don't make move yet

		print("✅ HLC VALIDATED: Sufficient time elapsed - O can now auto-play")

	# Turn is valid and timing checks passed - proceed with auto-play
	button.disabled = false
	if label:
		label.text = "YOUR TURN: Auto-playing move (" + my_symbol + ")"

	print("🎮 AUTO-PLAYING: It's my turn (" + my_symbol + ") - making move now!")
	_on_make_move()

func _get_best_move() -> int:
	# Simple AI: Find empty positions, prefer winning/critical positions
	var possible_moves = []

	# Check if we can win immediately
	for i in range(9):
		if board[i] == "":
			board[i] = my_symbol
			if check_winner() == my_symbol:
				board[i] = ""  # Undo
				return i
			board[i] = ""  # Undo

	# Block opponent wins
	var opponent = "O" if my_symbol == "X" else "X"
	for i in range(9):
		if board[i] == "":
			board[i] = opponent
			if check_winner() == opponent:
				board[i] = ""  # Undo
				return i
			board[i] = ""  # Undo

	# Prefer center and corners
	var priorities = [4, 0, 2, 6, 8, 1, 3, 5, 7]
	for pos in priorities:
		if board[pos] == "":
			return pos

	return -1  # No moves available

func _send_count():
	# 🚫 DISABLED: Old countdown logic replaced by Tic-Tac-Toe
	pass

func _on_countdown_tick():
	# Automatic countdown disabled - only send when ack received
	pass

func _on_poll_timeout():
	# Poll for network messages and connection state
	zenoh_peer.poll()

	# Handle connection completion during leader election
	if leader_election_phase and my_id == -1 and zenoh_peer.connection_status() == 2:
		my_id = zenoh_peer.get_unique_id()
		var zid = zenoh_peer.get_zid()
		print("Connection completed in election - ID: " + str(my_id) + " | ZID: " + zid)

		# Signal to all participants that final election phase should begin
		print("📢 Broadcasting final election signal - I now have real peer ID #" + str(my_id))
		signal_final_election()

		# Restart election in final phase (wait short time for signals)
		restart_election_with_real_id()

		# Update UI
		update_peer_info()

	# Check for received packets
	while zenoh_peer.get_available_packet_count(1) > 0:
		var data = zenoh_peer.get_packet(1)
		var data_string = data.get_string_from_utf8()

			# Handle simple countdown message: "COUNT:N:FROM_ID"
		if data_string.begins_with("COUNT:"):
			var parts = data_string.split(":")
			var count = -1
			var from_player_id = -1

			if parts.size() >= 3:  # Simple format without hash
				count = int(parts[1])
				from_player_id = int(parts[2])
				print("Player " + str(zenoh_peer.get_unique_id()) + " received COUNT:" + str(count) + " from Player " + str(from_player_id))
			else:
				# Fallback for old format
				var count_str = data_string.substr(6)
				count = int(count_str)
				from_player_id = get_other_player_id()
				print("Player " + str(zenoh_peer.get_unique_id()) + " received COUNT:" + str(count) + " from Player " + str(from_player_id) + " (legacy format)")

			last_received_count = count

			# Acknowledge receipt by decrementing and sending next number (after 1 second delay)
			# Only respond to messages from other peers (not own messages)
			if countdown_number > 0 and count >= 0 and from_player_id != zenoh_peer.get_unique_id():
				if label:
					label.text = "Received: " + str(count) + " - Preparing response..."
				print("Player " + str(zenoh_peer.get_unique_id()) + " acknowledging receipt of " + str(count) + " from " + str(from_player_id) + " - will respond in 1 second with countdown: " + str(countdown_number))

				# In automatic mode, complete the exchange and exit
				var args = OS.get_cmdline_args()
				if args.has("--client") and count <= 1:  # Exit after complete minimal exchange
					print("Client test successful - completed packet exchange!")
					get_tree().quit()

				# Wait 1 second before responding (doesn't block the polling)
				var response_timer = Timer.new()
				response_timer.one_shot = true
				response_timer.wait_time = 1.0
				response_timer.connect("timeout", Callable(self, "_delayed_response"))
				add_child(response_timer)
				response_timer.start()
			else:
				if label:
					label.text = "Game already finished"

func _delayed_response():
	# This runs after 1 second delay
	if countdown_number > 0:
		countdown_number -= 1
		if countdown_number == 0:
			if label:
				label.text = "GAME OVER!"
			print("Countdown complete!")
		else:
			if label:
				label.text = "Responding with: " + str(countdown_number)
			print("After 1 second - sending countdown: " + str(countdown_number))
			_send_count()
			# Wait for next receipt (no auto countdown)

func get_other_player_id():
	return 2 if my_id == 1 else 1

# UI STATUS UPDATE FUNCTIONS
func update_peer_info():
	var peer_info_node = find_child("peer_info")
	if peer_info_node:
		var role = "Server/Leader" if is_host else "Client/Follower"
		if connection_state == STATE_LEADER_ELECTION:
			role = "Electing Leader"

		var zid = ""
		if zenoh_peer and zenoh_peer.has_method("get_zid"):
			zid = zenoh_peer.get_zid()
		else:
			zid = "N/A"

		peer_info_node.text = "Peer ID: " + str(my_id) + " | Role: " + role + " | State: " + get_state_text(connection_state) + " | ZID: " + zid



func get_state_text(state: int) -> String:
	match state:
		STATE_DISCONNECTED: return "DISCONNECTED"
		STATE_CONNECTING: return "CONNECTING"
		STATE_CONNECTED: return "CONNECTED"
		STATE_FAILED: return "FAILED"
		STATE_SERVER_READY: return "SERVER_READY"
		STATE_CLIENT_ATTEMPTING: return "CLIENT_ATTEMPTING"
		STATE_ZENOH_SESSION_FAILED: return "ZENOH_FAILED"
		STATE_LEADER_ELECTION: return "LEADER_ELECTION"
		_: return "UNKNOWN"

# GENSERVER-DRIVEN LEADER ELECTION - Clean Process Isolation
func start_leader_election():
	# Initialize Election GenServer
	var init_result = election_server.init({
		"my_id": zenoh_peer.get_unique_id() if zenoh_peer.get_unique_id() != -1 else 0,
		"game_id": zenoh_peer.game_id
	})

	if init_result[0] != "ok":
		print("❌ Failed to initialize election GenServer")
		return

	print("🏁 Starting Elixir-Style Leader Election with GenServer")
	leader_election_phase = true
	connection_state = STATE_LEADER_ELECTION

	# Update UI - use GenServer state
	if label:
		label.text = "ELECTING LEADER: Initializing coordination..."

	# Try client connection first (non-blocking)
	var result = zenoh_peer.create_client("localhost", 7447)
	if result != 0:
		# If no existing server, become the leader immediately via GenServer
		result = zenoh_peer.create_server(7447, 32)
		if result == 0:
			print("✅ Became immediate leader - server role")
			# Force election completion through direct GenServer call
			var call_result = election_server.handle_call({
				"force_leader": true,
				"leader_id": zenoh_peer.get_unique_id()
			}, self, election_server.state_data)
			if call_result[0] == "reply":
				complete_leader_election_as_leader()
				return
		else:
			print("❌ Failed to create server as leader")
			return

	print("Client connection initiated - election will coordinate through GenServer events")

	# Start polling for connection state changes and send events to GenServers
	var poll_timer = Timer.new()
	poll_timer.autostart = true
	poll_timer.wait_time = 0.1
	poll_timer.connect("timeout", Callable(self, "_on_coordinated_poll"))
	add_child(poll_timer)

func send_election_heartbeat():
	var election_id: int = 0

	# Use unique ID if available, otherwise use deterministic election ID
	if my_id != -1:
		election_id = my_id
	else:
		# Use process ID and timestamp as deterministic election ID
		election_id = zenoh_peer.request_hlc_timestamp()

	var heartbeat_msg = "ELECT:" + str(election_id) + ":" + str(zenoh_peer.get_zid())
	var data = PackedByteArray()
	data.append_array(heartbeat_msg.to_utf8_buffer())

	var result = zenoh_peer.put_packet(data, 1)
	print("Sent election heartbeat: " + heartbeat_msg + " (current election_id: " + str(election_id) + ")")
	if result != 0:
		print("⚠️ Election heartbeat send failed, but continuing")

func _on_election_poll():
	zenoh_peer.poll()

	# Handle state machine transitions based on connection state
	if election_state == ElectionState.WAITING_CONNECTIONS:
		if zenoh_peer.connection_status() == 2:  # Connected
			print("Zenoh connected - proceeding to generate HLC election ID")
			_election_transition_generating_id()

	# Process election messages and state transitions
	while zenoh_peer.get_available_packet_count(1) > 0:
		var data = zenoh_peer.get_packet(1)
		var msg = data.get_string_from_utf8()

		_process_election_message(msg)

func _process_election_message(msg: String):
	# Handle different election message types based on current state
	if msg.begins_with("ELECT:"):
		var parts = msg.split(":")
		if parts.size() >= 3:
			var peer_election_id = int(parts[1])
			var peer_zid = parts[2]

			# Add to collected election IDs
			if collected_peer_ids.find(peer_election_id) == -1:
				collected_peer_ids.append(peer_election_id)
				print("📥 Received election announcement #" + str(peer_election_id) + " (" + peer_zid + ")")
				print("   Total election IDs collected: " + str(collected_peer_ids.size()))

			# State-specific message handling
			match election_state:
				ElectionState.COLLECTING_PEERS:
					if label:
						label.text = "ELECTING LEADER: " + str(collected_peer_ids.size()) + " participants"
					_check_if_all_peers_collected()

	elif msg.begins_with("VICTORY:"):
		var parts = msg.split(":")
		if parts.size() >= 4:
			var winner_election_id = int(parts[1])
			var winner_zid = parts[2]
			var requires_ack = parts.size() >= 5 and parts[4] == "ACK_REQUIRED"
			print("🎉 VICTORY MESSAGE received from #" + str(winner_election_id) + " (" + winner_zid + ")")

			# 🎯 CRITICAL FIX: Process victory messages from ANY state (not just VICTORY_LISTENING)
			# Followers must be able to respond to victory messages regardless of their current election state

			if winner_election_id == my_election_id:
				print("✅ Victory message confirms: I won the election")
				# I'm the leader - this is just a confirmation
				if election_state == ElectionState.VICTORY_BROADCASTING:
					pass  # Already handling the victory process
				else:
					# Something weird happened - force leader state
					election_state = ElectionState.FINALIZED
					complete_leader_election_as_leader()
			else:
				print("✅ Another instance won - I am becoming a follower")
				# I'm a follower - send acknowledgment if required
				if requires_ack:
					print("📤 Sending victory acknowledgment to leader")
					var ack_msg = "VICTORY_ACK:" + str(my_election_id) + ":" + str(zenoh_peer.get_zid())
					var data = PackedByteArray()
					data.append_array(ack_msg.to_utf8_buffer())
					zenoh_peer.put_packet(data, 1)
					print("✅ Sent acknowledgment: " + ack_msg)
				# 🏆 COMPLETE election as follower regardless of current state
				complete_leader_election_as_follower()

	elif msg.begins_with("VICTORY_ACK:"):
		var parts = msg.split(":")
		if parts.size() >= 3:
			var ack_from_id = int(parts[1])
			print("📨 Received victory acknowledgment from #" + str(ack_from_id))

			# Leader: Count acknowledgments and proceed when all received
			if election_state == ElectionState.VICTORY_BROADCASTING:
				victory_acknowledgments += 1
				print("📊 Acknowledgment #" + str(victory_acknowledgments) + "/" + str(expected_acknowledgments) + " received")

				if victory_acknowledgments >= expected_acknowledgments:
					print("✅ ALL VICTORY ACKNOWLEDGMENTS RECEIVED - SAFE TO PROCEED")
					election_state = ElectionState.FINALIZED
					print("🏆 Election complete - I am the SINGLE LEADER")
					print("🎯 ALL PARTICIPANTS SYNCHRONIZED - Starting Tic-Tac-Toe!")
					complete_leader_election_as_leader()
	elif msg.begins_with("FINAL_ELECT:"):
		print("🔄 Received final election signal - restarting election with real IDs")
		restart_election_with_real_id()

func _check_if_all_peers_collected():
	# MESSAGE-DRIVEN QUORUM: Pure state machine - no waiting, only message responses
	# This requires ALL expected peers before state transition (pure coordination)
	var expected_total_participants = 3  # Known for current test
	var required_peer_announcements = expected_total_participants - 1  # Minus self

	if collected_peer_ids.size() >= required_peer_announcements:
		print("🔄 MESSAGE-DRIVEN TRANSITION: Quorum achieved with " + str(collected_peer_ids.size()) + "/" + str(required_peer_announcements) + " peer announcements")
		print("✅ PURE STATE MACHINE: Transitioning to leader decision phase")
		_election_transition_deciding_leader()  # QUEUE-DRIVEN: Called after each message receipt
	# NO WAITING: State machine stays in COLLECTING_PEERS until condition met by message events

# STATE MACHINE TRANSITION FUNCTIONS
func _election_transition_generating_id():
	election_state = ElectionState.GENERATING_ID
	print("🔗 Election State: GENERATING_ID")

	# Request HLC timestamp for consistent election ID
	var hlc_result = zenoh_peer.request_hlc_timestamp()
	if hlc_result == 0:
		print("HLC timestamp requested - waiting for response")

		# Set a short timer to wait for HLC response and transition
		election_timer = Timer.new()
		election_timer.one_shot = true
		election_timer.wait_time = 0.5  # Wait 500ms for HLC
		election_timer.connect("timeout", Callable(self, "_on_hlc_ready_timeout"))
		add_child(election_timer)
		election_timer.start()
	else:
		print("❌ Failed to request HLC timestamp")
		# Fall back to using current timestamp
		my_election_id = int(Time.get_unix_time_from_system() * 1000000)
		_election_transition_broadcasting()

func _on_hlc_ready_timeout():
	# Check if HLC timestamp is available (simplified - in real impl check a callback)
	# For now, generate our ID and proceed
	my_election_id = int(Time.get_unix_time_from_system() * 1000000) # Use microsecond precision
	print("Using election ID: " + str(my_election_id))
	_election_transition_broadcasting()

func _election_transition_broadcasting():
	election_state = ElectionState.BROADCASTING_HEARTBEATS
	print("🔗 Election State: BROADCASTING_HEARTBEATS")

	# Broadcast our election ID
	var heartbeat_msg = "ELECT:" + str(my_election_id) + ":" + str(zenoh_peer.get_zid())
	var data = PackedByteArray()
	data.append_array(heartbeat_msg.to_utf8_buffer())

	var result = zenoh_peer.put_packet(data, 1)
	if result == 0:
		print("✅ Sent election announcement: " + heartbeat_msg)
		if label:
			label.text = "ELECTING LEADER: Announced participation"
	else:
		print("⚠️ Failed to send election announcement")

	# Transition to collecting peers after broadcasting
	_election_transition_collecting_peers()

func _election_transition_collecting_peers():
	election_state = ElectionState.COLLECTING_PEERS
	print("🔗 Election State: COLLECTING_PEERS (PURE MESSAGE-DRIVEN)")
	print("🚫 NO TIMEOUTS: State machine waits for quorum via message queue events only")

	if label:
		label.text = "ELECTING LEADER: Awaiting quorum (message-driven)"
	# NO TIMER: Pure state machine - waits for _check_if_all_peers_collected() to trigger transition

# FUNCTION REMOVED: No timeouts in pure message-driven state machine
# All state transitions happen via _check_if_all_peers_collected() being called
# after each message receipt when quorum requirements are met

func _election_transition_deciding_leader():
	election_state = ElectionState.DECIDING_LEADER
	print("🔗 Election State: DECIDING_LEADER")

	if label:
		label.text = "ELECTING LEADER: Analyzing participants"

	# Include our own ID in the decision
	collected_peer_ids.append(my_election_id)

	# Sort by ID - lowest HLC timestamp wins!
	collected_peer_ids.sort()

	print("Election Decision Analysis:")
	print("  All participant IDs: " + str(collected_peer_ids))
	print("  Lowest ID (winner): " + str(collected_peer_ids[0]))
	print("  My ID: " + str(my_election_id))

	# Bully algorithm: lowest ID wins
	var winner_id = collected_peer_ids[0]
	current_leader_id = winner_id

	if my_election_id == winner_id:
		print("🎉 I WON THE ELECTION! Lowest HLC ID: #" + str(my_election_id))
		_election_transition_victory_broadcasting()
	else:
		print("✅ I lost - following leader #" + str(winner_id))
		_election_transition_victory_listening()

func _election_transition_victory_broadcasting():
	election_state = ElectionState.VICTORY_BROADCASTING
	print("🔗 Election State: VICTORY_BROADCASTING")

	if label:
		label.text = "LEADER: Broadcasting victory & waiting for acknowledgments"

	# TRIPLE BARRIER: Victory + Acknowledgment + State Synchronization
	# Calculate expected acknowledgments (total participants - 1)
	var total_participants = collected_peer_ids.size() + 1  # +1 for self
	var expected_acknowledgments = total_participants - 1

	print("🎯 VICTORY BARRIER: Expecting " + str(expected_acknowledgments) + " victory acknowledgments from followers")

	# Leader state: wait for all acknowledgments before starting game
	# This ensures EVERY participant reaches the same state before Tic-Tac-Toe begins

	# Announce victory and request acknowledgments
	var victory_msg = "VICTORY:" + str(my_election_id) + ":" + str(zenoh_peer.get_zid()) + ":HLC_LOWEST_WINS:ACK_REQUIRED"
	var data = PackedByteArray()
	data.append_array(victory_msg.to_utf8_buffer())

	var result = zenoh_peer.put_packet(data, 1)
	if result == 0:
		print("🌟 Victory broadcast sent: " + victory_msg)
		if label:
			label.text = "LEADER: Awaiting victory acknowledgments..."
	else:
		print("⚠️ Failed to send victory broadcast")

func _election_transition_victory_listening():
	election_state = ElectionState.VICTORY_LISTENING
	print("🔗 Election State: VICTORY_LISTENING")

	if label:
		label.text = "ELECTING LEADER: Waiting for winner announcement"

	# Set a reasonable timeout for victory announcement
	election_timer = Timer.new()
	election_timer.one_shot = true
	election_timer.wait_time = 3.0  # Longer timeout for victory announcement
	election_timer.connect("timeout", Callable(self, "_on_victory_listening_timeout"))
	add_child(election_timer)
	election_timer.start()

func _on_victory_listening_timeout():
	print("Victory announcement timeout - assuming election complete")
	election_state = ElectionState.FINALIZED
	print("🏁 Election finalized - proceeding as follower")
	complete_leader_election_as_follower()

func signal_final_election():
	print("🏁 Signalling final election to all participants!")
	var signal_msg = "FINAL_ELECT:" + str(my_id) + ":" + str(zenoh_peer.get_zid())
	var data = PackedByteArray()
	data.append_array(signal_msg.to_utf8_buffer())

	zenoh_peer.put_packet(data, 1)
	print("Sent final election signal: " + signal_msg)

func _on_election_timeout():
	print("Election timeout - analyzing " + str(known_peers.size()) + " discovered peers")

	if my_id == -1:
		print("⏳ Still no Zenoh ID - extending election")
		restart_election_with_timeout_extension()
		return

	# TRUE BULLY ALGORITHM: Am I the lowest ID among all known peers?
	var all_known_peers = []
	all_known_peers.append_array(known_peers)
	all_known_peers.append(my_id)  # Include myself

	# Sort by ID - lowest ID wins
	all_known_peers.sort()
	var lowest_peer_id = all_known_peers[0]

	print("Bully Election Analysis:")
	print("  Known peers: " + str(known_peers))
	print("  My ID: " + str(my_id))
	print("  Lowest ID: " + str(lowest_peer_id))
	print("  Am I the winner? " + str(my_id == lowest_peer_id))

	if my_id == lowest_peer_id:
		print("🎉 BULLY VICTORY: I have the lowest ID #" + str(my_id))
		print("🌟 I am the SINGLE LEADER!")
		current_leader_id = my_id
		broadcast_leader_victory()
		complete_leader_election_as_leader()
	else:
		print("✅ Defeat: " + str(lowest_peer_id) + " has lower ID than me (" + str(my_id) + ")")
		print("👥 I am a follower to leader #" + str(lowest_peer_id))
		current_leader_id = lowest_peer_id
		stop_broadcasting_hearts()  # Quit competing
		complete_leader_election_as_follower()

func restart_election_with_timeout_extension():
	# Extend election timeout again until we have real peer IDs
	print("Extending election timeout for real peer ID assignment...")

	# Don't free existing timer during a callback - Godot may still process it
	# Just create a new one with extended timeout
	var new_timer = Timer.new()
	new_timer.one_shot = true
	new_timer.wait_time = 2.0  # Additional 2 seconds to get real IDs
	new_timer.connect("timeout", Callable(self, "_on_election_timeout"))
	add_child(new_timer)
	new_timer.start()

	# Store the new timer and try to clean up old one safely
	var old_timer = election_timer
	election_timer = new_timer

	# Mark old timer for safe cleanup (don't call immediately)
	if old_timer and old_timer != new_timer:
		# Use a deferred cleanup to avoid locking issues
		call_deferred("_safe_free_timer", old_timer)

	print("Election extended - waiting for real Zenoh peer IDs...")

func complete_leader_election_as_leader():
	# Change to server mode
	print("🎮 Election complete - starting TIC-TAC-TOE server for followers")
	leader_election_phase = false
	connection_state = STATE_SERVER_READY
	is_host = true

	# 🔥 I AM X (first player) - leader always gets X!
	my_symbol = "X"
	print("🎯 I am X (leader/first player) in Tic-Tac-Toe game")

	if label:
		label.text = "LEADER: Waiting for Tic-Tac-Toe opponents..."

	# Setup networking and initialize game
	setup_tic_tac_toe_networking()

func complete_leader_election_as_follower():
	# Switch to client mode to connect to the elected leader
	print("Election complete - connecting as client to leader")
	leader_election_phase = false
	connection_state = STATE_CONNECTED
	is_host = false

	# 🔥 FOLLOWERS GET O - opposite of leaders!
	my_symbol = "O"
	print("🎯 I am O (follower/second player) in Tic-Tac-Toe game")

	if label:
		label.text = "FOLLOWER: Connecting to leader for Tic-Tac-Toe..."

	# Setup Tic-Tac-Toe client networking (not legacy networking)
	setup_tic_tac_toe_networking()

func restart_election_with_real_id():
	# Clear previous election state
	known_peers = []
	if election_timer:
		election_timer.stop()
		election_timer.free()
	election_timer = null

	# Restart election with 2-second timeout since we now have real IDs
	print("Restarting election with dedicated 2-second phase for real ID coordination")

	# Send heartbeat with real ID now
	send_election_heartbeat()

	# Start shorter election timeout (2 seconds) for real ID election
	election_timer = Timer.new()
	election_timer.one_shot = true
	election_timer.wait_time = 2.0  # 2 seconds for real ID election
	election_timer.connect("timeout", Callable(self, "_on_election_timeout"))
	add_child(election_timer)
	election_timer.start()

	print("Election restarted with real peer IDs - completing leader selection")

func broadcast_leader_victory():
	# Announce victory to all peers
	var victory_msg = "VICTORY:" + str(my_id) + ":" + str(zenoh_peer.get_zid()) + ":LOWEST_ID_WINS"
	var data = PackedByteArray()
	data.append_array(victory_msg.to_utf8_buffer())

	zenoh_peer.put_packet(data, 1)
	print("🌟 Broadcasting BULLY VICTORY: '" + victory_msg + "' - All other instances should become followers")

func stop_broadcasting_hearts():
	# Stop sending heartbeats since election is over
	print("🛑 Stopping election heartbeat broadcasts - election is over")
	# Could kill the polling timer here, but the leader might still need it



func _on_hlc_request_pressed():
	print("🎯 Requesting HLC timestamp from Zenoh session...")
	var result = zenoh_peer.request_hlc_timestamp()
	if result == 0:
		print("✅ HLC timestamp request sent to worker thread")
	else:
		print("❌ Failed to send HLC timestamp request")

func get_current_hlc_timestamp() -> int:
	# 🔥 Get current HLC timestamp from Zenoh session for turn validation
	# Uses system time + process ID for distributed coordination (same as Rust implementation)
	var process_id = OS.get_process_id()
	var system_time_nanos = int(Time.get_unix_time_from_system() * 1000000000)
	var hlc_timestamp = (process_id * 1000000000) + system_time_nanos

	print("⏱️ Generated HLC timestamp: " + str(hlc_timestamp))
	return hlc_timestamp

func _safe_free_timer(old_timer: Timer):
	# Safely free the timer after the current frame to avoid locking issues
	if old_timer and not old_timer.is_inside_tree():
		# Timer is already removed from tree, safe to free
		old_timer.free()
		print("Old election timer safely freed")
	elif old_timer and old_timer.is_inside_tree():
		# Timer still in tree, mark for freeing at end of frame
		old_timer.call_deferred("free")
		print("Old election timer marked for deferred freeing")

func _notification(what):
	if what == NOTIFICATION_EXIT_TREE:
		if zenoh_peer:
			print("Cleaning up zenoh peer...")
			zenoh_peer.close()
