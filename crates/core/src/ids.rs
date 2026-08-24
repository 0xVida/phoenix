//! Strongly-typed identifiers. No `String` soup anywhere near the state machine.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! define_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(uuid::Uuid);

        impl $name {
            pub fn generate() -> Self {
                Self(uuid::Uuid::new_v4())
            }

            pub fn nil() -> Self {
                Self(uuid::Uuid::nil())
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }
    };
}

define_id!(
    /// Identifies one PR-review task end to end.
    TaskId
);
define_id!(
    /// Identifies one worker generation; a fresh id is minted per attempt.
    WorkerId
);

/// Fencing token: which assignment generation a message belongs to.
/// The supervisor rejects any message whose attempt is not the current one.
pub type Attempt = u32;
