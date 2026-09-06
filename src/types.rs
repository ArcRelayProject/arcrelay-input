use std::fmt;

use arcrelay_peer::ServiceInstanceId;
use serde::{Deserialize, Serialize};

macro_rules! string_id {
    ($name:ident) => {
        #[derive(
            Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ts_rs::TS,
        )]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, IdentifierError> {
                let value = value.into();
                let value = value.trim();
                if value.is_empty() {
                    return Err(IdentifierError::Empty);
                }
                if value.len() > 128 {
                    return Err(IdentifierError::TooLong);
                }
                Ok(Self(value.to_string()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

string_id!(WorkspaceId);
string_id!(DisplayId);
string_id!(PortalId);
string_id!(DisplayFingerprint);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IdentifierError {
    #[error("identifier cannot be empty")]
    Empty,
    #[error("identifier is too long")]
    TooLong,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    ts_rs::TS,
)]
#[serde(transparent)]
pub struct TopologyRevision(pub u64);

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    ts_rs::TS,
)]
#[serde(transparent)]
pub struct InventoryRevision(pub u64);

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct ControlEpoch(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeHeader {
    pub workspace_id: WorkspaceId,
    pub topology_revision: TopologyRevision,
    pub control_epoch: ControlEpoch,
    pub source_device_id: ServiceInstanceId,
    pub target_device_id: ServiceInstanceId,
    pub sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub enum OsFamily {
    Windows,
    MacOs,
    LinuxX11,
    LinuxWayland,
    Android,
    Ios,
    Unknown,
}
