//! Run the production UI state machines without initializing CEF or a GPU.
#![allow(dead_code)]
#[path = "../../../spikes/composite/src/tooltip_state.rs"]
mod tooltip_state;
#[path = "../../../spikes/composite/src/context_menu_model.rs"]
mod context_menu_model;
#[path = "../../../spikes/composite/src/reading_rules_migration.rs"]
mod reading_rules_migration;

#[path = "../../../spikes/composite/src/file_viewer.rs"]
mod file_viewer;
#[path = "../../../spikes/composite/src/pip_policy.rs"]
mod pip_policy;
