//! Module for looking up HTML5 entities by name.

include!(concat!(env!("OUT_DIR"), "/html_entities.rs"));

/// Looks up an HTML5 entity by its name.
pub fn look_up_html5_entity_by_name(name: &str) -> Option<&'static str> {
    HTML_ENTITIES.get(name).copied()
}
