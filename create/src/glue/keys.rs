//! Key mappings by name: vanilla's (`key.jump`) and the mod's own, resolved when asked.
//!
//! Horizon reads a vanilla mapping as a snapshot (`ClientHost::mapping`) and the mod's own by
//! id; it has no reading of the physical key under a mapping, so "physically held" is the
//! mapping's own held state.

use mod_api::{ClientHost, MappingError, Text};

/// `KeyMapping.isDown` of vanilla's mapping `name`.
pub fn down(host: &dyn ClientHost, name: &str) -> bool {
    host.mapping(name).ok().flatten().is_some_and(|m| m.down)
}

/// Whether the key bound to vanilla's mapping `name` is held (`InputConstants.isKeyDown`).
pub fn physical_down(host: &dyn ClientHost, name: &str) -> bool {
    down(host, name)
}

/// `KeyMapping.setDown` of vanilla's mapping `name`.
pub fn set_down(host: &mut dyn ClientHost, name: &str, down: bool) -> Result<(), MappingError> {
    host.set_mapping_down(name, down)
}

/// `KeyMapping.getTranslatedKeyMessage` of vanilla's mapping `name`.
pub fn label(host: &dyn ClientHost, name: &str) -> Option<Text> {
    host.mapping(name).ok().flatten().map(|m| m.label)
}

/// The key the mod's mapping `id` is bound to.
pub fn mod_binding(host: &dyn ClientHost, owner: &str, id: &str) -> Option<String> {
    host.mod_key(owner, id).map(|(key, _)| key)
}

/// Whether the mod's mapping `id` is held.
pub fn mod_down(host: &dyn ClientHost, owner: &str, id: &str) -> bool {
    host.is_down(owner, id)
}

/// `KeyMapping.consumeClick` of the mod's mapping `id`.
pub fn mod_consume_click(host: &mut dyn ClientHost, owner: &str, id: &str) -> bool {
    host.consume_click(owner, id)
}
