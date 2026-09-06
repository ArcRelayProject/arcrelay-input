use super::*;
use crate::{
    DisplayFingerprint, DisplayRotation, GeometryConfidence, InventoryRevision, LogicalRect,
    ScaleFactor, SizeI64, SizeU32,
};
use arcrelay_peer::ServiceInstanceId;
use proptest::prelude::*;

fn three_screen_layout(offline: &str) -> WorkspaceLayout {
    let displays = ["a", "b", "c"]
        .into_iter()
        .enumerate()
        .map(|(index, id)| {
            let value = display(
                id,
                &format!("device-{id}"),
                DeskRectUm {
                    x: index as i64 * 500_000,
                    y: 0,
                    width: 500_000,
                    height: 300_000,
                },
            );
            (value.display_id.clone(), value)
        })
        .collect();
    let mut layout = WorkspaceLayout {
        workspace_id: WorkspaceId::parse("desk").unwrap(),
        revision: TopologyRevision(1),
        portals: derive_auto_portals(&displays, &[], 5_000, 1_000).unwrap(),
        displays,
    };
    for portal in &mut layout.portals {
        if portal.source_display.as_str() == offline || portal.target_display.as_str() == offline {
            portal.status = PortalStatus::SuspendedOffline;
        }
    }
    layout
}

#[test]
fn offline_neighbor_does_not_block_the_online_component() {
    let layout = three_screen_layout("c");
    let component =
        active_component_devices(&layout, &ServiceInstanceId::parse("device-a").unwrap());
    assert_eq!(
        component
            .into_iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>(),
        ["device-a", "device-b"]
    );
    let result = PortalResolver::route(
        &layout,
        DisplayId::parse("a").unwrap(),
        DeskPointUm {
            x: 499_000,
            y: 150_000,
        },
        DeskVectorUm { x: 900_000, y: 0 },
        RoutingContext::default(),
    )
    .unwrap();
    assert_eq!(result.display_id.as_str(), "b");
}

#[test]
fn offline_middle_is_a_barrier_even_for_a_large_pointer_delta() {
    let layout = three_screen_layout("b");
    assert_eq!(
        active_component_devices(&layout, &ServiceInstanceId::parse("device-a").unwrap()).len(),
        1
    );
    let result = PortalResolver::route(
        &layout,
        DisplayId::parse("a").unwrap(),
        DeskPointUm {
            x: 499_000,
            y: 150_000,
        },
        DeskVectorUm { x: 900_000, y: 0 },
        RoutingContext::default(),
    )
    .unwrap();
    assert_eq!(result.display_id.as_str(), "a");
    assert!(result.crossings.is_empty());
}

#[test]
fn an_online_detour_connects_screens_around_an_offline_middle() {
    let mut layout = three_screen_layout("b");
    let lower = display(
        "d",
        "device-d",
        DeskRectUm {
            x: 0,
            y: 300_000,
            width: 1_500_000,
            height: 300_000,
        },
    );
    layout.displays.insert(lower.display_id.clone(), lower);
    layout.portals = derive_auto_portals(&layout.displays, &layout.portals, 5_000, 1_000).unwrap();
    for portal in &mut layout.portals {
        if portal.source_display.as_str() == "b" || portal.target_display.as_str() == "b" {
            portal.status = PortalStatus::SuspendedOffline;
        }
    }
    assert_eq!(
        active_component_devices(&layout, &ServiceInstanceId::parse("device-a").unwrap())
            .into_iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>(),
        ["device-a", "device-c", "device-d"]
    );
}

#[test]
fn removing_a_middle_screen_does_not_implicitly_close_the_gap() {
    let mut layout = three_screen_layout("b");
    layout.displays.remove(&DisplayId::parse("b").unwrap());
    layout.portals = derive_auto_portals(&layout.displays, &[], 5_000, 1_000).unwrap();
    assert!(layout.portals.is_empty());
    assert_eq!(
        layout.displays[&DisplayId::parse("c").unwrap()]
            .desk_rect_um
            .x,
        1_000_000
    );
}

