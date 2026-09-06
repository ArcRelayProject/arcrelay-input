use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};

use arcrelay_peer::ServiceInstanceId;
use serde::{Deserialize, Serialize};

use crate::{ControlEpoch, DisplayId, RuntimeHeader, TopologyRevision, WorkspaceId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlGrant {
    pub workspace_id: WorkspaceId,
    pub controller: ServiceInstanceId,
    pub epoch: ControlEpoch,
}

#[derive(Debug, Default)]
struct ArbiterState {
    next_epoch: u64,
    active: Option<ControlGrant>,
}

/// Workspace-global exclusive controller arbitration.
#[derive(Debug, Clone, Default)]
pub struct ControlArbiter {
    state: Arc<Mutex<ArbiterState>>,
}

impl ControlArbiter {
    pub fn acquire(
        &self,
        workspace_id: WorkspaceId,
        controller: ServiceInstanceId,
    ) -> Result<ControlGrant, ControlError> {
        let mut state = lock(&self.state);
        if let Some(active) = &state.active {
            if active.workspace_id == workspace_id && active.controller == controller {
                return Ok(active.clone());
            }
            return Err(ControlError::AlreadyControlled {
                controller: active.controller.clone(),
                epoch: active.epoch,
            });
        }
        state.next_epoch = state.next_epoch.saturating_add(1).max(1);
        let grant = ControlGrant {
            workspace_id,
            controller,
            epoch: ControlEpoch(state.next_epoch),
        };
        state.active = Some(grant.clone());
        Ok(grant)
    }

    /// Atomically transfers ownership only when the caller presents the live
    /// epoch. This is the compare-and-swap boundary for user takeover.
    pub fn transfer(
        &self,
        expected_epoch: ControlEpoch,
        next_controller: ServiceInstanceId,
    ) -> Result<ControlGrant, ControlError> {
        let mut state = lock(&self.state);
        let active = state.active.as_ref().ok_or(ControlError::NotControlled)?;
        if active.epoch != expected_epoch {
            return Err(ControlError::StaleEpoch {
                expected: active.epoch,
                actual: expected_epoch,
            });
        }
        let workspace_id = active.workspace_id.clone();
        state.next_epoch = state.next_epoch.saturating_add(1).max(1);
        let grant = ControlGrant {
            workspace_id,
            controller: next_controller,
            epoch: ControlEpoch(state.next_epoch),
        };
        state.active = Some(grant.clone());
        Ok(grant)
    }

    pub fn release(&self, controller: &ServiceInstanceId, epoch: ControlEpoch) -> bool {
        let mut state = lock(&self.state);
        if state
            .active
            .as_ref()
            .is_some_and(|active| &active.controller == controller && active.epoch == epoch)
        {
            state.active = None;
            true
        } else {
            false
        }
    }

    pub fn revoke(&self) -> Option<ControlGrant> {
        lock(&self.state).active.take()
    }

    pub fn current(&self) -> Option<ControlGrant> {
        lock(&self.state).active.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    Local,
    PreparingTarget,
    Remote,
    PreparingNextTarget,
    Recovering,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeldInputState {
    pub held_physical_keys: BTreeSet<u16>,
    pub held_mouse_buttons: BTreeSet<u16>,
    pub modifier_state: u32,
    pub caps_lock: bool,
    pub num_lock: bool,
    pub scroll_lock: bool,
    pub keyboard_profile_revision: u64,
}

impl HeldInputState {
    pub fn is_empty(&self) -> bool {
        self.held_physical_keys.is_empty()
            && self.held_mouse_buttons.is_empty()
            && self.modifier_state == 0
    }

    pub fn checksum(&self) -> u64 {
        let mut checksum = 0xcbf2_9ce4_8422_2325_u64;
        for value in self
            .held_physical_keys
            .iter()
            .copied()
            .map(|value| u64::from(value) | 1 << 16)
            .chain(
                self.held_mouse_buttons
                    .iter()
                    .copied()
                    .map(|value| u64::from(value) | 1 << 24),
            )
            .chain([
                u64::from(self.modifier_state),
                self.keyboard_profile_revision,
                u64::from(self.caps_lock)
                    | (u64::from(self.num_lock) << 1)
                    | (u64::from(self.scroll_lock) << 2),
            ])
        {
            checksum ^= value;
            checksum = checksum.wrapping_mul(0x100_0000_01b3);
        }
        checksum
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlSession {
    pub workspace_id: WorkspaceId,
    pub controller: ServiceInstanceId,
    pub current_target: ServiceInstanceId,
    pub current_display: DisplayId,
    pub topology_revision: TopologyRevision,
    pub control_epoch: ControlEpoch,
    pub next_sequence: u64,
    pub state: SessionState,
    pub held: HeldInputState,
}

impl ControlSession {
    pub fn validate_header(&mut self, header: &RuntimeHeader) -> Result<(), ControlError> {
        if header.workspace_id != self.workspace_id {
            return Err(ControlError::WrongWorkspace);
        }
        if header.topology_revision != self.topology_revision {
            return Err(ControlError::StaleTopology {
                expected: self.topology_revision,
                actual: header.topology_revision,
            });
        }
        if header.control_epoch != self.control_epoch {
            return Err(ControlError::StaleEpoch {
                expected: self.control_epoch,
                actual: header.control_epoch,
            });
        }
        if header.source_device_id != self.controller {
            return Err(ControlError::WrongSource);
        }
        if header.target_device_id != self.current_target {
            return Err(ControlError::WrongTarget);
        }
        if header.source_device_id == header.target_device_id {
            return Err(ControlError::ForwardingForbidden);
        }
        if header.sequence != self.next_sequence {
            return Err(ControlError::OutOfOrder {
                expected: self.next_sequence,
                actual: header.sequence,
            });
        }
        self.next_sequence = self.next_sequence.saturating_add(1);
        Ok(())
    }

    pub fn begin_handoff(&mut self) -> Result<(), ControlError> {
        if self.state != SessionState::Local && self.state != SessionState::Remote {
            return Err(ControlError::InvalidSessionTransition);
        }
        self.state = if self.state == SessionState::Local {
            SessionState::PreparingTarget
        } else {
            SessionState::PreparingNextTarget
        };
        Ok(())
    }

    pub fn commit_handoff(
        &mut self,
        target: ServiceInstanceId,
        display: DisplayId,
    ) -> Result<Vec<HandoffAction>, ControlError> {
        if !matches!(
            self.state,
            SessionState::PreparingTarget | SessionState::PreparingNextTarget
        ) {
            return Err(ControlError::InvalidSessionTransition);
        }
        let actions = vec![
            HandoffAction::ReleaseAll {
                target: self.current_target.clone(),
            },
            HandoffAction::PlacePointer {
                target: target.clone(),
                display: display.clone(),
            },
            HandoffAction::RestoreModifiers {
                target: target.clone(),
                modifier_state: self.held.modifier_state,
            },
        ];
        self.current_target = target;
        self.current_display = display;
        self.state = SessionState::Remote;
        Ok(actions)
    }

    pub fn recover(&mut self) -> Vec<HandoffAction> {
        self.state = SessionState::Recovering;
        let target = self.current_target.clone();
        self.held.clear();
        self.state = SessionState::Local;
        vec![HandoffAction::ReleaseAll { target }]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffAction {
    ReleaseAll {
        target: ServiceInstanceId,
    },
    PlacePointer {
        target: ServiceInstanceId,
        display: DisplayId,
    },
    RestoreModifiers {
        target: ServiceInstanceId,
        modifier_state: u32,
    },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ControlError {
    #[error("workspace is already controlled by {controller} at epoch {epoch:?}")]
    AlreadyControlled {
        controller: ServiceInstanceId,
        epoch: ControlEpoch,
    },
    #[error("workspace is not controlled")]
    NotControlled,
    #[error("cross-screen input is disabled")]
    InputSharingDisabled,
    #[error("online screen component has changed; retry after topology synchronization")]
    ComponentChanged,
    #[error("wrong workspace")]
    WrongWorkspace,
    #[error("stale topology revision: expected {expected:?}, got {actual:?}")]
    StaleTopology {
        expected: TopologyRevision,
        actual: TopologyRevision,
    },
    #[error("stale control epoch: expected {expected:?}, got {actual:?}")]
    StaleEpoch {
        expected: ControlEpoch,
        actual: ControlEpoch,
    },
    #[error("wrong input source")]
    WrongSource,
    #[error("wrong input target")]
    WrongTarget,
    #[error("input forwarding is forbidden")]
    ForwardingForbidden,
    #[error("out-of-order input: expected {expected}, got {actual}")]
    OutOfOrder { expected: u64, actual: u64 },
    #[error("invalid control-session transition")]
    InvalidSessionTransition,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(value: &str) -> ServiceInstanceId {
        ServiceInstanceId::parse(value).unwrap()
    }

    #[test]
    fn simultaneous_acquire_has_one_winner() {
        let arbiter = ControlArbiter::default();
        let workspace = WorkspaceId::parse("desk").unwrap();
        let first = arbiter.acquire(workspace.clone(), device("a")).unwrap();
        let second = arbiter.acquire(workspace, device("b"));
        assert!(matches!(
            second,
            Err(ControlError::AlreadyControlled { .. })
        ));
        assert_eq!(arbiter.current(), Some(first));
    }

    #[test]
    fn transfer_is_compare_and_swap() {
        let arbiter = ControlArbiter::default();
        let first = arbiter
            .acquire(WorkspaceId::parse("desk").unwrap(), device("a"))
            .unwrap();
        assert!(matches!(
            arbiter.transfer(ControlEpoch(first.epoch.0 + 1), device("b")),
            Err(ControlError::StaleEpoch { .. })
        ));
        let second = arbiter.transfer(first.epoch, device("b")).unwrap();
        assert!(second.epoch > first.epoch);
        assert_eq!(second.controller, device("b"));
    }

    #[test]
    fn recovery_always_clears_held_state() {
        let mut held = HeldInputState::default();
        held.held_physical_keys.insert(0x04);
        held.held_mouse_buttons.insert(1);
        let mut session = ControlSession {
            workspace_id: WorkspaceId::parse("desk").unwrap(),
            controller: device("a"),
            current_target: device("b"),
            current_display: DisplayId::parse("display-b").unwrap(),
            topology_revision: TopologyRevision(1),
            control_epoch: ControlEpoch(1),
            next_sequence: 1,
            state: SessionState::Remote,
            held,
        };
        assert_eq!(session.recover().len(), 1);
        assert!(session.held.is_empty());
        assert_eq!(session.state, SessionState::Local);
    }

    #[test]
    fn rejects_stale_topology_epoch_and_out_of_order_frames() {
        let mut session = ControlSession {
            workspace_id: WorkspaceId::parse("desk").unwrap(),
            controller: device("controller"),
            current_target: device("target"),
            current_display: DisplayId::parse("display-target").unwrap(),
            topology_revision: TopologyRevision(7),
            control_epoch: ControlEpoch(9),
            next_sequence: 4,
            state: SessionState::Remote,
            held: HeldInputState::default(),
        };
        let mut header = RuntimeHeader {
            workspace_id: WorkspaceId::parse("desk").unwrap(),
            topology_revision: TopologyRevision(6),
            control_epoch: ControlEpoch(9),
            source_device_id: device("controller"),
            target_device_id: device("target"),
            sequence: 4,
        };
        assert!(matches!(
            session.validate_header(&header),
            Err(ControlError::StaleTopology { .. })
        ));
        header.topology_revision = TopologyRevision(7);
        header.control_epoch = ControlEpoch(8);
        assert!(matches!(
            session.validate_header(&header),
            Err(ControlError::StaleEpoch { .. })
        ));
        header.control_epoch = ControlEpoch(9);
        header.sequence = 5;
        assert!(matches!(
            session.validate_header(&header),
            Err(ControlError::OutOfOrder { .. })
        ));
        header.sequence = 4;
        assert!(session.validate_header(&header).is_ok());
        assert_eq!(session.next_sequence, 5);
    }
}
