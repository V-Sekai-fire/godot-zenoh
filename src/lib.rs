// GDExtension entry point
use godot::prelude::*;

#[gdextension]
unsafe impl godot::init::ExtensionLibrary for ZenohExtension {
    fn on_level_init(level: godot::init::InitLevel) {
        if level == godot::init::InitLevel::Scene {
            godot::register_class::<ZenohMultiplayerPeer>();
        }
    }
}

pub mod networking;
pub mod peer;
pub mod raft_consensus;

pub use peer::*;