fn display(id: &str, device: &str, rect: DeskRectUm) -> DisplaySurface {
    DisplaySurface {
        display_id: DisplayId::parse(id).unwrap(),
        device_id: ServiceInstanceId::parse(device).unwrap(),
        fingerprint: DisplayFingerprint::parse(format!("fingerprint-{id}")).unwrap(),
        name: id.into(),
        pixel_size: SizeU32 {
            width: 1920,
            height: 1080,
        },
        logical_bounds: LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        },
        scale_factor: ScaleFactor(1.0),
        physical_size_um: SizeI64 {
            width: rect.width,
            height: rect.height,
        },
        rotation: DisplayRotation::Degrees0,
        desk_rect_um: rect,
        geometry_confidence: GeometryConfidence::UserCalibrated,
        inventory_revision: InventoryRevision(1),
    }
}

fn horizontal_layout() -> WorkspaceLayout {
    let a = display(
        "a",
        "device-a",
        DeskRectUm {
            x: 0,
            y: 0,
            width: 500_000,
            height: 300_000,
        },
    );
    let b = display(
        "b",
        "device-b",
        DeskRectUm {
            x: 600_000,
            y: 50_000,
            width: 400_000,
            height: 200_000,
        },
    );
    WorkspaceLayout {
        workspace_id: WorkspaceId::parse("desk").unwrap(),
        revision: TopologyRevision(1),
        displays: [(a.display_id.clone(), a), (b.display_id.clone(), b)]
            .into_iter()
            .collect(),
        portals: vec![Portal {
            portal_id: PortalId::parse("a-to-b").unwrap(),
            source_display: DisplayId::parse("a").unwrap(),
            source_edge: Edge::Right,
            source_segment: EdgeSegment {
                start_um: 50_000,
                end_um: 250_000,
            },
            target_display: DisplayId::parse("b").unwrap(),
            target_edge: Edge::Left,
            target_segment: EdgeSegment {
                start_um: 0,
                end_um: 200_000,
            },
            direction: PortalDirection::OneWay,
            activation_policy: ActivationPolicy::Immediate,
            allow_while_dragging: false,
            inset_um: 500,
            hysteresis_um: 500,
            status: PortalStatus::Active,
        }],
    }
}

#[test]
fn high_speed_motion_cannot_skip_a_portal() {
    let outcome = PortalResolver::route(
        &horizontal_layout(),
        DisplayId::parse("a").unwrap(),
        DeskPointUm {
            x: 100_000,
            y: 150_000,
        },
        DeskVectorUm { x: 900_000, y: 0 },
        RoutingContext::default(),
    )
    .unwrap();
    assert_eq!(outcome.display_id.as_str(), "b");
    assert_eq!(outcome.crossings.len(), 1);
    assert!(outcome.desk_point.x > 600_000);
}

#[test]
fn dragging_is_blocked_by_default() {
    let outcome = PortalResolver::route(
        &horizontal_layout(),
        DisplayId::parse("a").unwrap(),
        DeskPointUm {
            x: 490_000,
            y: 150_000,
        },
        DeskVectorUm { x: 50_000, y: 0 },
        RoutingContext {
            dragging: true,
            held_modifier: None,
            ..RoutingContext::default()
        },
    )
    .unwrap();
    assert_eq!(outcome.display_id.as_str(), "a");
    assert!(outcome.crossings.is_empty());
}

#[test]
fn dwell_portal_requires_elapsed_threshold() {
    let mut layout = horizontal_layout();
    layout.portals[0].activation_policy = ActivationPolicy::Dwell { milliseconds: 200 };
    let route = |dwell_elapsed_ms| {
        PortalResolver::route(
            &layout,
            DisplayId::parse("a").unwrap(),
            DeskPointUm {
                x: 490_000,
                y: 150_000,
            },
            DeskVectorUm { x: 50_000, y: 0 },
            RoutingContext {
                dwell_elapsed_ms,
                ..RoutingContext::default()
            },
        )
        .unwrap()
    };
    assert_eq!(route(199).display_id.as_str(), "a");
    assert_eq!(route(200).display_id.as_str(), "b");
}

