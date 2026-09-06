use std::sync::Arc;

use arc_swap::ArcSwap;

use crate::{
    ControlEpoch, DeskPointUm, DeskVectorUm, DisplayId, PortalResolver, RouteOutcome,
    RoutingContext, TopologyError, TopologyRevision, WorkspaceLayout,
};

#[derive(Debug, Clone)]
pub struct RuntimeRoutingSnapshot {
    pub topology_revision: TopologyRevision,
    pub layout: WorkspaceLayout,
}

impl RuntimeRoutingSnapshot {
    pub fn compile(layout: WorkspaceLayout) -> Result<Self, TopologyError> {
        layout.validate()?;
        Ok(Self {
            topology_revision: layout.revision,
            layout,
        })
    }
}

#[derive(Debug)]
pub struct RuntimeRouter {
    snapshot: ArcSwap<RuntimeRoutingSnapshot>,
}

impl RuntimeRouter {
    pub fn new(snapshot: RuntimeRoutingSnapshot) -> Self {
        Self {
            snapshot: ArcSwap::from_pointee(snapshot),
        }
    }

    pub fn replace(&self, snapshot: RuntimeRoutingSnapshot) {
        self.snapshot.store(Arc::new(snapshot));
    }

    pub fn revision(&self) -> TopologyRevision {
        self.snapshot.load().topology_revision
    }

    pub fn snapshot(&self) -> Arc<RuntimeRoutingSnapshot> {
        self.snapshot.load_full()
    }

    pub fn route(
        &self,
        revision: TopologyRevision,
        display_id: DisplayId,
        point: DeskPointUm,
        movement: DeskVectorUm,
        context: RoutingContext,
    ) -> Result<RouteOutcome, RuntimeRouteError> {
        let snapshot = self.snapshot.load();
        if snapshot.topology_revision != revision {
            return Err(RuntimeRouteError::StaleTopology {
                expected: snapshot.topology_revision,
                actual: revision,
            });
        }
        PortalResolver::route(&snapshot.layout, display_id, point, movement, context)
            .map_err(RuntimeRouteError::Topology)
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RuntimeRouteError {
    #[error("stale topology: expected {expected:?}, got {actual:?}")]
    StaleTopology {
        expected: TopologyRevision,
        actual: TopologyRevision,
    },
    #[error(transparent)]
    Topology(#[from] TopologyError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeFence {
    pub topology_revision: TopologyRevision,
    pub control_epoch: ControlEpoch,
}

impl RuntimeFence {
    pub fn accepts(self, topology_revision: TopologyRevision, control_epoch: ControlEpoch) -> bool {
        self.topology_revision == topology_revision && self.control_epoch == control_epoch
    }
}
