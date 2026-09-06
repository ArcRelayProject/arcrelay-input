//! Whitelisted HID Consumer-page (0x0c) actions. These are not keyboard-page
//! F-keys and never participate in held-key replay across a screen handoff.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ts_rs::TS)]
#[repr(u8)]
pub enum ConsumerKey {
    // Stable capability-mask bit positions; never reorder or reuse them.
    PreviousTrack = 0,
    PlayPause = 1,
    NextTrack = 2,
    Mute = 3,
    VolumeDown = 4,
    VolumeUp = 5,
    BrightnessDown = 6,
    BrightnessUp = 7,
}

impl ConsumerKey {
    pub const ALL: [Self; 8] = [
        Self::PreviousTrack,
        Self::PlayPause,
        Self::NextTrack,
        Self::Mute,
        Self::VolumeDown,
        Self::VolumeUp,
        Self::BrightnessDown,
        Self::BrightnessUp,
    ];
    pub const MEDIA_MASK: u32 = 0x3f;
    pub const BRIGHTNESS_MASK: u32 = 0xc0;
    pub const ALL_MASK: u32 = 0xff;
    pub fn mask(self) -> u32 {
        1 << (self as u32)
    }
    pub fn usage(self) -> u16 {
        match self {
            Self::PreviousTrack => 0xb6,
            Self::PlayPause => 0xcd,
            Self::NextTrack => 0xb5,
            Self::Mute => 0xe2,
            Self::VolumeDown => 0xea,
            Self::VolumeUp => 0xe9,
            Self::BrightnessDown => 0x70,
            Self::BrightnessUp => 0x6f,
        }
    }
    pub fn from_usage(usage: u32) -> Result<Self, &'static str> {
        Self::ALL
            .into_iter()
            .find(|key| u32::from(key.usage()) == usage)
            .ok_or("unsupported consumer usage")
    }
    pub fn is_brightness(self) -> bool {
        self.mask() & Self::BRIGHTNESS_MASK != 0
    }
    pub fn repeats(self) -> bool {
        self.is_brightness() || matches!(self, Self::VolumeDown | Self::VolumeUp)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsumerKeyEvent {
    pub key: ConsumerKey,
    pub down: bool,
    pub repeat: bool,
}

impl TryFrom<crate::proto::ConsumerKey> for ConsumerKeyEvent {
    type Error = &'static str;
    fn try_from(value: crate::proto::ConsumerKey) -> Result<Self, Self::Error> {
        if value.repeat && !value.down {
            return Err("consumer release cannot repeat");
        }
        Ok(Self {
            key: ConsumerKey::from_usage(value.hid_usage)?,
            down: value.down,
            repeat: value.repeat,
        })
    }
}
impl From<ConsumerKeyEvent> for crate::proto::ConsumerKey {
    fn from(value: ConsumerKeyEvent) -> Self {
        Self {
            hid_usage: u32::from(value.key.usage()),
            down: value.down,
            repeat: value.repeat,
        }
    }
}

/// Toggle actions execute once; volume/brightness repeat only after a real down.
/// Native adapters emit balanced pulses, so release/disconnect cannot leave a
/// system key physically held. This state is deliberately not replayed at handoff.
#[derive(Default)]
pub struct ConsumerSequence {
    held: BTreeSet<ConsumerKey>,
}
impl ConsumerSequence {
    pub fn apply(&mut self, event: ConsumerKeyEvent) -> bool {
        if !event.down {
            self.held.remove(&event.key);
            return false;
        }
        if event.repeat {
            return event.key.repeats() && self.held.contains(&event.key);
        }
        self.held.insert(event.key)
    }
    pub fn clear(&mut self) {
        self.held.clear();
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ConsumerShortcut {
    pub key: u16,
    /// Ctrl=1, Alt/Option=2, Shift=4, Meta/Command=8; left/right are equivalent.
    pub modifiers: u8,
    pub action: ConsumerKey,
}
pub fn validate_consumer_shortcuts(shortcuts: &[ConsumerShortcut]) -> Result<(), &'static str> {
    if shortcuts.len() > 8 {
        return Err("at most eight consumer shortcuts are allowed");
    }
    let mut chords = BTreeSet::new();
    for shortcut in shortcuts {
        if shortcut.modifiers == 0
            || shortcut.modifiers > 15
            || !(0x04..=0x73).contains(&shortcut.key)
            || !chords.insert((shortcut.key, shortcut.modifiers))
        {
            return Err("consumer shortcut must be a unique modified keyboard key");
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsumerCaptureResult {
    Pass,
    Suppress,
    Forward {
        event: ConsumerKeyEvent,
        generation: u64,
    },
}

#[derive(Default)]
pub struct ConsumerCapture {
    generation: u64,
    mask: u32,
    held: BTreeMap<ConsumerKey, Option<u64>>,
    physical: BTreeSet<u16>,
    shortcuts: Vec<ConsumerShortcut>,
    shortcut_held: BTreeMap<u16, ConsumerKey>,
}
impl ConsumerCapture {
    pub fn set_route(&mut self, generation: u64, mask: u32) {
        self.generation = generation;
        self.mask = mask & ConsumerKey::ALL_MASK;
        // Keep pressed origins until up. A held key must not start on the next
        // device, and a swallowed down must not leak its up to the local OS.
    }
    pub fn set_shortcuts(&mut self, shortcuts: Vec<ConsumerShortcut>) {
        self.shortcuts = shortcuts;
    }
    pub fn reset(&mut self) {
        self.set_route(0, 0);
        self.held.clear();
        self.physical.clear();
        self.shortcut_held.clear();
    }
    pub fn route(&mut self, mut event: ConsumerKeyEvent) -> ConsumerCaptureResult {
        if event.down && self.held.contains_key(&event.key) {
            event.repeat = true;
        }
        let origin = if event.down {
            *self.held.entry(event.key).or_insert_with(|| {
                (!event.repeat && self.generation != 0 && self.mask & event.key.mask() != 0)
                    .then_some(self.generation)
            })
        } else {
            self.held.remove(&event.key).flatten()
        };
        match origin {
            Some(generation)
                if generation == self.generation && self.mask & event.key.mask() != 0 =>
            {
                ConsumerCaptureResult::Forward { event, generation }
            }
            Some(_) => ConsumerCaptureResult::Suppress,
            None => ConsumerCaptureResult::Pass,
        }
    }
    pub fn physical(&mut self, usage: u16, down: bool) -> Option<ConsumerCaptureResult> {
        let repeat = if down {
            !self.physical.insert(usage)
        } else {
            self.physical.remove(&usage);
            false
        };
        if let Some(key) = self.shortcut_held.get(&usage).copied() {
            if !down {
                self.shortcut_held.remove(&usage);
            }
            return Some(self.route_shortcut(ConsumerKeyEvent { key, down, repeat }));
        }
        if !down || repeat {
            return None;
        }
        let modifiers = self.physical.iter().fold(0, |mask, usage| {
            mask | match usage {
                0xe0 | 0xe4 => 1,
                0xe2 | 0xe6 => 2,
                0xe1 | 0xe5 => 4,
                0xe3 | 0xe7 => 8,
                _ => 0,
            }
        });
        let key = self
            .shortcuts
            .iter()
            .find(|binding| binding.key == usage && binding.modifiers == modifiers)?
            .action;
        if self.generation == 0 || self.mask & key.mask() == 0 {
            return None;
        }
        self.shortcut_held.insert(usage, key);
        Some(self.route_shortcut(ConsumerKeyEvent { key, down, repeat }))
    }
    fn route_shortcut(&mut self, event: ConsumerKeyEvent) -> ConsumerCaptureResult {
        match self.route(event) {
            // The physical key belongs to this binding once its down was
            // intercepted. If a simultaneous native key for the same action
            // ended the shared action sequence, never leak a raw shortcut tail.
            ConsumerCaptureResult::Pass => ConsumerCaptureResult::Suppress,
            result => result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlapping_hardware_key_cannot_leak_an_intercepted_shortcut_tail() {
        let mut capture = ConsumerCapture::default();
        capture.set_route(1, ConsumerKey::ALL_MASK);
        capture.set_shortcuts(vec![ConsumerShortcut {
            key: 0x4b,
            modifiers: 3,
            action: ConsumerKey::VolumeUp,
        }]);
        capture.physical(0xe0, true);
        capture.physical(0xe2, true);
        assert!(matches!(
            capture.physical(0x4b, true),
            Some(ConsumerCaptureResult::Forward { .. })
        ));
        capture.route(ConsumerKeyEvent {
            key: ConsumerKey::VolumeUp,
            down: true,
            repeat: false,
        });
        capture.route(ConsumerKeyEvent {
            key: ConsumerKey::VolumeUp,
            down: false,
            repeat: false,
        });
        assert_eq!(
            capture.physical(0x4b, true),
            Some(ConsumerCaptureResult::Suppress)
        );
        assert_eq!(
            capture.physical(0x4b, false),
            Some(ConsumerCaptureResult::Suppress)
        );
    }

    #[test]
    fn capabilities_are_opt_in_and_brightness_is_display_specific() {
        let display = crate::DisplayId::parse("panel").unwrap();
        assert_eq!(
            crate::PlatformCapabilities::default().consumer_mask_for_display(&display),
            0
        );
        let caps = crate::PlatformCapabilities {
            consumer_inject_mask: ConsumerKey::ALL_MASK,
            brightness_display_ids: vec!["panel".into()],
            ..Default::default()
        };
        assert_eq!(
            caps.consumer_mask_for_display(&display),
            ConsumerKey::ALL_MASK
        );
        assert_eq!(
            caps.consumer_mask_for_display(&crate::DisplayId::parse("other").unwrap()),
            ConsumerKey::MEDIA_MASK
        );
    }
    #[test]
    fn invalid_shortcuts_are_rejected_and_reset_does_not_keep_pressed_keys() {
        let binding = ConsumerShortcut {
            key: 0x4b,
            modifiers: 3,
            action: ConsumerKey::BrightnessUp,
        };
        assert!(validate_consumer_shortcuts(&[binding.clone(), binding.clone()]).is_err());
        assert!(validate_consumer_shortcuts(&[ConsumerShortcut {
            modifiers: 0,
            ..binding.clone()
        }])
        .is_err());
        let mut gate = ConsumerCapture::default();
        gate.set_shortcuts(vec![binding]);
        gate.set_route(1, 255);
        gate.route(event(ConsumerKey::VolumeUp, true, false));
        gate.reset();
        gate.set_route(2, 255);
        assert!(matches!(
            gate.route(event(ConsumerKey::VolumeUp, true, false)),
            ConsumerCaptureResult::Forward { generation: 2, .. }
        ));
    }
    fn event(key: ConsumerKey, down: bool, repeat: bool) -> ConsumerKeyEvent {
        ConsumerKeyEvent { key, down, repeat }
    }
    #[test]
    fn consumer_usage_whitelist_and_wire_roundtrip() {
        for key in ConsumerKey::ALL {
            let e = event(key, true, false);
            assert_eq!(
                ConsumerKeyEvent::try_from(crate::proto::ConsumerKey::from(e)).unwrap(),
                e
            );
        }
        assert!(ConsumerKey::from_usage(0x100e9).is_err());
        assert!(ConsumerKeyEvent::try_from(crate::proto::ConsumerKey {
            hid_usage: 0xe9,
            down: false,
            repeat: true
        })
        .is_err());
    }
    #[test]
    fn toggles_once_and_only_held_continuous_actions_repeat() {
        let mut sequence = ConsumerSequence::default();
        for key in ConsumerKey::ALL {
            assert!(!sequence.apply(event(key, true, true)));
            assert!(sequence.apply(event(key, true, false)));
            assert!(!sequence.apply(event(key, true, false)));
            assert_eq!(sequence.apply(event(key, true, true)), key.repeats());
            assert!(!sequence.apply(event(key, false, false)));
        }
        sequence.apply(event(ConsumerKey::VolumeUp, true, false));
        sequence.clear();
        assert!(!sequence.apply(event(ConsumerKey::VolumeUp, true, true)));
    }
    #[test]
    fn route_fences_hold_across_handoff_and_passes_unsupported_keys() {
        let mut gate = ConsumerCapture::default();
        gate.set_route(1, ConsumerKey::MEDIA_MASK);
        assert_eq!(
            gate.route(event(ConsumerKey::BrightnessUp, true, false)),
            ConsumerCaptureResult::Pass
        );
        assert!(matches!(
            gate.route(event(ConsumerKey::VolumeUp, true, false)),
            ConsumerCaptureResult::Forward { generation: 1, .. }
        ));
        gate.set_route(2, ConsumerKey::ALL_MASK);
        assert_eq!(
            gate.route(event(ConsumerKey::VolumeUp, true, true)),
            ConsumerCaptureResult::Suppress
        );
        assert_eq!(
            gate.route(event(ConsumerKey::VolumeUp, false, false)),
            ConsumerCaptureResult::Suppress
        );
        assert_eq!(
            gate.route(event(ConsumerKey::BrightnessUp, true, true)),
            ConsumerCaptureResult::Pass
        );
        assert!(matches!(
            gate.route(event(ConsumerKey::VolumeUp, true, false)),
            ConsumerCaptureResult::Forward { generation: 2, .. }
        ));
    }
    #[test]
    fn shortcut_key_up_is_paired_even_when_modifiers_release_first() {
        let mut gate = ConsumerCapture::default();
        gate.set_route(1, ConsumerKey::ALL_MASK);
        let bindings = vec![ConsumerShortcut {
            key: 0x4b,
            modifiers: 3,
            action: ConsumerKey::BrightnessUp,
        }];
        assert!(validate_consumer_shortcuts(&bindings).is_ok());
        gate.set_shortcuts(bindings);
        assert_eq!(gate.physical(0xe0, true), None);
        assert_eq!(gate.physical(0xe2, true), None);
        assert!(matches!(
            gate.physical(0x4b, true),
            Some(ConsumerCaptureResult::Forward { .. })
        ));
        gate.physical(0xe0, false);
        gate.physical(0xe2, false);
        assert!(matches!(
            gate.physical(0x4b, false),
            Some(ConsumerCaptureResult::Forward {
                event: ConsumerKeyEvent { down: false, .. },
                ..
            })
        ));
    }
}