#[test]
fn bidirectional_portal_routes_back_without_a_second_definition() {
    let mut layout = horizontal_layout();
    layout.portals[0].direction = PortalDirection::Bidirectional;
    let outcome = PortalResolver::route(
        &layout,
        DisplayId::parse("b").unwrap(),
        DeskPointUm {
            x: 601_000,
            y: 150_000,
        },
        DeskVectorUm { x: -20_000, y: 0 },
        RoutingContext::default(),
    )
    .unwrap();
    assert_eq!(outcome.display_id.as_str(), "a");
    assert_eq!(outcome.crossings[0].portal_id.as_str(), "a-to-b");
}

#[test]
fn auto_portal_uses_only_the_physical_overlap() {
    let a = display(
        "a",
        "device-a",
        DeskRectUm {
            x: 0,
            y: 0,
            width: 500_000,
            height: 300_000,
        },
    );
    let b = display(
        "b",
        "device-b",
        DeskRectUm {
            x: 510_000,
            y: 50_000,
            width: 400_000,
            height: 200_000,
        },
    );
    let displays = [(a.display_id.clone(), a), (b.display_id.clone(), b)]
        .into_iter()
        .collect();
    let portals = derive_auto_portals(
        &displays,
        &[],
        DEFAULT_PORTAL_ADJACENCY_TOLERANCE_UM,
        DEFAULT_MINIMUM_PORTAL_SPAN_UM,
    )
    .unwrap();

    assert_eq!(portals.len(), 1);
    assert_eq!(portals[0].source_edge, Edge::Right);
    assert_eq!(
        portals[0].source_segment,
        EdgeSegment {
            start_um: 50_000,
            end_um: 250_000,
        }
    );
    assert_eq!(portals[0].target_edge, Edge::Left);
    assert_eq!(
        portals[0].target_segment,
        EdgeSegment {
            start_um: 0,
            end_um: 200_000,
        }
    );
}

#[test]
fn auto_portal_does_not_connect_distant_or_corner_only_displays() {
    let a = display(
        "a",
        "device-a",
        DeskRectUm {
            x: 0,
            y: 0,
            width: 500_000,
            height: 300_000,
        },
    );
    let distant = display(
        "distant",
        "device-b",
        DeskRectUm {
            x: 530_000,
            y: 0,
            width: 400_000,
            height: 200_000,
        },
    );
    let corner = display(
        "corner",
        "device-c",
        DeskRectUm {
            x: 500_000,
            y: 300_000,
            width: 400_000,
            height: 200_000,
        },
    );
    let displays = [a, distant, corner]
        .into_iter()
        .map(|display| (display.display_id.clone(), display))
        .collect();

    assert!(derive_auto_portals(
        &displays,
        &[],
        DEFAULT_PORTAL_ADJACENCY_TOLERANCE_UM,
        DEFAULT_MINIMUM_PORTAL_SPAN_UM,
    )
    .unwrap()
    .is_empty());
}

#[test]
fn auto_portal_preserves_behavior_but_replaces_manual_segments() {
    let mut layout = horizontal_layout();
    layout
        .displays
        .get_mut(&DisplayId::parse("b").unwrap())
        .unwrap()
        .desk_rect_um
        .x = 510_000;
    layout.portals[0].activation_policy = ActivationPolicy::Dwell { milliseconds: 420 };
    layout.portals[0].allow_while_dragging = true;
    layout.portals[0].source_segment = EdgeSegment {
        start_um: 0,
        end_um: 300_000,
    };
    let portals = derive_auto_portals(
        &layout.displays,
        &layout.portals,
        DEFAULT_PORTAL_ADJACENCY_TOLERANCE_UM,
        DEFAULT_MINIMUM_PORTAL_SPAN_UM,
    )
    .unwrap();

    assert_eq!(portals.len(), 1);
    assert_eq!(
        portals[0].activation_policy,
        ActivationPolicy::Dwell { milliseconds: 420 }
    );
    assert!(portals[0].allow_while_dragging);
    assert_eq!(portals[0].source_segment.start_um, 50_000);
}

