use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::OsFamily;

pub const HID_KEY_A: u16 = 0x04;
pub const HID_KEY_C: u16 = 0x06;
pub const HID_KEY_V: u16 = 0x19;
pub const HID_KEY_X: u16 = 0x1b;
pub const HID_KEY_Z: u16 = 0x1d;
pub const HID_KEY_TAB: u16 = 0x2b;
pub const HID_LEFT_CONTROL: u16 = 0xe0;
pub const HID_LEFT_SHIFT: u16 = 0xe1;
pub const HID_LEFT_ALT: u16 = 0xe2;
pub const HID_LEFT_META: u16 = 0xe3;
/// ArcRelay-reserved physical-key extension for the macOS Fn/Globe modifier.
///
/// Apple exposes this key through Quartz rather than the USB HID keyboard
/// usage page. Non-macOS injection backends deliberately ignore this value.
pub const HID_KEY_FUNCTION: u16 = u16::MAX;

pub const MAX_SIMULATED_KEYBOARD_TEXT_CHARS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimulatedKeyStroke {
    pub hid_usage: u16,
    pub shift: bool,
}

/// Convert printable ASCII text into US-keyboard HID strokes. This deliberately
/// excludes control characters and non-ASCII text: physical key injection is
/// intended for remote applications that reject clipboard and Unicode events.
pub fn simulated_key_strokes(text: &str) -> Result<Vec<SimulatedKeyStroke>, &'static str> {
    let character_count = text.chars().count();
    if character_count == 0 {
        return Err("simulated keyboard input cannot be empty");
    }
    if character_count > MAX_SIMULATED_KEYBOARD_TEXT_CHARS {
        return Err("simulated keyboard input is limited to 256 characters");
    }

    text.chars()
        .map(|character| {
            let (hid_usage, shift) = match character {
                'a'..='z' => (0x04 + (character as u16 - 'a' as u16), false),
                'A'..='Z' => (0x04 + (character as u16 - 'A' as u16), true),
                '1'..='9' => (0x1E + (character as u16 - '1' as u16), false),
                '0' => (0x27, false),
                ' ' => (0x2C, false),
                '-' => (0x2D, false),
                '_' => (0x2D, true),
                '=' => (0x2E, false),
                '+' => (0x2E, true),
                '[' => (0x2F, false),
                '{' => (0x2F, true),
                ']' => (0x30, false),
                '}' => (0x30, true),
                '\\' => (0x31, false),
                '|' => (0x31, true),
                ';' => (0x33, false),
                ':' => (0x33, true),
                '\'' => (0x34, false),
                '"' => (0x34, true),
                '`' => (0x35, false),
                '~' => (0x35, true),
                ',' => (0x36, false),
                '<' => (0x36, true),
                '.' => (0x37, false),
                '>' => (0x37, true),
                '/' => (0x38, false),
                '?' => (0x38, true),
                '!' => (0x1E, true),
                '@' => (0x1F, true),
                '#' => (0x20, true),
                '$' => (0x21, true),
                '%' => (0x22, true),
                '^' => (0x23, true),
                '&' => (0x24, true),
                '*' => (0x25, true),
                '(' => (0x26, true),
                ')' => (0x27, true),
                _ => return Err("simulated keyboard input supports printable ASCII only"),
            };
            Ok(SimulatedKeyStroke { hid_usage, shift })
        })
        .collect()
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ts_rs::TS,
)]
pub enum SemanticAction {
    SelectAll,
    Copy,
    Paste,
    Cut,
    Undo,
    Redo,
    ApplicationSwitch,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct KeyChord {
    pub modifiers: BTreeSet<u16>,
    pub key: u16,
}

impl KeyChord {
    pub fn new(modifiers: impl IntoIterator<Item = u16>, key: u16) -> Self {
        Self {
            modifiers: modifiers.into_iter().collect(),
            key,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub enum KeyboardProfileKind {
    Productivity,
    Terminal,
    Ide,
    RemoteDesktop,
    GameRaw,
    Presentation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub enum TextStrategy {
    UseTargetLayout,
    FollowSourceText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct KeyboardProfile {
    pub name: String,
    pub kind: KeyboardProfileKind,
    pub revision: u64,
    pub text_strategy: TextStrategy,
    pub semantic_overrides: Vec<KeyboardMappingRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct KeyboardMappingRule {
    pub source: KeyChord,
    pub action: SemanticAction,
}

impl KeyboardProfile {
    pub fn built_in(kind: KeyboardProfileKind) -> Self {
        Self {
            name: format!("{kind:?}"),
            kind,
            revision: 1,
            text_strategy: TextStrategy::UseTargetLayout,
            semantic_overrides: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MappedKeyboardEvent {
    Physical { hid_usage: u16, down: bool },
    Semantic { action: SemanticAction, down: bool },
    TextCommit(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LockedChord {
    source: KeyChord,
    mapped: Option<SemanticAction>,
}

/// Locks mapping context at the first key-down until every chord key releases.
#[derive(Debug, Clone)]
pub struct KeyboardMappingEngine {
    source_os: OsFamily,
    target_os: OsFamily,
    profile: KeyboardProfile,
    application_profile: Option<KeyboardProfileKind>,
    raw_override: bool,
    pressed: BTreeSet<u16>,
    locked: Option<LockedChord>,
}

impl KeyboardMappingEngine {
    pub fn new(source_os: OsFamily, target_os: OsFamily, profile: KeyboardProfile) -> Self {
        Self {
            source_os,
            target_os,
            profile,
            application_profile: None,
            raw_override: false,
            pressed: BTreeSet::new(),
            locked: None,
        }
    }

    pub fn set_application_profile(&mut self, profile: Option<KeyboardProfileKind>) {
        self.application_profile = profile;
    }

    pub fn set_raw_override(&mut self, enabled: bool) {
        self.raw_override = enabled;
    }

    pub fn map_key(&mut self, hid_usage: u16, down: bool) -> MappedKeyboardEvent {
        if down {
            if is_modifier(hid_usage) {
                self.pressed.insert(hid_usage);
                return MappedKeyboardEvent::Physical { hid_usage, down };
            }
            let chord = KeyChord::new(self.pressed.iter().copied(), hid_usage);
            let mapped = self.semantic_action(&chord);
            self.locked = Some(LockedChord {
                source: chord,
                mapped,
            });
            self.pressed.insert(hid_usage);
            mapped.map_or(
                MappedKeyboardEvent::Physical { hid_usage, down },
                |action| MappedKeyboardEvent::Semantic { action, down },
            )
        } else {
            self.pressed.remove(&hid_usage);
            let mapped = self
                .locked
                .as_ref()
                .filter(|locked| locked.source.key == hid_usage)
                .and_then(|locked| locked.mapped);
            if self.pressed.is_empty()
                || self
                    .locked
                    .as_ref()
                    .is_some_and(|locked| locked.source.key == hid_usage)
            {
                self.locked = None;
            }
            mapped.map_or(
                MappedKeyboardEvent::Physical { hid_usage, down },
                |action| MappedKeyboardEvent::Semantic { action, down },
            )
        }
    }

    pub fn map_text(&self, text: String) -> Option<MappedKeyboardEvent> {
        (self.profile.text_strategy == TextStrategy::FollowSourceText)
            .then_some(MappedKeyboardEvent::TextCommit(text))
    }

    fn semantic_action(&self, chord: &KeyChord) -> Option<SemanticAction> {
        if self.source_os == self.target_os
            || self.raw_override
            || self.profile.kind == KeyboardProfileKind::GameRaw
            || self.application_profile == Some(KeyboardProfileKind::GameRaw)
            || self.application_profile == Some(KeyboardProfileKind::RemoteDesktop)
        {
            return None;
        }
        if self.application_profile == Some(KeyboardProfileKind::Terminal)
            && chord.modifiers.contains(&HID_LEFT_CONTROL)
        {
            return None;
        }
        self.profile
            .semantic_overrides
            .iter()
            .find(|rule| &rule.source == chord)
            .map(|rule| rule.action)
            .or_else(|| system_semantic(self.source_os, self.target_os, chord))
    }
}

fn is_modifier(hid_usage: u16) -> bool {
    (0xe0..=0xe7).contains(&hid_usage) || hid_usage == HID_KEY_FUNCTION
}

fn system_semantic(source: OsFamily, target: OsFamily, chord: &KeyChord) -> Option<SemanticAction> {
    let command_modifier = if source == OsFamily::MacOs {
        HID_LEFT_META
    } else {
        HID_LEFT_CONTROL
    };
    if !chord.modifiers.contains(&command_modifier) {
        if chord.modifiers.contains(&HID_LEFT_ALT) && chord.key == HID_KEY_TAB {
            return Some(SemanticAction::ApplicationSwitch);
        }
        return None;
    }
    let common_desktop_pair = matches!(
        (source, target),
        (OsFamily::Windows, OsFamily::MacOs)
            | (OsFamily::MacOs, OsFamily::Windows)
            | (OsFamily::LinuxX11 | OsFamily::LinuxWayland, OsFamily::MacOs)
            | (OsFamily::MacOs, OsFamily::LinuxX11 | OsFamily::LinuxWayland)
            | (
                OsFamily::Windows,
                OsFamily::LinuxX11 | OsFamily::LinuxWayland
            )
            | (
                OsFamily::LinuxX11 | OsFamily::LinuxWayland,
                OsFamily::Windows
            )
    );
    if !common_desktop_pair && source != target {
        return None;
    }
    match chord.key {
        HID_KEY_A => Some(SemanticAction::SelectAll),
        HID_KEY_C => Some(SemanticAction::Copy),
        HID_KEY_V => Some(SemanticAction::Paste),
        HID_KEY_X => Some(SemanticAction::Cut),
        HID_KEY_Z if chord.modifiers.contains(&HID_LEFT_SHIFT) => Some(SemanticAction::Redo),
        HID_KEY_Z => Some(SemanticAction::Undo),
        HID_KEY_TAB => Some(SemanticAction::ApplicationSwitch),
        _ => None,
    }
}

pub fn target_chord(action: SemanticAction, target: OsFamily) -> KeyChord {
    let command = if target == OsFamily::MacOs {
        HID_LEFT_META
    } else {
        HID_LEFT_CONTROL
    };
    match action {
        SemanticAction::SelectAll => KeyChord::new([command], HID_KEY_A),
        SemanticAction::Copy => KeyChord::new([command], HID_KEY_C),
        SemanticAction::Paste => KeyChord::new([command], HID_KEY_V),
        SemanticAction::Cut => KeyChord::new([command], HID_KEY_X),
        SemanticAction::Undo => KeyChord::new([command], HID_KEY_Z),
        SemanticAction::Redo => KeyChord::new([command, HID_LEFT_SHIFT], HID_KEY_Z),
        SemanticAction::ApplicationSwitch => KeyChord::new(
            [if target == OsFamily::MacOs {
                HID_LEFT_META
            } else {
                HID_LEFT_ALT
            }],
            HID_KEY_TAB,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulated_key_strokes_cover_letters_digits_and_symbols() {
        let strokes = simulated_key_strokes("aZ0! _?/\\").unwrap();
        assert_eq!(
            strokes,
            vec![
                SimulatedKeyStroke {
                    hid_usage: 0x04,
                    shift: false
                },
                SimulatedKeyStroke {
                    hid_usage: 0x1D,
                    shift: true
                },
                SimulatedKeyStroke {
                    hid_usage: 0x27,
                    shift: false
                },
                SimulatedKeyStroke {
                    hid_usage: 0x1E,
                    shift: true
                },
                SimulatedKeyStroke {
                    hid_usage: 0x2C,
                    shift: false
                },
                SimulatedKeyStroke {
                    hid_usage: 0x2D,
                    shift: true
                },
                SimulatedKeyStroke {
                    hid_usage: 0x38,
                    shift: true
                },
                SimulatedKeyStroke {
                    hid_usage: 0x38,
                    shift: false
                },
                SimulatedKeyStroke {
                    hid_usage: 0x31,
                    shift: false
                },
            ]
        );
    }

    #[test]
    fn simulated_key_strokes_accept_every_printable_ascii_character() {
        let text: String = (0x20_u8..=0x7E).map(char::from).collect();
        assert_eq!(simulated_key_strokes(&text).unwrap().len(), 95);
    }

    #[test]
    fn simulated_key_strokes_reject_controls_unicode_and_oversized_text() {
        assert!(simulated_key_strokes("").is_err());
        assert!(simulated_key_strokes("line\nfeed").is_err());
        assert!(simulated_key_strokes("中文").is_err());
        assert!(simulated_key_strokes(&"a".repeat(MAX_SIMULATED_KEYBOARD_TEXT_CHARS + 1)).is_err());
    }

    #[test]
    fn productivity_maps_windows_copy_to_macos_semantics() {
        let mut engine = KeyboardMappingEngine::new(
            OsFamily::Windows,
            OsFamily::MacOs,
            KeyboardProfile::built_in(KeyboardProfileKind::Productivity),
        );
        engine.map_key(HID_LEFT_CONTROL, true);
        assert_eq!(
            engine.map_key(HID_KEY_C, true),
            MappedKeyboardEvent::Semantic {
                action: SemanticAction::Copy,
                down: true,
            }
        );
        assert_eq!(
            target_chord(SemanticAction::Copy, OsFamily::MacOs),
            KeyChord::new([HID_LEFT_META], HID_KEY_C)
        );
    }

    #[test]
    fn terminal_profile_preserves_control_c() {
        let mut engine = KeyboardMappingEngine::new(
            OsFamily::Windows,
            OsFamily::MacOs,
            KeyboardProfile::built_in(KeyboardProfileKind::Productivity),
        );
        engine.set_application_profile(Some(KeyboardProfileKind::Terminal));
        engine.map_key(HID_LEFT_CONTROL, true);
        assert_eq!(
            engine.map_key(HID_KEY_C, true),
            MappedKeyboardEvent::Physical {
                hid_usage: HID_KEY_C,
                down: true,
            }
        );
    }

    #[test]
    fn mapping_stays_locked_until_key_up() {
        let mut engine = KeyboardMappingEngine::new(
            OsFamily::Windows,
            OsFamily::MacOs,
            KeyboardProfile::built_in(KeyboardProfileKind::Productivity),
        );
        engine.map_key(HID_LEFT_CONTROL, true);
        engine.map_key(HID_KEY_V, true);
        engine.set_application_profile(Some(KeyboardProfileKind::Terminal));
        assert_eq!(
            engine.map_key(HID_KEY_V, false),
            MappedKeyboardEvent::Semantic {
                action: SemanticAction::Paste,
                down: false,
            }
        );
    }

    #[test]
    fn application_switch_uses_the_native_modifier_on_each_target() {
        let mut engine = KeyboardMappingEngine::new(
            OsFamily::MacOs,
            OsFamily::Windows,
            KeyboardProfile::built_in(KeyboardProfileKind::Productivity),
        );
        engine.map_key(HID_LEFT_META, true);
        assert_eq!(
            engine.map_key(HID_KEY_TAB, true),
            MappedKeyboardEvent::Semantic {
                action: SemanticAction::ApplicationSwitch,
                down: true,
            }
        );
        assert_eq!(
            target_chord(SemanticAction::ApplicationSwitch, OsFamily::Windows),
            KeyChord::new([HID_LEFT_ALT], HID_KEY_TAB)
        );
    }

    #[test]
    fn same_os_preserves_the_native_application_switch_sequence() {
        let mut engine = KeyboardMappingEngine::new(
            OsFamily::MacOs,
            OsFamily::MacOs,
            KeyboardProfile::built_in(KeyboardProfileKind::Productivity),
        );

        for (hid_usage, down) in [
            (HID_LEFT_META, true),
            (HID_KEY_TAB, true),
            (HID_KEY_TAB, false),
            (HID_KEY_TAB, true),
            (HID_KEY_TAB, false),
            (HID_LEFT_META, false),
        ] {
            assert_eq!(
                engine.map_key(hid_usage, down),
                MappedKeyboardEvent::Physical { hid_usage, down }
            );
        }
    }

    #[test]
    fn same_os_bypasses_custom_semantic_overrides() {
        let mut profile = KeyboardProfile::built_in(KeyboardProfileKind::Productivity);
        profile.semantic_overrides.push(KeyboardMappingRule {
            source: KeyChord::new([HID_LEFT_META], HID_KEY_C),
            action: SemanticAction::Copy,
        });
        let mut engine = KeyboardMappingEngine::new(OsFamily::MacOs, OsFamily::MacOs, profile);

        engine.map_key(HID_LEFT_META, true);
        assert_eq!(
            engine.map_key(HID_KEY_C, true),
            MappedKeyboardEvent::Physical {
                hid_usage: HID_KEY_C,
                down: true,
            }
        );
    }

    #[test]
    fn macos_function_extension_is_a_modifier() {
        assert!(is_modifier(HID_KEY_FUNCTION));
    }

    #[test]
    fn raw_override_bypasses_semantic_mapping() {
        let mut engine = KeyboardMappingEngine::new(
            OsFamily::MacOs,
            OsFamily::Windows,
            KeyboardProfile::built_in(KeyboardProfileKind::Productivity),
        );
        engine.set_raw_override(true);
        engine.map_key(HID_LEFT_META, true);

        assert_eq!(
            engine.map_key(HID_KEY_C, true),
            MappedKeyboardEvent::Physical {
                hid_usage: HID_KEY_C,
                down: true,
            }
        );
    }
}
