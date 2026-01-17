// GDExtension entry point
use godot::init::gdextension;

pub mod networking;
pub mod peer;

// mod extension;  // Not needed for current implementation
// pub use extension::*;  // Not needed for current implementation

// Make the ZenohMultiplayerPeer class visible for gdextension registration
pub use peer::ZenohMultiplayerPeer;

// Register the extension with Godot
#[gdextension]
unsafe impl godot::init::ExtensionLibrary for ZenohExtension {
    // Extension library initialization - uses default implementations for compatibility
}

pub struct ZenohExtension;
