// Copyright (c) 2026-present K. S. Ernest (iFire) Lee
// SPDX-License-Identifier: MIT

// GDExtension entry point
use godot::init::gdextension;

pub mod networking;
pub mod peer;

pub use peer::ZenohMultiplayerPeer;

#[gdextension]
unsafe impl godot::init::ExtensionLibrary for ZenohExtension {}

pub struct ZenohExtension;
