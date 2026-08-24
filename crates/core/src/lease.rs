//! Heartbeat/lease tuning parameters.
//!
//! Defaults chosen for the hackathon demo (DEVLOG 2026-08-24): a dead worker
//! is detected within ~1.5 s — three missed heartbeats. Tests use faster
//! values so kill→reassign→recover runs in well under a second.

use crate::error::SwarmError;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseConfig {
    pub heartbeat_interval: Duration,
    pub lease_timeout: Duration,
}

impl Default for LeaseConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_millis(500),
            lease_timeout: Duration::from_millis(1500),
        }
    }
}

impl LeaseConfig {
    /// A timeout at or below the heartbeat interval would reap healthy workers
    /// between beats — refuse it up front.
    pub fn validate(&self) -> Result<(), SwarmError> {
        if self.lease_timeout <= self.heartbeat_interval {
            return Err(SwarmError::InvalidConfig(format!(
                "lease_timeout ({:?}) must be strictly greater than heartbeat_interval ({:?})",
                self.lease_timeout, self.heartbeat_interval
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert!(LeaseConfig::default().validate().is_ok());
    }

    #[test]
    fn timeout_must_exceed_heartbeat() {
        let cfg = LeaseConfig {
            heartbeat_interval: Duration::from_secs(1),
            lease_timeout: Duration::from_secs(1),
        };
        assert!(matches!(
            cfg.validate(),
            Err(SwarmError::InvalidConfig(_))
        ));
    }
}
