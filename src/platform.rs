use std::sync::mpsc::Receiver;

use serde::{Deserialize, Serialize};

use crate::{DisplayId, DisplayInventory, LogicalPoint, MappedKeyboardEvent};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(default, rename_all = "camelCase")]
pub struct PlatformCapabilities {
    pub can_capture_pointer: bool,
    pub can_capture_keyboard: bool,
    pub can_suppress_local_input: bool,
    pub can_place_internal_barrier: bool,
    pub can_inject_absolute_pointer: bool,
    pub can_inject_keyboard: bool,
    /// Pointer input is confined to the receiver's foreground application.
    /// This does not grant system-wide pointer or keyboard injection.
    pub can_inject_app_pointer: bool,
    pub can_control_elevated_apps: bool,
    pub can_persist_permission: bool,
    pub can_capture_native_quartz_events: bool,
    pub can_inject_native_quartz_events: bool,
    pub can_capture_precision_touchpad_events: bool,
    pub can_inject_precision_touchpad_events: bool,
    pub can_capture_system_gestures: bool,
    pub can_inject_system_gestures: bool,
    /// Maximum supported format; absent/zero means v1 for legacy opt-in peers.
    pub system_gesture_format_version: u32,
    pub consumer_capture_mask: u32,
    pub consumer_inject_mask: u32,
    pub brightness_display_ids: Vec<String>,
    pub limitation: Option<String>,
}

impl PlatformCapabilities {
    pub fn consumer_mask_for_display(&self, display: &DisplayId) -> u32 {
        let mask = self.consumer_inject_mask & crate::ConsumerKey::ALL_MASK;
        if self
            .brightness_display_ids
            .iter()
            .any(|id| id == display.as_str())
        {
            mask
        } else {
            mask & !crate::ConsumerKey::BRIGHTNESS_MASK
        }
    }
    pub fn system_gesture_version(&self) -> u32 {
        self.system_gesture_format_version
            .clamp(1, crate::MAX_SYSTEM_GESTURE_FORMAT_VERSION)
    }

    pub fn negotiated_system_gesture_version(&self, target: &Self) -> u32 {
        if self.can_capture_system_gestures && target.can_inject_system_gestures {
            self.system_gesture_version()
                .min(target.system_gesture_version())
        } else {
            0
        }
    }

    pub fn can_source(&self) -> bool {
        self.can_capture_pointer && self.can_capture_keyboard && self.can_suppress_local_input
    }

