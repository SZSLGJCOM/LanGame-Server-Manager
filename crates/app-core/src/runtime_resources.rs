use serde::{Deserialize, Serialize};

/// Limits cover the complete instance, including every world and descendant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RuntimeResourceLimits {
    /// Percentage of the host CPU allocation, not a single logical processor.
    pub cpu_percent: Option<u8>,
    /// Windows Job committed-memory limit. One MiB is 1,048,576 bytes.
    pub memory_limit_mib: Option<u64>,
    /// Host headroom used only when a memory budget is configured.
    pub host_memory_reserve_mib: u64,
}

impl Default for RuntimeResourceLimits {
    fn default() -> Self {
        Self {
            cpu_percent: None,
            memory_limit_mib: None,
            host_memory_reserve_mib: 2048,
        }
    }
}

impl RuntimeResourceLimits {
    pub fn enabled(&self) -> bool {
        self.cpu_percent.is_some() || self.memory_limit_mib.is_some()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self
            .cpu_percent
            .is_some_and(|value| !(1..=100).contains(&value))
        {
            return Err("cpu_percent must be an integer from 1 to 100, or null".into());
        }
        if self
            .memory_limit_mib
            .is_some_and(|value| !(64..=1_048_576).contains(&value))
        {
            return Err(
                "memory_limit_mib must be an integer from 64 to 1048576 MiB, or null".into(),
            );
        }
        if self.host_memory_reserve_mib > 1_048_576 {
            return Err("host_memory_reserve_mib must be an integer from 0 to 1048576 MiB".into());
        }
        Ok(())
    }
}
