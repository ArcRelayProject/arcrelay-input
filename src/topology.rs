use std::collections::{BTreeMap, BTreeSet, VecDeque};

use arcrelay_peer::ServiceInstanceId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{DisplayId, DisplaySurface, PortalId, TopologyRevision, WorkspaceId};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeskPointUm {
    pub x: i64,
    pub y: i64,
}

impl DeskPointUm {
    fn checked_add(self, delta: DeskVectorUm) -> Result<Self, TopologyError> {
        Ok(Self {
            x: self.x.checked_add(delta.x).ok_or(TopologyError::Overflow)?,
            y: self.y.checked_add(delta.y).ok_or(TopologyError::Overflow)?,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeskVectorUm {
    pub x: i64,
    pub y: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub struct DeskRectUm {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

impl DeskRectUm {
    pub fn validate(self) -> Result<(), TopologyError> {
        if self.width <= 0 || self.height <= 0 {
            return Err(TopologyError::EmptyRect);
        }
        self.x
            .checked_add(self.width)
            .ok_or(TopologyError::Overflow)?;
        self.y
            .checked_add(self.height)
            .ok_or(TopologyError::Overflow)?;
        Ok(())
    }

    pub fn contains(self, point: DeskPointUm) -> bool {
        point.x >= self.x
            && point.x <= self.x.saturating_add(self.width)
            && point.y >= self.y
            && point.y <= self.y.saturating_add(self.height)
    }

    fn clamp(self, point: DeskPointUm, inset: i64) -> DeskPointUm {
        let inset_x = inset.max(0).min(self.width / 2);
        let inset_y = inset.max(0).min(self.height / 2);
        DeskPointUm {
            x: point.x.clamp(
                self.x.saturating_add(inset_x),
                self.x.saturating_add(self.width).saturating_sub(inset_x),
            ),
            y: point.y.clamp(
                self.y.saturating_add(inset_y),
                self.y.saturating_add(self.height).saturating_sub(inset_y),
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct LogicalPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, ts_rs::TS)]
pub struct LogicalRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct EdgeSegment {
    #[serde(alias = "start_um")]
    pub start_um: i64,
    #[serde(alias = "end_um")]
    pub end_um: i64,
}

pub const DEFAULT_PORTAL_ADJACENCY_TOLERANCE_UM: i64 = 25_000;
pub const DEFAULT_MINIMUM_PORTAL_SPAN_UM: i64 = 5_000;

impl EdgeSegment {
    pub fn length(self) -> i64 {
        self.end_um.saturating_sub(self.start_um)
    }

    pub fn contains(self, value: f64) -> bool {
        value >= self.start_um as f64 && value <= self.end_um as f64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub enum PortalDirection {
    OneWay,
    Bidirectional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub enum ActivationPolicy {
    Immediate,
    RequireModifier { hid_usage: u16 },
    Dwell { milliseconds: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub enum PortalStatus {
    Active,
    SuspendedOffline,
    NeedsReconciliation,
    UnsupportedCapability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct Portal {
    pub portal_id: PortalId,
    pub source_display: DisplayId,
    pub source_edge: Edge,
    pub source_segment: EdgeSegment,
    pub target_display: DisplayId,
    pub target_edge: Edge,
    pub target_segment: EdgeSegment,
    pub direction: PortalDirection,
    pub activation_policy: ActivationPolicy,
    pub allow_while_dragging: bool,
    pub inset_um: i64,
    pub hysteresis_um: i64,
    pub status: PortalStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceLayout {
    pub workspace_id: WorkspaceId,
    pub revision: TopologyRevision,
    pub displays: BTreeMap<DisplayId, DisplaySurface>,
    pub portals: Vec<Portal>,
}

impl WorkspaceLayout {
    pub fn validate(&self) -> Result<(), TopologyError> {
        if self.revision.0 == 0 {
            return Err(TopologyError::ZeroRevision);
        }
        for display in self.displays.values() {
            display.desk_rect_um.validate()?;
        }
        let mut ids = BTreeSet::new();
        for portal in &self.portals {
            if !ids.insert(portal.portal_id.clone()) {
                return Err(TopologyError::DuplicatePortal(portal.portal_id.clone()));
            }
            let source = self
                .displays
                .get(&portal.source_display)
                .ok_or_else(|| TopologyError::MissingDisplay(portal.source_display.clone()))?;
            let target = self
                .displays
                .get(&portal.target_display)
                .ok_or_else(|| TopologyError::MissingDisplay(portal.target_display.clone()))?;
            if portal.source_display == portal.target_display {
                return Err(TopologyError::SelfPortal(portal.portal_id.clone()));
            }
            validate_segment(
                portal.portal_id.clone(),
                portal.source_segment,
                edge_length(source.desk_rect_um, portal.source_edge),
            )?;
            validate_segment(
                portal.portal_id.clone(),
                portal.target_segment,
                edge_length(target.desk_rect_um, portal.target_edge),
            )?;
            if portal.inset_um < 0 || portal.hysteresis_um < 0 {
                return Err(TopologyError::NegativePolicyDistance(
                    portal.portal_id.clone(),
                ));
            }
        }
        Ok(())
    }
}

fn validate_segment(
    portal: PortalId,
    segment: EdgeSegment,
    maximum: i64,
) -> Result<(), TopologyError> {
    if segment.start_um < 0 || segment.end_um > maximum || segment.length() <= 0 {
        return Err(TopologyError::InvalidSegment(portal));
    }
    Ok(())
}

fn edge_length(rect: DeskRectUm, edge: Edge) -> i64 {
    match edge {
        Edge::Left | Edge::Right => rect.height,
        Edge::Top | Edge::Bottom => rect.width,
    }
}

/// Compile bidirectional portals from the physical overlap of nearby displays.
///
/// Existing portals are used only as policy overrides. Their endpoints and
/// segments are always replaced by the geometry-derived values.
/// Ownership spans only the connected component of usable screen edges.
/// Same-device displays share an owner; disconnected/offline islands do not.
pub fn active_component_devices(
    layout: &WorkspaceLayout,
    local: &ServiceInstanceId,
) -> BTreeSet<ServiceInstanceId> {
    let mut devices = BTreeSet::new();
    if layout
        .displays
        .values()
        .any(|display| &display.device_id == local)
    {
        devices.insert(local.clone());
    }
    loop {
        let previous = devices.len();
        for portal in &layout.portals {
            if portal.status != PortalStatus::Active {
                continue;
            }
            let (Some(source), Some(target)) = (
                layout.displays.get(&portal.source_display),
                layout.displays.get(&portal.target_display),
            ) else {
                continue;
            };
            if devices.contains(&source.device_id) || devices.contains(&target.device_id) {
                devices.insert(source.device_id.clone());
                devices.insert(target.device_id.clone());
            }
        }
        if devices.len() == previous {
            return devices;
        }
    }
}

/// Keep every device's displays in the relative arrangement reported by its
/// operating system while preserving the existing position of a stable anchor
/// display. Physical dimensions remain user-calibrated; native edge segments
/// provide the constraints between differently scaled or rotated displays.
pub fn align_system_display_groups(
    displays: &mut BTreeMap<DisplayId, DisplaySurface>,
) -> Result<(), TopologyError> {
    let devices = displays
        .values()
        .map(|display| display.device_id.clone())
        .collect::<BTreeSet<_>>();
    for device in devices {
        align_system_display_group(displays, &device)?;
    }
    Ok(())
}

pub fn align_system_display_group(
    displays: &mut BTreeMap<DisplayId, DisplaySurface>,
    device: &ServiceInstanceId,
) -> Result<(), TopologyError> {
    let ids = displays
        .values()
        .filter(|display| &display.device_id == device)
        .map(|display| display.display_id.clone())
        .collect::<Vec<_>>();
    if ids.len() < 2 {
        return Ok(());
    }
    let candidates = {
        let values = ids
            .iter()
            .filter_map(|id| displays.get(id))
            .collect::<Vec<_>>();
        let mut candidates = Vec::new();
        for left_index in 0..values.len() {
            for right_index in (left_index + 1)..values.len() {
                if let Some(candidate) =
                    best_system_adjacency(values[left_index], values[right_index])
                {
                    candidates.push(candidate);
                }
            }
        }
        candidates
    };
    // Prefer the group's existing top-left DeskSpace member. Newly discovered
    // displays are appended by reconciliation, so they cannot unexpectedly
    // become the anchor merely because their persistent ID sorts first.
    let anchor = ids
        .iter()
        .min_by_key(|id| {
            let display = &displays[*id];
            (
                display.desk_rect_um.x,
                display.desk_rect_um.y,
                display.display_id.clone(),
            )
        })
        .cloned()
        .expect("display group with at least two members has an anchor");
    let anchor_rect = displays
        .get(&anchor)
        .ok_or_else(|| TopologyError::MissingDisplay(anchor.clone()))?
        .desk_rect_um;
    let mut positions = BTreeMap::from([(anchor.clone(), (anchor_rect.x, anchor_rect.y))]);
    let mut queue = VecDeque::from([anchor]);
    while let Some(source_id) = queue.pop_front() {
        for candidate in &candidates {
            let relation = if candidate.source_display == source_id {
                Some((
                    &candidate.source_display,
                    candidate.source_edge,
                    candidate.source_segment,
                    &candidate.target_display,
                    candidate.target_edge,
                    candidate.target_segment,
                ))
            } else if candidate.target_display == source_id {
                Some((
                    &candidate.target_display,
                    candidate.target_edge,
                    candidate.target_segment,
                    &candidate.source_display,
                    candidate.source_edge,
                    candidate.source_segment,
                ))
            } else {
                None
            };
            let Some((
                source_id,
                source_edge,
                source_segment,
                target_id,
                target_edge,
                target_segment,
            )) = relation
            else {
                continue;
            };
            if positions.contains_key(target_id) {
                continue;
            }
            let source = displays
                .get(source_id)
                .ok_or_else(|| TopologyError::MissingDisplay(source_id.clone()))?;
            let target = displays
                .get(target_id)
                .ok_or_else(|| TopologyError::MissingDisplay(target_id.clone()))?;
            let (source_x, source_y) = positions[source_id];
            let source_point = edge_segment_start(
                DeskRectUm {
                    x: source_x,
                    y: source_y,
                    ..source.desk_rect_um
                },
                source_edge,
                source_segment,
            );
            let target_offset = edge_segment_start(
                DeskRectUm {
                    x: 0,
                    y: 0,
                    ..target.desk_rect_um
                },
                target_edge,
                target_segment,
            );
            positions.insert(
                target_id.clone(),
                (
                    source_point.x.saturating_sub(target_offset.x),
                    source_point.y.saturating_sub(target_offset.y),
                ),
            );
            queue.push_back(target_id.clone());
        }
    }
    for (id, (x, y)) in positions {
        let display = displays
            .get_mut(&id)
            .ok_or_else(|| TopologyError::MissingDisplay(id.clone()))?;
        display.desk_rect_um.x = x;
        display.desk_rect_um.y = y;
    }
    Ok(())
}

fn edge_segment_start(rect: DeskRectUm, edge: Edge, segment: EdgeSegment) -> DeskPointUm {
    match edge {
        Edge::Left => DeskPointUm {
            x: rect.x,
            y: rect.y.saturating_add(segment.start_um),
        },
        Edge::Right => DeskPointUm {
            x: rect.x.saturating_add(rect.width),
            y: rect.y.saturating_add(segment.start_um),
        },
        Edge::Top => DeskPointUm {
            x: rect.x.saturating_add(segment.start_um),
            y: rect.y,
        },
        Edge::Bottom => DeskPointUm {
            x: rect.x.saturating_add(segment.start_um),
            y: rect.y.saturating_add(rect.height),
        },
    }
}

pub fn derive_auto_portals(
    displays: &BTreeMap<DisplayId, DisplaySurface>,
    existing: &[Portal],
    adjacency_tolerance_um: i64,
    minimum_span_um: i64,
) -> Result<Vec<Portal>, TopologyError> {
    let adjacency_tolerance_um = adjacency_tolerance_um.max(0);
    let minimum_span_um = minimum_span_um.max(1);
    for display in displays.values() {
        display.desk_rect_um.validate()?;
    }

    let values = displays.values().collect::<Vec<_>>();
    let mut native_candidates = Vec::new();
    let mut remote_candidates = Vec::new();
    for left_index in 0..values.len() {
        for right_index in (left_index + 1)..values.len() {
            let left = values[left_index];
            let right = values[right_index];
            if left.device_id == right.device_id {
                if let Some(candidate) = best_system_adjacency(left, right) {
                    native_candidates.push(candidate);
                }
            } else if let Some(candidate) =
                best_adjacency(left, right, adjacency_tolerance_um, minimum_span_um)
            {
                remote_candidates.push(candidate);
            }
        }
    }

    // An edge segment already used by the operating system to move between
    // two displays on the same device cannot also activate a remote portal in
    // the default native-layout mode. Omitting the ambiguous remote portal is
    // safer than letting an implementation-specific portal identifier decide
    // whether the system or Arc Input wins the crossing.
    remote_candidates
        .retain(|candidate| !candidate_conflicts_with_native(candidate, &native_candidates));

    let mut portals = native_candidates
        .iter()
        .chain(&remote_candidates)
        .map(|candidate| portal_from_candidate(candidate, existing))
        .collect::<Result<Vec<_>, _>>()?;
    portals.sort_by(|left, right| left.portal_id.cmp(&right.portal_id));
    Ok(portals)
}

fn portal_from_candidate(
    candidate: &AdjacencyCandidate,
    existing: &[Portal],
) -> Result<Portal, TopologyError> {
    let prior = existing.iter().find(|portal| {
        portal_matches(
            portal,
            &candidate.source_display,
            candidate.source_edge,
            &candidate.target_display,
            candidate.target_edge,
        )
    });
    Ok(Portal {
        portal_id: generated_portal_id(candidate)?,
        source_display: candidate.source_display.clone(),
        source_edge: candidate.source_edge,
        source_segment: candidate.source_segment,
        target_display: candidate.target_display.clone(),
        target_edge: candidate.target_edge,
        target_segment: candidate.target_segment,
        direction: PortalDirection::Bidirectional,
        activation_policy: prior.map_or(ActivationPolicy::Immediate, |portal| {
            portal.activation_policy
        }),
        allow_while_dragging: prior.is_some_and(|portal| portal.allow_while_dragging),
        inset_um: prior.map_or(500, |portal| portal.inset_um.max(0)),
        hysteresis_um: prior.map_or(500, |portal| portal.hysteresis_um.max(0)),
        status: prior.map_or(PortalStatus::Active, |portal| portal.status),
    })
}

#[derive(Debug)]
struct AdjacencyCandidate {
    source_display: DisplayId,
    source_edge: Edge,
    source_segment: EdgeSegment,
    target_display: DisplayId,
    target_edge: Edge,
    target_segment: EdgeSegment,
    normal_gap_um: i64,
}

fn best_adjacency(
    left: &DisplaySurface,
    right: &DisplaySurface,
    tolerance: i64,
    minimum_span: i64,
) -> Option<AdjacencyCandidate> {
    let a = left.desk_rect_um;
    let b = right.desk_rect_um;
    let mut candidates = Vec::with_capacity(4);

    if let Some((overlap_start, overlap_end)) = interval_overlap(a.y, a.height, b.y, b.height) {
        let span = overlap_end.saturating_sub(overlap_start);
        if span >= minimum_span {
            push_candidate(
                &mut candidates,
                left,
                Edge::Right,
                overlap_start.saturating_sub(a.y),
                overlap_end.saturating_sub(a.y),
                right,
                Edge::Left,
                overlap_start.saturating_sub(b.y),
                overlap_end.saturating_sub(b.y),
                coordinate_distance(a.x.saturating_add(a.width), b.x),
                tolerance,
            );
            push_candidate(
                &mut candidates,
                right,
                Edge::Right,
                overlap_start.saturating_sub(b.y),
                overlap_end.saturating_sub(b.y),
                left,
                Edge::Left,
                overlap_start.saturating_sub(a.y),
                overlap_end.saturating_sub(a.y),
                coordinate_distance(b.x.saturating_add(b.width), a.x),
                tolerance,
            );
        }
    }

    if let Some((overlap_start, overlap_end)) = interval_overlap(a.x, a.width, b.x, b.width) {
        let span = overlap_end.saturating_sub(overlap_start);
        if span >= minimum_span {
            push_candidate(
                &mut candidates,
                left,
                Edge::Bottom,
                overlap_start.saturating_sub(a.x),
                overlap_end.saturating_sub(a.x),
                right,
                Edge::Top,
                overlap_start.saturating_sub(b.x),
                overlap_end.saturating_sub(b.x),
                coordinate_distance(a.y.saturating_add(a.height), b.y),
                tolerance,
            );
            push_candidate(
                &mut candidates,
                right,
                Edge::Bottom,
                overlap_start.saturating_sub(b.x),
                overlap_end.saturating_sub(b.x),
                left,
                Edge::Top,
                overlap_start.saturating_sub(a.x),
                overlap_end.saturating_sub(a.x),
                coordinate_distance(b.y.saturating_add(b.height), a.y),
                tolerance,
            );
        }
    }

    candidates.into_iter().min_by(|left, right| {
        left.normal_gap_um
            .cmp(&right.normal_gap_um)
            .then_with(|| left.source_display.cmp(&right.source_display))
            .then_with(|| edge_rank(left.source_edge).cmp(&edge_rank(right.source_edge)))
    })
}

/// Derive the transition between two displays on one operating-system
/// desktop. System coordinates are device-local and never compared across
/// devices; the resulting edge segments are mapped into each display's
/// calibrated DeskSpace dimensions.
fn best_system_adjacency(
    left: &DisplaySurface,
    right: &DisplaySurface,
) -> Option<AdjacencyCandidate> {
    let a = left.logical_bounds;
    let b = right.logical_bounds;
    if ![a.x, a.y, a.width, a.height, b.x, b.y, b.width, b.height]
        .into_iter()
        .all(f64::is_finite)
    {
        return None;
    }
    let mut candidates = Vec::with_capacity(4);

    if let Some((overlap_start, overlap_end)) = interval_overlap_f64(a.y, a.height, b.y, b.height) {
        push_system_candidate(
            &mut candidates,
            left,
            Edge::Right,
            overlap_start,
            overlap_end,
            right,
            Edge::Left,
            overlap_start,
            overlap_end,
            (a.x + a.width - b.x).abs(),
        );
        push_system_candidate(
            &mut candidates,
            right,
            Edge::Right,
            overlap_start,
            overlap_end,
            left,
            Edge::Left,
            overlap_start,
            overlap_end,
            (b.x + b.width - a.x).abs(),
        );
    }

    if let Some((overlap_start, overlap_end)) = interval_overlap_f64(a.x, a.width, b.x, b.width) {
        push_system_candidate(
            &mut candidates,
            left,
            Edge::Bottom,
            overlap_start,
            overlap_end,
            right,
            Edge::Top,
            overlap_start,
            overlap_end,
            (a.y + a.height - b.y).abs(),
        );
        push_system_candidate(
            &mut candidates,
            right,
            Edge::Bottom,
            overlap_start,
            overlap_end,
            left,
            Edge::Top,
            overlap_start,
            overlap_end,
            (b.y + b.height - a.y).abs(),
        );
    }

    candidates.into_iter().min_by(|left, right| {
        left.normal_gap_um
            .cmp(&right.normal_gap_um)
            .then_with(|| left.source_display.cmp(&right.source_display))
            .then_with(|| edge_rank(left.source_edge).cmp(&edge_rank(right.source_edge)))
    })
}

#[allow(clippy::too_many_arguments)]
fn push_system_candidate(
    candidates: &mut Vec<AdjacencyCandidate>,
    source: &DisplaySurface,
    source_logical_edge: Edge,
    source_overlap_start: f64,
    source_overlap_end: f64,
    target: &DisplaySurface,
    target_logical_edge: Edge,
    target_overlap_start: f64,
    target_overlap_end: f64,
    gap: f64,
) {
    const SYSTEM_EDGE_TOLERANCE: f64 = 1.0;
    if gap > SYSTEM_EDGE_TOLERANCE {
        return;
    }
    let Some((source_edge, source_segment)) = system_edge_segment(
        source,
        source_logical_edge,
        source_overlap_start,
        source_overlap_end,
    ) else {
        return;
    };
    let Some((target_edge, target_segment)) = system_edge_segment(
        target,
        target_logical_edge,
        target_overlap_start,
        target_overlap_end,
    ) else {
        return;
    };
    candidates.push(AdjacencyCandidate {
        source_display: source.display_id.clone(),
        source_edge,
        source_segment,
        target_display: target.display_id.clone(),
        target_edge,
        target_segment,
        normal_gap_um: (gap * 1000.0).round() as i64,
    });
}

fn system_edge_segment(
    display: &DisplaySurface,
    logical_edge: Edge,
    overlap_start: f64,
    overlap_end: f64,
) -> Option<(Edge, EdgeSegment)> {
    let bounds = display.logical_bounds;
    let (axis_start, axis_length) = match logical_edge {
        Edge::Left | Edge::Right => (bounds.y, bounds.height),
        Edge::Top | Edge::Bottom => (bounds.x, bounds.width),
    };
    if axis_length <= 0.0 || overlap_end <= overlap_start {
        return None;
    }
    let (desk_edge, reversed) = logical_to_desk_edge(display.rotation, logical_edge);
    let desk_length = edge_length(display.desk_rect_um, desk_edge);
    let start = ((overlap_start - axis_start) / axis_length).clamp(0.0, 1.0);
    let end = ((overlap_end - axis_start) / axis_length).clamp(0.0, 1.0);
    let (start, end) = if reversed {
        (1.0 - end, 1.0 - start)
    } else {
        (start, end)
    };
    let start_um = (start * desk_length as f64)
        .round()
        .clamp(0.0, desk_length.saturating_sub(1) as f64) as i64;
    let end_um = (end * desk_length as f64)
        .round()
        .clamp(start_um.saturating_add(1) as f64, desk_length as f64) as i64;
    Some((desk_edge, EdgeSegment { start_um, end_um }))
}

fn logical_to_desk_edge(rotation: crate::DisplayRotation, edge: Edge) -> (Edge, bool) {
    use crate::DisplayRotation::{Degrees0, Degrees180, Degrees270, Degrees90};
    match (rotation, edge) {
        (Degrees0, edge) => (edge, false),
        (Degrees90, Edge::Left) => (Edge::Top, true),
        (Degrees90, Edge::Right) => (Edge::Bottom, true),
        (Degrees90, Edge::Top) => (Edge::Right, false),
        (Degrees90, Edge::Bottom) => (Edge::Left, false),
        (Degrees180, Edge::Left) => (Edge::Right, true),
        (Degrees180, Edge::Right) => (Edge::Left, true),
        (Degrees180, Edge::Top) => (Edge::Bottom, true),
        (Degrees180, Edge::Bottom) => (Edge::Top, true),
        (Degrees270, Edge::Left) => (Edge::Bottom, false),
        (Degrees270, Edge::Right) => (Edge::Top, false),
        (Degrees270, Edge::Top) => (Edge::Left, true),
        (Degrees270, Edge::Bottom) => (Edge::Right, true),
    }
}

fn interval_overlap_f64(
    left_start: f64,
    left_length: f64,
    right_start: f64,
    right_length: f64,
) -> Option<(f64, f64)> {
    let start = left_start.max(right_start);
    let end = (left_start + left_length).min(right_start + right_length);
    (end > start).then_some((start, end))
}

fn candidate_conflicts_with_native(
    candidate: &AdjacencyCandidate,
    native: &[AdjacencyCandidate],
) -> bool {
    native.iter().any(|reserved| {
        candidate_endpoint_overlaps(
            &candidate.source_display,
            candidate.source_edge,
            candidate.source_segment,
            reserved,
        ) || candidate_endpoint_overlaps(
            &candidate.target_display,
            candidate.target_edge,
            candidate.target_segment,
            reserved,
        )
    })
}

fn candidate_endpoint_overlaps(
    display: &DisplayId,
    edge: Edge,
    segment: EdgeSegment,
    reserved: &AdjacencyCandidate,
) -> bool {
    (display == &reserved.source_display
        && edge == reserved.source_edge
        && segments_overlap(segment, reserved.source_segment))
        || (display == &reserved.target_display
            && edge == reserved.target_edge
            && segments_overlap(segment, reserved.target_segment))
}

fn segments_overlap(left: EdgeSegment, right: EdgeSegment) -> bool {
    left.start_um.max(right.start_um) < left.end_um.min(right.end_um)
}

#[allow(clippy::too_many_arguments)]
fn push_candidate(
    candidates: &mut Vec<AdjacencyCandidate>,
    source: &DisplaySurface,
    source_edge: Edge,
    source_start: i64,
    source_end: i64,
    target: &DisplaySurface,
    target_edge: Edge,
    target_start: i64,
    target_end: i64,
    normal_gap_um: i64,
    tolerance: i64,
) {
    if normal_gap_um <= tolerance {
        candidates.push(AdjacencyCandidate {
            source_display: source.display_id.clone(),
            source_edge,
            source_segment: EdgeSegment {
                start_um: source_start,
                end_um: source_end,
            },
            target_display: target.display_id.clone(),
            target_edge,
            target_segment: EdgeSegment {
                start_um: target_start,
                end_um: target_end,
            },
            normal_gap_um,
        });
    }
}

fn interval_overlap(
    left_start: i64,
    left_length: i64,
    right_start: i64,
    right_length: i64,
) -> Option<(i64, i64)> {
    let start = left_start.max(right_start);
    let end = left_start
        .saturating_add(left_length)
        .min(right_start.saturating_add(right_length));
    (end > start).then_some((start, end))
}

fn coordinate_distance(left: i64, right: i64) -> i64 {
    ((i128::from(left) - i128::from(right)).abs()).min(i128::from(i64::MAX)) as i64
}

fn portal_matches(
    portal: &Portal,
    source_display: &DisplayId,
    source_edge: Edge,
    target_display: &DisplayId,
    target_edge: Edge,
) -> bool {
    (portal.source_display == *source_display
        && portal.source_edge == source_edge
        && portal.target_display == *target_display
        && portal.target_edge == target_edge)
        || (portal.direction == PortalDirection::Bidirectional
            && portal.source_display == *target_display
            && portal.source_edge == target_edge
            && portal.target_display == *source_display
            && portal.target_edge == source_edge)
}

fn generated_portal_id(candidate: &AdjacencyCandidate) -> Result<PortalId, TopologyError> {
    let relation = format!(
        "{}:{}:{}:{}",
        candidate.source_display,
        edge_rank(candidate.source_edge),
        candidate.target_display,
        edge_rank(candidate.target_edge)
    );
    let digest = Sha256::digest(relation.as_bytes());
    let suffix = digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    PortalId::parse(format!("auto-{suffix}")).map_err(|_| TopologyError::GeneratedPortalId)
}

fn edge_rank(edge: Edge) -> u8 {
    match edge {
        Edge::Left => 0,
        Edge::Right => 1,
        Edge::Top => 2,
        Edge::Bottom => 3,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PortalCrossing {
    pub portal_id: PortalId,
    pub source_display: DisplayId,
    pub target_display: DisplayId,
    pub source_point: DeskPointUm,
    pub target_point: DeskPointUm,
    pub fraction: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RouteOutcome {
    pub display_id: DisplayId,
    pub desk_point: DeskPointUm,
    pub crossings: Vec<PortalCrossing>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RoutingContext {
    pub held_modifier: Option<u16>,
    pub dragging: bool,
    pub dwell_elapsed_ms: u32,
    /// Ignore transitions to another display owned by the current device.
    /// The source operating system remains authoritative for those native
    /// display transitions while local input is active.
    pub cross_device_only: bool,
}

pub struct PortalResolver;

impl PortalResolver {
    pub fn dwell_requirement(
        layout: &WorkspaceLayout,
        display_id: &DisplayId,
        point: DeskPointUm,
        movement: DeskVectorUm,
        context: RoutingContext,
    ) -> Result<Option<(PortalId, u32)>, TopologyError> {
        layout.validate()?;
        let display = layout
            .displays
            .get(display_id)
            .ok_or_else(|| TopologyError::MissingDisplay(display_id.clone()))?;
        let destination = point.checked_add(movement)?;
        Ok(layout
            .portals
            .iter()
            .filter_map(|portal| {
                let reverse = portal.direction == PortalDirection::Bidirectional
                    && portal.target_display == display.display_id;
                let forward = portal.source_display == display.display_id;
                if (!forward && !reverse)
                    || portal.status != PortalStatus::Active
                    || (context.dragging && !portal.allow_while_dragging)
                    || (context.cross_device_only && !portal_crosses_devices(layout, portal))
                {
                    return None;
                }
                let ActivationPolicy::Dwell { milliseconds } = portal.activation_policy else {
                    return None;
                };
                let (edge, segment) = if reverse {
                    (portal.target_edge, portal.target_segment)
                } else {
                    (portal.source_edge, portal.source_segment)
                };
                intersection(display.desk_rect_um, point, destination, edge, segment)
                    .map(|(_, fraction, _)| (fraction, portal.portal_id.clone(), milliseconds))
            })
            .min_by(|left, right| left.0.total_cmp(&right.0))
            .map(|(_, portal_id, milliseconds)| (portal_id, milliseconds)))
    }

    pub fn route(
        layout: &WorkspaceLayout,
        mut display_id: DisplayId,
        mut point: DeskPointUm,
        mut movement: DeskVectorUm,
        context: RoutingContext,
    ) -> Result<RouteOutcome, TopologyError> {
        layout.validate()?;
        let mut crossings = Vec::new();
        for _ in 0..8 {
            let display = layout
                .displays
                .get(&display_id)
                .ok_or_else(|| TopologyError::MissingDisplay(display_id.clone()))?;
            let destination = point.checked_add(movement)?;
            let Some(hit) = earliest_crossing(layout, display, point, destination, context) else {
                point = display.desk_rect_um.clamp(destination, 1);
                return Ok(RouteOutcome {
                    display_id,
                    desk_point: point,
                    crossings,
                });
            };
            let portal = &layout.portals[hit.portal_index];
            let (source_edge, source_segment, target_display_id, target_edge, target_segment) =
                if hit.reverse {
                    (
                        portal.target_edge,
                        portal.target_segment,
                        &portal.source_display,
                        portal.source_edge,
                        portal.source_segment,
                    )
                } else {
                    (
                        portal.source_edge,
                        portal.source_segment,
                        &portal.target_display,
                        portal.target_edge,
                        portal.target_segment,
                    )
                };
            let target = layout
                .displays
                .get(target_display_id)
                .ok_or_else(|| TopologyError::MissingDisplay(target_display_id.clone()))?;
            let remaining = vector_scale(movement, 1.0 - hit.fraction);
            let mapped_edge_point = map_edge_position(
                target.desk_rect_um,
                target_edge,
                target_segment,
                hit.normalized_position,
            );
            let mapped_remaining = map_overshoot(
                remaining,
                source_edge,
                target_edge,
                source_segment,
                target_segment,
            );
            let inset = portal.inset_um.max(1).saturating_add(portal.hysteresis_um);
            let target_edge_point = add_inward(mapped_edge_point, target_edge, inset)?;
            let target_point = target
                .desk_rect_um
                .clamp(target_edge_point.checked_add(mapped_remaining)?, 1);
            crossings.push(PortalCrossing {
                portal_id: portal.portal_id.clone(),
                source_display: display_id.clone(),
                target_display: target_display_id.clone(),
                source_point: hit.point,
                target_point,
                fraction: hit.fraction,
            });
            display_id = target_display_id.clone();
            point = target_point;
            movement = DeskVectorUm::default();
        }
        Err(TopologyError::TooManyCrossings)
    }
}

struct CrossingHit {
    portal_index: usize,
    point: DeskPointUm,
    fraction: f64,
    normalized_position: f64,
    reverse: bool,
}

fn earliest_crossing(
    layout: &WorkspaceLayout,
    display: &DisplaySurface,
    start: DeskPointUm,
    end: DeskPointUm,
    context: RoutingContext,
) -> Option<CrossingHit> {
    layout
        .portals
        .iter()
        .enumerate()
        .filter(|(_, portal)| {
            (portal.source_display == display.display_id
                || (portal.direction == PortalDirection::Bidirectional
                    && portal.target_display == display.display_id))
                && portal.status == PortalStatus::Active
                && (!context.dragging || portal.allow_while_dragging)
                && (!context.cross_device_only || portal_crosses_devices(layout, portal))
                && match portal.activation_policy {
                    ActivationPolicy::Immediate => true,
                    ActivationPolicy::Dwell { milliseconds } => {
                        context.dwell_elapsed_ms >= milliseconds
                    }
                    ActivationPolicy::RequireModifier { hid_usage } => {
                        context.held_modifier == Some(hid_usage)
                    }
                }
        })
        .filter_map(|(index, portal)| {
            let reverse = portal.target_display == display.display_id
                && portal.direction == PortalDirection::Bidirectional;
            let (edge, segment) = if reverse {
                (portal.target_edge, portal.target_segment)
            } else {
                (portal.source_edge, portal.source_segment)
            };
            intersection(display.desk_rect_um, start, end, edge, segment).map(
                |(point, fraction, normalized_position)| CrossingHit {
                    portal_index: index,
                    point,
                    fraction,
                    normalized_position,
                    reverse,
                },
            )
        })
        .min_by(|left, right| {
            left.fraction.total_cmp(&right.fraction).then_with(|| {
                layout.portals[left.portal_index]
                    .portal_id
                    .cmp(&layout.portals[right.portal_index].portal_id)
            })
        })
}

fn portal_crosses_devices(layout: &WorkspaceLayout, portal: &Portal) -> bool {
    match (
        layout.displays.get(&portal.source_display),
        layout.displays.get(&portal.target_display),
    ) {
        (Some(source), Some(target)) => source.device_id != target.device_id,
        _ => false,
    }
}

fn intersection(
    rect: DeskRectUm,
    start: DeskPointUm,
    end: DeskPointUm,
    edge: Edge,
    segment: EdgeSegment,
) -> Option<(DeskPointUm, f64, f64)> {
    let dx = (end.x - start.x) as f64;
    let dy = (end.y - start.y) as f64;
    let (plane, origin, delta, outward) = match edge {
        Edge::Left => (rect.x as f64, start.x as f64, dx, dx < 0.0),
        Edge::Right => (
            rect.x.saturating_add(rect.width) as f64,
            start.x as f64,
            dx,
            dx > 0.0,
        ),
        Edge::Top => (rect.y as f64, start.y as f64, dy, dy < 0.0),
        Edge::Bottom => (
            rect.y.saturating_add(rect.height) as f64,
            start.y as f64,
            dy,
            dy > 0.0,
        ),
    };
    if !outward || delta == 0.0 {
        return None;
    }
    let fraction = (plane - origin) / delta;
    if !(0.0..=1.0).contains(&fraction) {
        return None;
    }
    let (x, y, along) = match edge {
        Edge::Left | Edge::Right => {
            let y = start.y as f64 + dy * fraction;
            (plane, y, y - rect.y as f64)
        }
        Edge::Top | Edge::Bottom => {
            let x = start.x as f64 + dx * fraction;
            (x, plane, x - rect.x as f64)
        }
    };
    let segment_owns_terminal = segment.end_um == edge_length(rect, edge);
    if along < segment.start_um as f64
        || along > segment.end_um as f64
        || (along == segment.end_um as f64 && !segment_owns_terminal)
    {
        return None;
    }
    let normalized = ((along - segment.start_um as f64) / segment.length() as f64).clamp(0.0, 1.0);
    Some((
        DeskPointUm {
            x: x.round() as i64,
            y: y.round() as i64,
        },
        fraction,
        normalized,
    ))
}

fn map_edge_position(
    rect: DeskRectUm,
    edge: Edge,
    segment: EdgeSegment,
    normalized: f64,
) -> DeskPointUm {
    let along = segment.start_um as f64 + normalized * segment.length() as f64;
    match edge {
        Edge::Left => DeskPointUm {
            x: rect.x,
            y: rect.y.saturating_add(along.round() as i64),
        },
        Edge::Right => DeskPointUm {
            x: rect.x.saturating_add(rect.width),
            y: rect.y.saturating_add(along.round() as i64),
        },
        Edge::Top => DeskPointUm {
            x: rect.x.saturating_add(along.round() as i64),
            y: rect.y,
        },
        Edge::Bottom => DeskPointUm {
            x: rect.x.saturating_add(along.round() as i64),
            y: rect.y.saturating_add(rect.height),
        },
    }
}

fn vector_scale(vector: DeskVectorUm, scale: f64) -> DeskVectorUm {
    DeskVectorUm {
        x: (vector.x as f64 * scale).round() as i64,
        y: (vector.y as f64 * scale).round() as i64,
    }
}

fn basis(edge: Edge) -> (DeskVectorUm, DeskVectorUm) {
    match edge {
        Edge::Left => (DeskVectorUm { x: -1, y: 0 }, DeskVectorUm { x: 0, y: 1 }),
        Edge::Right => (DeskVectorUm { x: 1, y: 0 }, DeskVectorUm { x: 0, y: 1 }),
        Edge::Top => (DeskVectorUm { x: 0, y: -1 }, DeskVectorUm { x: 1, y: 0 }),
        Edge::Bottom => (DeskVectorUm { x: 0, y: 1 }, DeskVectorUm { x: 1, y: 0 }),
    }
}

fn map_overshoot(
    remaining: DeskVectorUm,
    source_edge: Edge,
    target_edge: Edge,
    source_segment: EdgeSegment,
    target_segment: EdgeSegment,
) -> DeskVectorUm {
    let (source_outward, source_tangent) = basis(source_edge);
    let (target_outward, target_tangent) = basis(target_edge);
    let normal =
        remaining.x.saturating_mul(source_outward.x) + remaining.y.saturating_mul(source_outward.y);
    let tangent =
        remaining.x.saturating_mul(source_tangent.x) + remaining.y.saturating_mul(source_tangent.y);
    let tangent_scale = target_segment.length() as f64 / source_segment.length() as f64;
    let tangent = (tangent as f64 * tangent_scale).round() as i64;
    DeskVectorUm {
        x: target_outward
            .x
            .saturating_neg()
            .saturating_mul(normal)
            .saturating_add(target_tangent.x.saturating_mul(tangent)),
        y: target_outward
            .y
            .saturating_neg()
            .saturating_mul(normal)
            .saturating_add(target_tangent.y.saturating_mul(tangent)),
    }
}

fn add_inward(point: DeskPointUm, edge: Edge, inset: i64) -> Result<DeskPointUm, TopologyError> {
    let (outward, _) = basis(edge);
    point.checked_add(DeskVectorUm {
        x: outward.x.saturating_neg().saturating_mul(inset),
        y: outward.y.saturating_neg().saturating_mul(inset),
    })
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum TopologyError {
    #[error("topology revision must be non-zero")]
    ZeroRevision,
    #[error("desk rectangle must be non-empty")]
    EmptyRect,
    #[error("desk coordinate overflow")]
    Overflow,
    #[error("display {0} does not exist")]
    MissingDisplay(DisplayId),
    #[error("duplicate portal {0}")]
    DuplicatePortal(PortalId),
    #[error("portal {0} targets its own display")]
    SelfPortal(PortalId),
    #[error("portal {0} has an invalid edge segment")]
    InvalidSegment(PortalId),
    #[error("portal {0} has a negative inset or hysteresis")]
    NegativePolicyDistance(PortalId),
    #[error("motion crossed too many portals")]
    TooManyCrossings,
    #[error("failed to generate a stable portal identifier")]
    GeneratedPortalId,
}

#[cfg(test)]
#[path = "topology_tests.rs"]
mod topology_tests;
