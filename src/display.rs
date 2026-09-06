use std::collections::{BTreeMap, BTreeSet};

use arcrelay_peer::ServiceInstanceId;
use serde::{Deserialize, Serialize};

use crate::{
    DeskPointUm, DeskRectUm, DisplayFingerprint, DisplayId, InventoryRevision, LogicalPoint,
    LogicalRect,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub struct SizeU32 {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub struct SizeI64 {
    pub width: i64,
    pub height: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(transparent)]
pub struct ScaleFactor(pub f64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub enum DisplayRotation {
    Degrees0,
    Degrees90,
    Degrees180,
    Degrees270,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ts_rs::TS)]
pub enum GeometryConfidence {
    Unknown,
    Estimated,
    HardwareReported,
    UserProvided,
    UserCalibrated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct DisplaySurface {
    pub display_id: DisplayId,
    pub device_id: ServiceInstanceId,
    pub fingerprint: DisplayFingerprint,
    pub name: String,
    pub pixel_size: SizeU32,
    pub logical_bounds: LogicalRect,
    pub scale_factor: ScaleFactor,
    pub physical_size_um: SizeI64,
    pub rotation: DisplayRotation,
    pub desk_rect_um: DeskRectUm,
    pub geometry_confidence: GeometryConfidence,
    pub inventory_revision: InventoryRevision,
}

impl DisplaySurface {
    pub fn validate(&self) -> Result<(), DisplayError> {
        if self.pixel_size.width == 0 || self.pixel_size.height == 0 {
            return Err(DisplayError::EmptyPixelSize(self.display_id.clone()));
        }
        if self.logical_bounds.width <= 0.0 || self.logical_bounds.height <= 0.0 {
            return Err(DisplayError::EmptyLogicalSize(self.display_id.clone()));
        }
        if !self.scale_factor.0.is_finite() || self.scale_factor.0 <= 0.0 {
            return Err(DisplayError::InvalidScale(self.display_id.clone()));
        }
        if self.physical_size_um.width <= 0 || self.physical_size_um.height <= 0 {
            return Err(DisplayError::EmptyPhysicalSize(self.display_id.clone()));
        }
        self.desk_rect_um.validate()?;
        Ok(())
    }

    /// Convert a global physical desk point to the target OS logical space.
    pub fn logical_point_from_desk(&self, point: DeskPointUm) -> LogicalPoint {
        let u = ((point.x - self.desk_rect_um.x) as f64 / self.desk_rect_um.width as f64)
            .clamp(0.0, 1.0);
        let v = ((point.y - self.desk_rect_um.y) as f64 / self.desk_rect_um.height as f64)
            .clamp(0.0, 1.0);
        let (u, v) = match self.rotation {
            DisplayRotation::Degrees0 => (u, v),
            DisplayRotation::Degrees90 => (v, 1.0 - u),
            DisplayRotation::Degrees180 => (1.0 - u, 1.0 - v),
            DisplayRotation::Degrees270 => (1.0 - v, u),
        };
        LogicalPoint {
            x: self.logical_bounds.x + u * self.logical_bounds.width,
            y: self.logical_bounds.y + v * self.logical_bounds.height,
        }
    }

    /// Convert a point in the target OS global logical space back into the
    /// shared physical desk coordinate system.
    pub fn desk_point_from_logical(&self, point: LogicalPoint) -> DeskPointUm {
        let u = ((point.x - self.logical_bounds.x) / self.logical_bounds.width).clamp(0.0, 1.0);
        let v = ((point.y - self.logical_bounds.y) / self.logical_bounds.height).clamp(0.0, 1.0);
        let (u, v) = match self.rotation {
            DisplayRotation::Degrees0 => (u, v),
            DisplayRotation::Degrees90 => (1.0 - v, u),
            DisplayRotation::Degrees180 => (1.0 - u, 1.0 - v),
            DisplayRotation::Degrees270 => (v, 1.0 - u),
        };
        DeskPointUm {
            x: self.desk_rect_um.x + (u * self.desk_rect_um.width as f64).round() as i64,
            y: self.desk_rect_um.y + (v * self.desk_rect_um.height as f64).round() as i64,
        }
    }

    pub fn contains_logical_point(&self, point: LogicalPoint) -> bool {
        point.x >= self.logical_bounds.x
            && point.x < self.logical_bounds.x + self.logical_bounds.width
            && point.y >= self.logical_bounds.y
            && point.y < self.logical_bounds.y + self.logical_bounds.height
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInventory {
    pub device_id: ServiceInstanceId,
    pub revision: InventoryRevision,
    pub displays: Vec<DisplaySurface>,
}

impl DisplayInventory {
    pub fn validate(&self) -> Result<(), DisplayError> {
        let mut ids = BTreeSet::new();
        for display in &self.displays {
            display.validate()?;
            if display.device_id != self.device_id {
                return Err(DisplayError::WrongDevice(display.display_id.clone()));
            }
            if display.inventory_revision != self.revision {
                return Err(DisplayError::WrongRevision(display.display_id.clone()));
            }
            if !ids.insert(display.display_id.clone()) {
                return Err(DisplayError::DuplicateId(display.display_id.clone()));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reconciliation {
    Matched(DisplayId),
    New,
    NeedsReconciliation(Vec<DisplayId>),
}

pub struct DisplayReconciler;

impl DisplayReconciler {
    pub fn reconcile(
        current: &[DisplaySurface],
        reported_fingerprint: &DisplayFingerprint,
        pixel_size: SizeU32,
    ) -> Reconciliation {
        let exact = current
            .iter()
            .filter(|display| &display.fingerprint == reported_fingerprint)
            .map(|display| display.display_id.clone())
            .collect::<Vec<_>>();
        if exact.len() == 1 {
            return Reconciliation::Matched(exact[0].clone());
        }
        if exact.len() > 1 {
            return Reconciliation::NeedsReconciliation(exact);
        }
        let candidates = current
            .iter()
            .filter(|display| display.pixel_size == pixel_size)
            .map(|display| display.display_id.clone())
            .collect::<Vec<_>>();
        match candidates.len() {
            0 => Reconciliation::New,
            1 => Reconciliation::Matched(candidates[0].clone()),
            _ => Reconciliation::NeedsReconciliation(candidates),
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DisplayError {
    #[error("display {0} has empty pixel size")]
    EmptyPixelSize(DisplayId),
    #[error("display {0} has empty logical size")]
    EmptyLogicalSize(DisplayId),
    #[error("display {0} has invalid scale")]
    InvalidScale(DisplayId),
    #[error("display {0} has empty physical size")]
    EmptyPhysicalSize(DisplayId),
    #[error("display {0} belongs to another device")]
    WrongDevice(DisplayId),
    #[error("display {0} has the wrong inventory revision")]
    WrongRevision(DisplayId),
    #[error("duplicate display id {0}")]
    DuplicateId(DisplayId),
    #[error(transparent)]
    Geometry(#[from] crate::TopologyError),
}

pub fn inventories_by_device(
    displays: impl IntoIterator<Item = DisplaySurface>,
) -> BTreeMap<ServiceInstanceId, Vec<DisplaySurface>> {
    let mut grouped = BTreeMap::new();
    for display in displays {
        grouped
            .entry(display.device_id.clone())
            .or_insert_with(Vec::new)
            .push(display);
    }
    grouped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeskRectUm, DisplayFingerprint, LogicalRect};

    fn surface(id: &str, fingerprint: &str) -> DisplaySurface {
        DisplaySurface {
            display_id: DisplayId::parse(id).unwrap(),
            device_id: ServiceInstanceId::parse("device").unwrap(),
            fingerprint: DisplayFingerprint::parse(fingerprint).unwrap(),
            name: id.into(),
            pixel_size: SizeU32 {
                width: 3840,
                height: 2160,
            },
            logical_bounds: LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            },
            scale_factor: ScaleFactor(2.0),
            physical_size_um: SizeI64 {
                width: 600_000,
                height: 340_000,
            },
            rotation: DisplayRotation::Degrees0,
            desk_rect_um: DeskRectUm {
                x: 0,
                y: 0,
                width: 600_000,
                height: 340_000,
            },
            geometry_confidence: GeometryConfidence::HardwareReported,
            inventory_revision: InventoryRevision(1),
        }
    }

    #[test]
    fn ambiguous_identical_displays_require_reconciliation() {
        let displays = [surface("left", "same"), surface("right", "same")];
        assert!(matches!(
            DisplayReconciler::reconcile(
                &displays,
                &DisplayFingerprint::parse("same").unwrap(),
                SizeU32 {
                    width: 3840,
                    height: 2160
                }
            ),
            Reconciliation::NeedsReconciliation(values) if values.len() == 2
        ));
    }

    #[test]
    fn logical_and_desk_coordinates_round_trip_for_every_rotation() {
        for rotation in [
            DisplayRotation::Degrees0,
            DisplayRotation::Degrees90,
            DisplayRotation::Degrees180,
            DisplayRotation::Degrees270,
        ] {
            let mut display = surface("display", "fingerprint");
            display.rotation = rotation;
            display.logical_bounds = LogicalRect {
                x: -200.0,
                y: 50.0,
                width: 1600.0,
                height: 900.0,
            };
            display.desk_rect_um = DeskRectUm {
                x: 100_000,
                y: -50_000,
                width: 600_000,
                height: 340_000,
            };
            let desk = DeskPointUm {
                x: 280_000,
                y: 52_000,
            };
            let restored = display.desk_point_from_logical(display.logical_point_from_desk(desk));
            assert!((restored.x - desk.x).abs() <= 1, "{rotation:?}");
            assert!((restored.y - desk.y).abs() <= 1, "{rotation:?}");
        }
    }
}
