# Basic Zenoh networking test for Godot
# Tests basic GDExtension functionality and packet handling

extends SceneTree

var zenoh_peer: ZenohMultiplayerPeer

func _init():
	print("Zenoh Networking Test")
	print("Checking basic functionality...")

	# Create and test basic peer functionality
	zenoh_peer = ZenohMultiplayerPeer.new()

	if not zenoh_peer:
		print("ERROR: Failed to create ZenohMultiplayerPeer")
		quit()
		return

	zenoh_peer.game_id = "test_game"

	print("Testing server creation...")
	var server_result = zenoh_peer.create_server(7447, 32)
	print("Server creation result:", server_result)

	print("Testing channel configuration...")
	zenoh_peer.set_transfer_channel(0)
	print("Channel set to:", zenoh_peer.get_transfer_channel())

	print("Testing packet sending...")
	var test_data = PackedByteArray([1, 2, 3, 4])
	var send_result = zenoh_peer.put_packet(test_data)
	print("Packet send result:", send_result)

	print("Testing ZID retrieval...")
	var zid = zenoh_peer.get_zid()
	print("ZID:", zid)

	print("Basic tests completed")
	zenoh_peer.close()
	quit()
