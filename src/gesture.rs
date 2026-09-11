//! Bounded system-gesture semantics, independent of native event serialization.

use crate::proto;

pub const SYSTEM_GESTURE_FORMAT_VERSION: u32 = 1;
pub const SYSTEM_PINCH_FORMAT_VERSION: u32 = 2;
pub const SYSTEM_GESTURE_CONTACTS_FORMAT_VERSION: u32 = 3;
pub const MAX_SYSTEM_GESTURE_FORMAT_VERSION: u32 = SYSTEM_GESTURE_CONTACTS_FORMAT_VERSION;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SystemGestureEvent {
    /// 1: horizontal swipe, 2: vertical swipe, 3: system pinch/spread.
    pub axis: u32,
    pub phase: u32,
    pub progress: f64,
    pub velocity_x: f64,
    pub velocity_y: f64,
    /// Native direction metadata for v2 gestures; v1 always used false.
    #[serde(default)]
    pub inverted_from_device: bool,
    /// Physical contacts for v3: 2 for pinch, 3/4 for a system swipe.
    /// Zero retains the legacy receiver-selected contact count.
    #[serde(default)]
    pub finger_count: u32,
}

impl SystemGestureEvent {
    pub fn validate(self) -> Result<Self, &'static str> {
        if !matches!(self.axis, 1..=3) || !matches!(self.phase, 1 | 2 | 4 | 8) {
            return Err("unsupported system gesture axis or phase");
        }
        if !matches!(
            (self.axis, self.finger_count),
            (1 | 2, 0 | 3 | 4) | (3, 0 | 2)
        ) {
            return Err("unsupported system gesture contact count");
        }
        if !self.progress.is_finite()
            || self.progress.abs() > 4.0
            || !self.velocity_x.is_finite()
            || self.velocity_x.abs() > 1_000.0
            || !self.velocity_y.is_finite()
            || self.velocity_y.abs() > 1_000.0
        {
            return Err("system gesture value is non-finite or out of bounds");
        }
        Ok(self)
    }

    /// Swipes retain v1 interoperability, pinch requires v2, and an explicit
    /// physical contact count requires v3.
    pub fn format_version(self) -> u32 {
        if self.finger_count != 0 {
            SYSTEM_GESTURE_CONTACTS_FORMAT_VERSION
        } else if self.axis == 3 || self.inverted_from_device {
            SYSTEM_PINCH_FORMAT_VERSION
        } else {
            SYSTEM_GESTURE_FORMAT_VERSION
        }
    }

    pub fn validate_format(self, version: u32) -> Result<Self, &'static str> {
        if !(self.format_version()..=MAX_SYSTEM_GESTURE_FORMAT_VERSION).contains(&version) {
            return Err("unsupported system gesture format");
        }
        self.validate()
    }

    pub fn cancelled(self) -> Self {
        Self {
            phase: 8,
            velocity_x: 0.0,
            velocity_y: 0.0,
            ..self
        }
    }
}

impl TryFrom<proto::SystemGesture> for SystemGestureEvent {
    type Error = &'static str;

    fn try_from(value: proto::SystemGesture) -> Result<Self, Self::Error> {
        Self {
            axis: value.axis,
            phase: value.phase,
            progress: value.progress,
            velocity_x: value.velocity_x,
            velocity_y: value.velocity_y,
            inverted_from_device: value.inverted_from_device,
            finger_count: value.finger_count,
        }
        .validate_format(value.format_version)
    }
}

impl From<SystemGestureEvent> for proto::SystemGesture {
    fn from(value: SystemGestureEvent) -> Self {
        Self {
            format_version: value.format_version(),
            axis: value.axis,
            phase: value.phase,
            progress: value.progress,
            velocity_x: value.velocity_x,
            velocity_y: value.velocity_y,
            inverted_from_device: value.inverted_from_device,
            finger_count: value.finger_count,
        }
    }
}

/// Missing starts never synthesize input. A replacement begin cancels the old
/// sequence first; release, disconnect and handoff all use the same cancellation.
#[derive(Debug, Default)]
pub struct SystemGestureSequence {
    active: Option<SystemGestureEvent>,
}

