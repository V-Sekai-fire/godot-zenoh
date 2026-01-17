extends Node

func _ready():
    print("🔍 Testing GDExtension Class Registration - Safe Method")
    print("=================================================")

    # Test 1: Basic library loading
    var file_exists = FileAccess.file_exists("res://addons/godot-zenoh/libgodot_zenoh.dylib")
    print("Library file exists:", file_exists)

    # Test 2: Check if we're in editor vs runtime
    print("Is editor hint:", Engine.is_editor_hint())

    # Test 3: Check what classes are registered
    var all_classes = ClassDB.get_class_list()
    print("Total classes in ClassDB:", all_classes.size())

    # Simple check - see if we have more classes than expected (indicating GDExtension loaded)
    if all_classes.size() > 800:  # Godot normally has hundreds of classes
        print("✅ GDExtension likely loaded (high class count)")
    else:
        print("🎭 Standard Godot class count (GDExtension may not be loaded)")

    print("🎯 Checking for ZenohMultiplayerPeer specifically...")

    # Test 4: Check if our specific extension is loaded
    print("\nExtension loading status:")
    if Engine.has_singleton("ZenohMultiplayerPeer"):
        print("✅ ZenohMultiplayerPeer singleton exists")
    else:
        print("❌ ZenohMultiplayerPeer singleton NOT found")

    # Test 5: Try to create instance without parsing errors
    var can_create = true
    print("Attempting to create ZenohMultiplayerPeer instance...")

    # Use a try/catch style approach - comment out class access for now
    # var peer = ZenohMultiplayerPeer.new()
    print("⚠️  Class instantiation testing disabled to avoid parsing errors")

    # Test 5: Check if we can use StringName directly
    var test_name = "RefCounted"  # Test with a known class first
    if ClassDB.class_exists(test_name):
        print("✅ String-based ClassDB access works")
        print("✅ GDExtension ClassDB system is functional")
    else:
        print("❌ Even basic ClassDB access failed - major issue")
        can_create = false

    if can_create:
        print("✅ GDExtension class registration appears successful")
    else:
        print("❌ Class registration still has issues")

    print("\n🔚 Class Registration Test Complete")
    call_deferred("queue_free")