    pub fn can_target(&self) -> bool {
        (self.can_inject_absolute_pointer && self.can_inject_keyboard)
            || self.can_inject_app_pointer
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollUnit {
    /// Compatibility fallback for peers that only supplied `precise`.
    #[default]
    Unspecified,
    /// Post-acceleration pixel/point motion from a continuous touch surface.
    Pixel,
    /// Rotation expressed in fractions of one 120-unit wheel detent.
    WheelDetent,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollPhase {
    #[default]
    Unspecified,
    MayBegin,
    Began,
    Changed,
    Ended,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollMomentumPhase {
    #[default]
    Unspecified,
    Began,
    Changed,
    Ended,
}

/// Portable scroll semantics shared by all platform adapters. Native payloads
/// may preserve additional platform fields, but must always carry this event
/// as a safe cross-platform fallback.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ScrollEvent {
    pub delta_x: f64,
    pub delta_y: f64,
    pub unit: ScrollUnit,
    pub phase: ScrollPhase,
    pub momentum_phase: ScrollMomentumPhase,
}

impl ScrollEvent {
    pub fn is_finite(self) -> bool {
        self.delta_x.is_finite() && self.delta_y.is_finite()
    }

    pub fn has_delta(self) -> bool {
        self.delta_x != 0.0 || self.delta_y != 0.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CapturedInputEvent {
    PointerDelta {
        x: f64,
        y: f64,
    },
    PointerButton {
        hid_usage: u16,
        down: bool,
        /// Platform click sequence count (1 = single, 2 = double, 3 = triple).
        click_count: u8,
    },
    Scroll {
        event: ScrollEvent,
        /// Opaque `CGEventCreateData` representation. It is populated only
        /// while the active remote target negotiated the matching macOS
        /// capability; the structured fields remain the portable fallback.
        native_quartz_event: Option<Vec<u8>>,
    },
    Keyboard(MappedKeyboardEvent),
    ConsumerKey {
        event: crate::ConsumerKeyEvent,
        generation: u64,
    },
    SystemGesture {
        event: crate::SystemGestureEvent,
        /// Local routing generation; stale queued events must never cross a handoff.
        generation: u64,
    },
    EmergencyRelease,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureOptions {
    pub suppress_local: bool,
    pub capture_pointer: bool,
    pub capture_keyboard: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("platform capability is unsupported: {0}")]
    Unsupported(String),
    #[error("required permission is missing: {0}")]
    PermissionDenied(String),
    #[error("platform input operation failed: {0}")]
    Operation(String),
}

pub trait InputCapturePort: Send + Sync {
    fn capabilities(&self) -> PlatformCapabilities;
    fn start(&self, options: CaptureOptions)
        -> Result<Receiver<CapturedInputEvent>, PlatformError>;
    /// Switch between observing local input and exclusively capturing it for
    /// a remote target. Implementations must make disabling suppression
    /// recoverable even after a failed handoff.
    fn set_suppress_local(&self, suppress: bool) -> Result<(), PlatformError>;
    /// Enable platform-native event capture for the current target. The
    /// default is a no-op so portable backends remain source-compatible.
    fn set_native_quartz_capture_enabled(&self, _enabled: bool) -> Result<(), PlatformError> {
        Ok(())
    }
    fn set_consumer_capture(&self, _generation: u64, _mask: u32) {}
    fn set_consumer_shortcuts(&self, _shortcuts: Vec<crate::ConsumerShortcut>) {}
    /// Repair native cursor visibility if focus/Space changes revealed it.
    fn maintain_cursor_visibility(&self) -> Result<(), PlatformError> {
        Ok(())
    }
    /// Zero disables system gestures. A new nonzero generation starts a new route.
    /// Only gestures within the negotiated format may be suppressed locally.
    fn set_system_gesture_capture_generation(
        &self,
        _generation: u64,
        _format_version: u32,
    ) -> Result<(), PlatformError> {
        Ok(())
    }
    /// Return the pointer in the platform's global logical display space.
    fn current_pointer_position(&self) -> Result<LogicalPoint, PlatformError>;
    fn stop(&self) -> Result<(), PlatformError>;
}

pub trait InputInjectionPort: Send + Sync {
    fn capabilities(&self) -> PlatformCapabilities;
    fn place_pointer(
        &self,
        display: &DisplayId,
        logical_point: LogicalPoint,
    ) -> Result<(), PlatformError>;
    fn apply_keyboard(&self, event: &MappedKeyboardEvent) -> Result<(), PlatformError>;
    fn pointer_button(
        &self,
        hid_usage: u16,
        down: bool,
        click_count: u8,
    ) -> Result<(), PlatformError>;
    fn scroll(&self, event: ScrollEvent) -> Result<(), PlatformError>;
    /// Decode and post a flattened Quartz scroll event. Implementations must
    /// validate both the payload bound and the reconstructed event type.
    fn native_quartz_scroll(&self, _data: &[u8]) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported(
            "native Quartz event injection is unavailable".into(),
        ))
    }
    fn release_all(&self) -> Result<(), PlatformError>;
    fn consumer_key(
        &self,
        _event: crate::ConsumerKeyEvent,
        _display: &DisplayId,
    ) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported(
            "consumer keys are unavailable".into(),
        ))
    }
    /// Start/refresh slow native capability discovery off the input thread.
    fn refresh_consumer_capabilities(&self) {}
    fn release_consumer_keys(&self) {}
    fn system_gesture(&self, _event: crate::SystemGestureEvent) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported(
            "system gesture injection is unavailable".into(),
        ))
    }
    /// Safety maintenance for a missing terminal frame; must not start native resources.
    fn maintain_system_gesture(&self) -> Result<bool, PlatformError> {
        Ok(false)
    }
}

pub trait DisplayInventoryPort: Send + Sync {
    fn inventory(&self) -> Result<DisplayInventory, PlatformError>;
}

#[cfg(test)]
mod application_target_tests {
    use super::*;

    #[test]
    fn application_pointer_is_a_target_without_implying_global_injection_or_capture() {
        let caps = PlatformCapabilities {
            can_inject_app_pointer: true,
            ..Default::default()
        };
        assert!(caps.can_target());
        assert!(!caps.can_source());
        assert!(!caps.can_inject_absolute_pointer);
        assert!(!caps.can_inject_keyboard);
        assert!(!PlatformCapabilities::default().can_target());
        assert!(!PlatformCapabilities {
            can_inject_absolute_pointer: true,
            ..Default::default()
        }
        .can_target());
    }
}