impl SystemGestureSequence {
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub fn apply(
        &mut self,
        event: SystemGestureEvent,
    ) -> Result<Vec<SystemGestureEvent>, &'static str> {
        event.validate()?;
        let mut output = Vec::with_capacity(2);
        if event.phase == 1 {
            output.extend(self.cancel());
        } else if self.active.is_none_or(|active| {
            active.axis != event.axis
                || active.inverted_from_device != event.inverted_from_device
                || active.finger_count != event.finger_count
        }) {
            // A tail from a prior route must not start or mutate an animation.
            return Ok(output);
        }
        self.active = (!matches!(event.phase, 4 | 8)).then_some(event);
        output.push(event);
        Ok(output)
    }

    pub fn cancel(&mut self) -> Option<SystemGestureEvent> {
        self.active.take().map(SystemGestureEvent::cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(phase: u32) -> SystemGestureEvent {
        SystemGestureEvent {
            axis: 1,
            phase,
            progress: -0.4,
            velocity_x: -5.0,
            velocity_y: -5.0,
            inverted_from_device: false,
            finger_count: 0,
        }
    }

    #[test]
    fn validates_wire_values_and_preserves_measured_direction() {
        assert_eq!(
            SystemGestureEvent::try_from(proto::SystemGesture::from(event(2))),
            Ok(event(2))
        );
        for progress in [f64::NAN, f64::INFINITY, 4.01] {
            assert!(SystemGestureEvent {
                progress,
                ..event(1)
            }
            .validate()
            .is_err());
        }
        for axis in [0, 4, u32::MAX] {
            assert!(SystemGestureEvent { axis, ..event(1) }.validate().is_err());
        }
        for phase in [0, 3, 128] {
            assert!(event(phase).validate().is_err());
        }
        assert!(SystemGestureEvent {
            velocity_x: f64::NAN,
            ..event(1)
        }
        .validate()
        .is_err());
        assert!(SystemGestureEvent {
            velocity_y: 1001.0,
            ..event(1)
        }
        .validate()
        .is_err());
        assert!(SystemGestureEvent::try_from(proto::SystemGesture {
            format_version: 4,
            ..event(1).into()
        })
        .is_err());
        for (axis, finger_count) in [(1, 2), (2, 2), (3, 3), (3, 4), (3, 5)] {
            assert!(SystemGestureEvent {
                axis,
                finger_count,
                ..event(1)
            }
            .validate()
            .is_err());
        }
    }

    #[test]
    fn pinch_requires_v2_while_swipes_remain_v1() {
        assert_eq!(proto::SystemGesture::from(event(1)).format_version, 1);
        for progress in [-0.5, 0.5] {
            for phase in [1, 2, 4, 8] {
                let pinch = SystemGestureEvent {
                    axis: 3,
                    progress,
                    ..event(phase)
                };
                let wire = proto::SystemGesture::from(pinch);
                assert_eq!(wire.format_version, 2);
                assert_eq!(SystemGestureEvent::try_from(wire), Ok(pinch));
                assert!(SystemGestureEvent::try_from(proto::SystemGesture {
                    format_version: 1,
                    ..wire
                })
                .is_err());
            }
        }
    }

    #[test]
    fn direction_metadata_is_versioned_and_cannot_change_mid_gesture() {
        use prost::Message;
        let pinch = SystemGestureEvent {
            axis: 3,
            inverted_from_device: true,
            ..event(1)
        };
        let wire = proto::SystemGesture::from(pinch);
        let decoded = proto::SystemGesture::decode(wire.encode_to_vec().as_slice()).unwrap();
        assert_eq!(SystemGestureEvent::try_from(decoded), Ok(pinch));
        let mut sequence = SystemGestureSequence::default();
        sequence.apply(pinch).unwrap();
        assert!(sequence
            .apply(SystemGestureEvent {
                phase: 2,
                inverted_from_device: false,
                ..pinch
            })
            .unwrap()
            .is_empty());
        assert_eq!(sequence.cancel(), Some(pinch.cancelled()));
        assert!(pinch.cancelled().inverted_from_device);
        assert!(SystemGestureEvent {
            inverted_from_device: true,
            ..event(1)
        }
        .validate_format(1)
        .is_err());
    }

    #[test]
    fn contact_count_requires_v3_and_cannot_change_mid_gesture() {
        let swipe = SystemGestureEvent {
            finger_count: 3,
            ..event(1)
        };
        let wire = proto::SystemGesture::from(swipe);
        assert_eq!(wire.format_version, 3);
        assert_eq!(SystemGestureEvent::try_from(wire), Ok(swipe));
        assert!(SystemGestureEvent::try_from(proto::SystemGesture {
            format_version: 2,
            ..wire
        })
        .is_err());
        let mut sequence = SystemGestureSequence::default();
        assert_eq!(sequence.apply(swipe).unwrap(), vec![swipe]);
        assert!(sequence
            .apply(SystemGestureEvent {
                phase: 2,
                finger_count: 4,
                ..swipe
            })
            .unwrap()
            .is_empty());
        assert_eq!(sequence.cancel(), Some(swipe.cancelled()));
    }

    #[test]
    fn negotiation_keeps_legacy_swipes_and_requires_explicit_new_features() {
        let modern = crate::PlatformCapabilities {
            can_capture_system_gestures: true,
            can_inject_system_gestures: true,
            system_gesture_format_version: 3,
            ..Default::default()
        };
        let legacy = crate::PlatformCapabilities {
            system_gesture_format_version: 0,
            ..modern.clone()
        };
        assert_eq!(modern.negotiated_system_gesture_version(&modern), 3);
        assert_eq!(modern.negotiated_system_gesture_version(&legacy), 1);
        assert_eq!(legacy.negotiated_system_gesture_version(&modern), 1);
        assert_eq!(
            modern.negotiated_system_gesture_version(&crate::PlatformCapabilities::default()),
            0
        );
        assert_eq!(
            crate::PlatformCapabilities::default().negotiated_system_gesture_version(&modern),
            0
        );
        let future = crate::PlatformCapabilities {
            system_gesture_format_version: u32::MAX,
            ..modern.clone()
        };
        assert_eq!(modern.negotiated_system_gesture_version(&future), 3);
    }

    #[test]
    fn pinch_is_cancelled_on_replacement_and_release_and_ignores_orphan_tails() {
        let pinch = SystemGestureEvent {
            axis: 3,
            ..event(1)
        };
        let mut sequence = SystemGestureSequence::default();
        assert!(sequence
            .apply(SystemGestureEvent { phase: 2, ..pinch })
            .unwrap()
            .is_empty());
        assert_eq!(sequence.apply(pinch).unwrap(), vec![pinch]);
        assert!(sequence.apply(event(2)).unwrap().is_empty());
        assert_eq!(
            sequence.apply(event(1)).unwrap(),
            vec![pinch.cancelled(), event(1)]
        );
        assert_eq!(
            sequence.apply(pinch).unwrap(),
            vec![event(1).cancelled(), pinch]
        );
        assert_eq!(sequence.cancel(), Some(pinch.cancelled()));
        assert!(sequence
            .apply(SystemGestureEvent { phase: 4, ..pinch })
            .unwrap()
            .is_empty());
    }

    #[test]
    fn release_and_replacement_cancel_but_orphan_tails_do_not_inject() {
        let mut sequence = SystemGestureSequence::default();
        assert!(sequence.apply(event(2)).unwrap().is_empty());
        assert_eq!(sequence.apply(event(1)).unwrap(), vec![event(1)]);
        assert_eq!(sequence.apply(event(2)).unwrap(), vec![event(2)]);
        assert_eq!(
            sequence.apply(event(1)).unwrap(),
            vec![event(2).cancelled(), event(1)]
        );
        assert_eq!(sequence.cancel(), Some(event(1).cancelled()));
        assert_eq!(sequence.cancel(), None);
        assert!(sequence.apply(event(4)).unwrap().is_empty());
        sequence.apply(event(1)).unwrap();
        assert!(sequence
            .apply(SystemGestureEvent {
                axis: 2,
                ..event(2)
            })
            .unwrap()
            .is_empty());
        sequence.apply(event(4)).unwrap();
        assert_eq!(sequence.cancel(), None);
    }
}