#[test]
fn same_device_portal_follows_system_bounds_instead_of_workspace_placement() {
    let mut a = display(
        "a",
        "device-local",
        DeskRectUm {
            x: 100_000,
            y: 50_000,
            width: 500_000,
            height: 300_000,
        },
    );
    let mut b = display(
        "b",
        "device-local",
        DeskRectUm {
            x: 1_500_000,
            y: 900_000,
            width: 400_000,
            height: 200_000,
        },
    );
    a.logical_bounds = LogicalRect {
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1080.0,
    };
    b.logical_bounds = LogicalRect {
        x: 1920.0,
        y: 0.0,
        width: 1280.0,
        height: 720.0,
    };
    let displays = [a, b]
        .into_iter()
        .map(|display| (display.display_id.clone(), display))
        .collect();

    let portals = derive_auto_portals(
        &displays,
        &[],
        DEFAULT_PORTAL_ADJACENCY_TOLERANCE_UM,
        DEFAULT_MINIMUM_PORTAL_SPAN_UM,
    )
    .unwrap();

    assert_eq!(portals.len(), 1);
    assert_eq!(portals[0].source_display.as_str(), "a");
    assert_eq!(portals[0].source_edge, Edge::Right);
    assert_eq!(portals[0].target_display.as_str(), "b");
    assert_eq!(portals[0].target_edge, Edge::Left);
}

#[test]
fn remote_portal_cannot_occupy_a_system_owned_edge_segment() {
    let mut a = display(
        "a",
        "device-local",
        DeskRectUm {
            x: 0,
            y: 0,
            width: 500_000,
            height: 300_000,
        },
    );
    let mut b = display(
        "b",
        "device-local",
        DeskRectUm {
            x: 500_000,
            y: 0,
            width: 400_000,
            height: 300_000,
        },
    );
    let remote = display(
        "remote",
        "device-remote",
        DeskRectUm {
            x: 500_000,
            y: 0,
            width: 450_000,
            height: 300_000,
        },
    );
    a.logical_bounds.x = 0.0;
    b.logical_bounds.x = a.logical_bounds.width;
    let displays = [a, b, remote]
        .into_iter()
        .map(|display| (display.display_id.clone(), display))
        .collect();

    let portals = derive_auto_portals(
        &displays,
        &[],
        DEFAULT_PORTAL_ADJACENCY_TOLERANCE_UM,
        DEFAULT_MINIMUM_PORTAL_SPAN_UM,
    )
    .unwrap();

    assert_eq!(portals.len(), 1);
    let source = &displays[&portals[0].source_display];
    let target = &displays[&portals[0].target_display];
    assert_eq!(source.device_id, target.device_id);
}

#[test]
fn remote_portal_can_use_an_exposed_part_of_a_partially_owned_edge() {
    let mut a = display(
        "a",
        "device-local",
        DeskRectUm {
            x: 0,
            y: 0,
            width: 500_000,
            height: 300_000,
        },
    );
    let mut b = display(
        "b",
        "device-local",
        DeskRectUm {
            x: 500_000,
            y: 0,
            width: 400_000,
            height: 150_000,
        },
    );
    let remote = display(
        "remote",
        "device-remote",
        DeskRectUm {
            x: 500_000,
            y: 200_000,
            width: 450_000,
            height: 100_000,
        },
    );
    a.logical_bounds = LogicalRect {
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1080.0,
    };
    b.logical_bounds = LogicalRect {
        x: 1920.0,
        y: 0.0,
        width: 1280.0,
        height: 540.0,
    };
    let displays = [a, b, remote]
        .into_iter()
        .map(|display| (display.display_id.clone(), display))
        .collect();

    let portals = derive_auto_portals(
        &displays,
        &[],
        DEFAULT_PORTAL_ADJACENCY_TOLERANCE_UM,
        DEFAULT_MINIMUM_PORTAL_SPAN_UM,
    )
    .unwrap();

    assert_eq!(portals.len(), 2);
    assert!(portals.iter().any(|portal| {
        portal.source_display.as_str() == "remote" || portal.target_display.as_str() == "remote"
    }));
}

