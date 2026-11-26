use serde::{Deserialize, Serialize};

use crate::UserId;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Actor {
    #[default]
    Owner,
    Member {
        user_id: UserId,
    },
    Agent {
        name: String,
    },
}

impl Actor {
    pub fn is_owner(&self) -> bool {
        matches!(self, Self::Owner)
    }

    /// Agent identity never inherits owner privileges, even when its name
    /// looks like an owner account.
    pub fn is_agent(&self) -> bool {
        matches!(self, Self::Agent { .. })
    }

    pub fn as_str(&self) -> String {
        match self {
            Self::Owner => "owner".to_owned(),
            Self::Member { user_id } => format!("member:{user_id}"),
            Self::Agent { name } => format!("agent:{name}"),
        }
    }
}
