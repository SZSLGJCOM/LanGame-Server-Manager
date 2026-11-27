#![cfg(test)]

#[path = "ark_ascended_tests.rs"]
mod ark_ascended;
#[path = "ark_ascended_support_tests.rs"]
mod ark_ascended_support;
#[cfg(windows)]
#[path = "ark_cluster_backups_integration_tests.rs"]
mod ark_cluster_backups_integration_tests;
#[path = "ark_evolved_tests.rs"]
mod ark_evolved;
#[path = "ark_runtime_tests.rs"]
mod ark_runtime;
#[path = "ark_settings_precondition_tests.rs"]
mod ark_settings_precondition;