#[test]
fn native_layout_alignment_preserves_the_anchor_and_calibrated_sizes() {
    let device = ServiceInstanceId::parse("device-local").unwrap();
    let mut a = display(
        "a",
        device.as_str(),
        DeskRectUm {
            x: 100_000,
            y: 80_000,
            width: 500_000,
            height: 300_000,
        },
    );
    let mut b = display(
        "b",
        device.as_str(),
        DeskRectUm {
            x: 2_000_000,
            y: 2_000_000,
            width: 400_000,
            height: 200_000,
        },
    );
    a.logical_bounds.x = 0.0;
    b.logical_bounds.x = a.logical_bounds.width;
    let mut displays = [a, b]
        .into_iter()
        .map(|display| (display.display_id.clone(), display))
        .collect();

    align_system_display_group(&mut displays, &device).unwrap();

    assert_eq!(
        displays[&DisplayId::parse("a").unwrap()].desk_rect_um.x,
        100_000
    );
    assert_eq!(
        displays[&DisplayId::parse("a").unwrap()].desk_rect_um.y,
        80_000
    );
    assert_eq!(
        displays[&DisplayId::parse("b").unwrap()].desk_rect_um.x,
        600_000
    );
    assert_eq!(
        displays[&DisplayId::parse("b").unwrap()].desk_rect_um.y,
        80_000
    );
    assert_eq!(
        displays[&DisplayId::parse("b").unwrap()].desk_rect_um.width,
        400_000
    );
    assert_eq!(
        displays[&DisplayId::parse("b").unwrap()]
            .desk_rect_um
            .height,
        200_000
    );
}

#[test]
fn native_layout_alignment_does_not_anchor_a_newly_appended_lower_id() {
    let device = ServiceInstanceId::parse("device-local").unwrap();
    let mut existing = display(
        "z-existing",
        device.as_str(),
        DeskRectUm {
            x: 100_000,
            y: 80_000,
            width: 500_000,
            height: 300_000,
        },
    );
    let mut discovered = display(
        "a-new",
        device.as_str(),
        DeskRectUm {
            x: 2_000_000,
            y: 0,
            width: 400_000,
            height: 300_000,
        },
    );
    existing.logical_bounds.x = 0.0;
    discovered.logical_bounds.x = existing.logical_bounds.width;
    let mut displays = [existing, discovered]
        .into_iter()
        .map(|display| (display.display_id.clone(), display))
        .collect();

    align_system_display_group(&mut displays, &device).unwrap();

    assert_eq!(
        displays[&DisplayId::parse("z-existing").unwrap()]
            .desk_rect_um
            .x,
        100_000
    );
    assert_eq!(
        displays[&DisplayId::parse("a-new").unwrap()].desk_rect_um.x,
        600_000
    );
}

#[test]
fn native_portal_maps_rotated_system_edges_into_desk_space() {
    let mut rotated = display(
        "rotated",
        "device-local",
        DeskRectUm {
            x: 0,
            y: 0,
            width: 500_000,
            height: 300_000,
        },
    );
    rotated.rotation = DisplayRotation::Degrees90;
    rotated.logical_bounds = LogicalRect {
        x: 0.0,
        y: 0.0,
        width: 1080.0,
        height: 1920.0,
    };
    let mut right = display(
        "right",
        "device-local",
        DeskRectUm {
            x: 900_000,
            y: 0,
            width: 400_000,
            height: 250_000,
        },
    );
    right.logical_bounds = LogicalRect {
        x: 1080.0,
        y: 0.0,
        width: 1280.0,
        height: 1080.0,
    };
    let displays = [rotated, right]
        .into_iter()
        .map(|display| (display.display_id.clone(), display))
        .collect();

    let portals = derive_auto_portals(&displays, &[], 25_000, 5_000).unwrap();

    assert_eq!(portals.len(), 1);
    let portal = &portals[0];
    assert_eq!(portal.source_display.as_str(), "rotated");
    assert_eq!(portal.source_edge, Edge::Bottom);
    assert_eq!(portal.target_display.as_str(), "right");
    assert_eq!(portal.target_edge, Edge::Left);
    assert_eq!(portal.source_segment.start_um, 218_750);
    assert_eq!(portal.source_segment.end_um, 500_000);
}

#[test]
fn local_native_transition_can_be_excluded_from_cross_device_routing() {
    let mut a = display(
        "a",
        "device-local",
        DeskRectUm {
            x: 0,
            y: 0,
            width: 500_000,
            height: 300_000,
        },
    );
    let mut b = display(
        "b",
        "device-local",
        DeskRectUm {
            x: 500_000,
            y: 0,
            width: 400_000,
            height: 300_000,
        },
    );
    a.logical_bounds.x = 0.0;
    b.logical_bounds.x = a.logical_bounds.width;
    let displays = [a, b]
        .into_iter()
        .map(|display| (display.display_id.clone(), display))
        .collect();
    let layout = WorkspaceLayout {
        workspace_id: WorkspaceId::parse("desk").unwrap(),
        revision: TopologyRevision(1),
        portals: derive_auto_portals(&displays, &[], 25_000, 5_000).unwrap(),
        displays,
    };

    let outcome = PortalResolver::route(
        &layout,
        DisplayId::parse("a").unwrap(),
        DeskPointUm {
            x: 490_000,
            y: 150_000,
        },
        DeskVectorUm { x: 20_000, y: 0 },
        RoutingContext {
            cross_device_only: true,
            ..RoutingContext::default()
        },
    )
    .unwrap();

    assert_eq!(outcome.display_id.as_str(), "a");
    assert!(outcome.crossings.is_empty());
}

#[test]
fn shared_segment_endpoint_has_deterministic_owner() {
    let a = display(
        "a",
        "device-a",
        DeskRectUm {
            x: 0,
            y: 0,
            width: 500_000,
            height: 300_000,
        },
    );
    let b = display(
        "b",
        "device-b",
        DeskRectUm {
            x: 500_000,
            y: 0,
            width: 400_000,
            height: 150_000,
        },
    );
    let c = display(
        "c",
        "device-c",
        DeskRectUm {
            x: 500_000,
            y: 150_000,
            width: 400_000,
            height: 150_000,
        },
    );
    let displays = [a, b, c]
        .into_iter()
        .map(|display| (display.display_id.clone(), display))
        .collect::<BTreeMap<_, _>>();
    let portals = derive_auto_portals(
        &displays,
        &[],
        DEFAULT_PORTAL_ADJACENCY_TOLERANCE_UM,
        DEFAULT_MINIMUM_PORTAL_SPAN_UM,
    )
    .unwrap();
    let layout = WorkspaceLayout {
        workspace_id: WorkspaceId::parse("desk").unwrap(),
        revision: TopologyRevision(1),
        displays,
        portals,
    };

    let outcome = PortalResolver::route(
        &layout,
        DisplayId::parse("a").unwrap(),
        DeskPointUm {
            x: 490_000,
            y: 150_000,
        },
        DeskVectorUm { x: 20_000, y: 0 },
        RoutingContext::default(),
    )
    .unwrap();
    assert_eq!(outcome.display_id.as_str(), "c");
}

proptest! {
    #[test]
    fn portal_mapping_is_monotonic(y1 in 50_000_i64..150_000, y2 in 150_001_i64..250_000) {
        let layout = horizontal_layout();
        let first = PortalResolver::route(
            &layout,
            DisplayId::parse("a").unwrap(),
            DeskPointUm { x: 490_000, y: y1 },
            DeskVectorUm { x: 20_000, y: 0 },
            RoutingContext::default(),
        ).unwrap();
        let second = PortalResolver::route(
            &layout,
            DisplayId::parse("a").unwrap(),
            DeskPointUm { x: 490_000, y: y2 },
            DeskVectorUm { x: 20_000, y: 0 },
            RoutingContext::default(),
        ).unwrap();
        prop_assert!(first.desk_point.y < second.desk_point.y);
    }
}
