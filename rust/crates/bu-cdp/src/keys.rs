//! Key-combo parsing and CDP key-event planning, kept free of I/O so the
//! shortcut, text, and modifier rules can be unit-tested without a browser.

use anyhow::{anyhow, Result};

const ALT: i64 = 1;
const CONTROL: i64 = 2;
const META: i64 = 4;
const SHIFT: i64 = 8;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum KeyPhase {
    Down,
    Up,
}

/// One `Input.dispatchKeyEvent` call, minus its phase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KeyEvent {
    pub(crate) key: String,
    pub(crate) code: String,
    pub(crate) modifiers: i64,
    pub(crate) windows_virtual_key_code: Option<i64>,
    /// Text the key produces. A `keyDown` without it raises no `keypress`/`char`,
    /// so Enter would neither submit a form nor insert a newline.
    pub(crate) text: Option<String>,
    /// Chromium editing commands (`selectAll`, `paste`, …) to run with the event.
    pub(crate) commands: Vec<&'static str>,
}

impl KeyEvent {
    /// A bare event for `name`, resolving its `key`, side-specific `code`, and
    /// virtual key code. Carries no text and no commands.
    fn named(name: &str, modifiers: i64) -> Self {
        let (key, code, windows_virtual_key_code) = resolve_key(name, modifiers);
        Self {
            key,
            code,
            modifiers,
            windows_virtual_key_code,
            text: None,
            commands: Vec::new(),
        }
    }
}

/// Expands a combo such as `Meta+a`, `Enter`, or `Shift+Tab` into the full event
/// sequence: modifier downs, the main key down and up, modifier ups.
pub(crate) fn plan_key_press(combo: &str) -> Result<Vec<(KeyPhase, KeyEvent)>> {
    let (names, main) = split_key_combo(combo)?;

    let mut modifier_keys = Vec::with_capacity(names.len());
    let mut bits = 0_i64;
    for name in &names {
        let (key, bit) = canonical_modifier(name).ok_or_else(|| {
            anyhow!("unknown key modifier {name:?}: expected Alt, Control, Meta, or Shift")
        })?;
        modifier_keys.push(key);
        bits |= bit;
    }

    // A lone "Cmd" must reach the page as "Meta", the same as inside a combo.
    let main = canonical_modifier(main).map_or(main, |(key, _)| key);
    let mut press = KeyEvent::named(main, bits);
    if bits & (CONTROL | META) != 0 {
        // A synthetic accelerator does not run its shortcut by itself: Chromium
        // resolves editing shortcuts from a real OS key event, so without the
        // command `Meta+a` is a silent no-op that still reports success.
        press.commands = editing_commands(&press.key, bits);
    } else if bits & ALT == 0 {
        press.text = key_text(&press.key);
    }

    let mut events = Vec::with_capacity(modifier_keys.len() * 2 + 2);
    for key in &modifier_keys {
        events.push((KeyPhase::Down, KeyEvent::named(key, bits)));
    }
    let release = KeyEvent {
        commands: Vec::new(),
        text: None,
        ..press.clone()
    };
    events.push((KeyPhase::Down, press));
    events.push((KeyPhase::Up, release));
    for key in modifier_keys.iter().rev() {
        events.push((KeyPhase::Up, KeyEvent::named(key, 0)));
    }
    Ok(events)
}

/// The two events that run one Chromium editing command on the focused element.
///
/// Uses the platform accelerator (Cmd on macOS, Ctrl elsewhere) so the page also
/// sees the modifier state a real shortcut would carry.
pub(crate) fn editing_command_events(
    command: &'static str,
    key: &str,
) -> [(KeyPhase, KeyEvent); 2] {
    let accelerator = accelerator_modifier_bitmask();
    let mut down = KeyEvent::named(key, accelerator);
    down.commands = vec![command];
    [
        (KeyPhase::Down, down),
        (KeyPhase::Up, KeyEvent::named(key, accelerator)),
    ]
}

/// Splits `Meta+Shift+a` into its modifiers and its main key.
///
/// A trailing `+` (as in `Shift++`) keeps the literal `+` as the main key rather
/// than yielding an empty one.
fn split_key_combo(key: &str) -> Result<(Vec<&str>, &str)> {
    if key.is_empty() {
        return Err(anyhow!("key must not be empty"));
    }
    let Some((head, main_key)) = key.rsplit_once('+') else {
        return Ok((Vec::new(), key));
    };
    if main_key.is_empty() {
        // "Shift++" -> modifiers from "Shift+", main key "+".
        let modifiers = head.split('+').filter(|part| !part.is_empty()).collect();
        return Ok((modifiers, "+"));
    }
    let modifiers: Vec<&str> = head.split('+').collect();
    if modifiers.iter().any(|modifier| modifier.is_empty()) {
        return Err(anyhow!("malformed key combo {key:?}"));
    }
    Ok((modifiers, main_key))
}

/// The platform accelerator used for editing shortcuts (Cmd on macOS, Ctrl elsewhere).
fn accelerator_modifier_bitmask() -> i64 {
    if cfg!(target_os = "macos") {
        META
    } else {
        CONTROL
    }
}

/// A modifier's canonical `KeyboardEvent.key` and CDP bit, accepting the common
/// spellings. `None` for anything else, which the caller reports instead of
/// dropping: a silently ignored modifier turns a shortcut into a plain keypress.
fn canonical_modifier(name: &str) -> Option<(&'static str, i64)> {
    match name {
        "Alt" | "Option" => Some(("Alt", ALT)),
        "Control" | "Ctrl" => Some(("Control", CONTROL)),
        "Meta" | "Command" | "Cmd" => Some(("Meta", META)),
        "Shift" => Some(("Shift", SHIFT)),
        _ => None,
    }
}

/// The editing command a Ctrl/Meta shortcut stands for, if it has one.
fn editing_commands(key: &str, modifiers: i64) -> Vec<&'static str> {
    match key.to_ascii_lowercase().as_str() {
        "a" => vec!["selectAll"],
        "c" => vec!["copy"],
        "x" => vec!["cut"],
        "v" => vec!["paste"],
        "z" if modifiers & SHIFT != 0 => vec!["redo"],
        "z" => vec!["undo"],
        "y" => vec!["redo"],
        _ => Vec::new(),
    }
}

/// The text a plain key press inserts: Enter and Space, or one printable character.
fn key_text(key: &str) -> Option<String> {
    match key {
        "Enter" => Some("\r".to_owned()),
        _ => single_char(key)
            .filter(|char| !char.is_control())
            .map(String::from),
    }
}

/// Resolves a key name to its `key`, `code`, and `windowsVirtualKeyCode`.
///
/// `code` is physical and side-specific, so a modifier has to report `MetaLeft`
/// rather than `Meta` — Chromium drops the bare name and the page then sees an
/// empty `event.code`. Under Shift a letter's `key` is its capital.
fn resolve_key(name: &str, modifiers: i64) -> (String, String, Option<i64>) {
    let named = |key: &str, code: &str, vk: i64| (key.to_owned(), code.to_owned(), Some(vk));
    match name {
        "Enter" => named("Enter", "Enter", 13),
        "Backspace" => named("Backspace", "Backspace", 8),
        "Tab" => named("Tab", "Tab", 9),
        "Escape" => named("Escape", "Escape", 27),
        "Space" | " " => named(" ", "Space", 32),
        "PageUp" => named("PageUp", "PageUp", 33),
        "PageDown" => named("PageDown", "PageDown", 34),
        "End" => named("End", "End", 35),
        "Home" => named("Home", "Home", 36),
        "ArrowLeft" => named("ArrowLeft", "ArrowLeft", 37),
        "ArrowUp" => named("ArrowUp", "ArrowUp", 38),
        "ArrowRight" => named("ArrowRight", "ArrowRight", 39),
        "ArrowDown" => named("ArrowDown", "ArrowDown", 40),
        "Insert" => named("Insert", "Insert", 45),
        "Delete" => named("Delete", "Delete", 46),
        "Meta" => named("Meta", "MetaLeft", 91),
        "Control" => named("Control", "ControlLeft", 17),
        "Shift" => named("Shift", "ShiftLeft", 16),
        "Alt" => named("Alt", "AltLeft", 18),
        other => {
            if let Some(number) = function_key_number(other) {
                return named(other, other, 111 + number);
            }
            match single_char(other) {
                Some(char) if char.is_ascii_alphabetic() => {
                    let upper = char.to_ascii_uppercase();
                    let key = if modifiers & SHIFT != 0 { upper } else { char };
                    (key.to_string(), format!("Key{upper}"), Some(upper as i64))
                }
                Some(char) if char.is_ascii_digit() => {
                    (char.to_string(), format!("Digit{char}"), Some(char as i64))
                }
                // Punctuation and anything unnamed already is its own code.
                _ => (other.to_owned(), other.to_owned(), None),
            }
        }
    }
}

/// `F1`..`F12` as 1..=12.
fn function_key_number(name: &str) -> Option<i64> {
    let number: i64 = name.strip_prefix('F')?.parse().ok()?;
    (1..=12).contains(&number).then_some(number)
}

fn single_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let first = chars.next()?;
    chars.next().is_none().then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The main key's down and up. The plan is `m` modifier downs, the main down
    /// and up, then `m` modifier ups, so they sit in the middle.
    fn main_events(combo: &str) -> (KeyEvent, KeyEvent) {
        let events = plan_key_press(combo).expect("valid combo");
        let middle = events.len() / 2;
        assert_eq!(events[middle - 1].0, KeyPhase::Down, "{combo}");
        assert_eq!(events[middle].0, KeyPhase::Up, "{combo}");
        (events[middle - 1].1.clone(), events[middle].1.clone())
    }

    #[test]
    fn accelerator_combos_request_their_editing_command() {
        for (combo, command) in [
            ("Meta+a", "selectAll"),
            ("Control+v", "paste"),
            ("Ctrl+c", "copy"),
            ("Cmd+x", "cut"),
            ("Meta+z", "undo"),
            ("Meta+Shift+z", "redo"),
            ("Control+y", "redo"),
        ] {
            let (down, up) = main_events(combo);
            assert_eq!(down.commands, vec![command], "{combo}");
            assert!(up.commands.is_empty(), "{combo}: only the down carries it");
            assert_eq!(down.text, None, "{combo}: an accelerator inserts no text");
        }
    }

    #[test]
    fn accelerator_with_no_editing_meaning_carries_no_command() {
        let (down, _) = main_events("Control+k");
        assert!(down.commands.is_empty());
    }

    #[test]
    fn modifier_aliases_reach_the_page_under_their_canonical_name() {
        let events = plan_key_press("Cmd+Option+a").expect("valid combo");
        let keys: Vec<_> = events.iter().map(|(_, event)| event.key.as_str()).collect();
        assert_eq!(keys, ["Meta", "Alt", "a", "a", "Alt", "Meta"]);
        let codes: Vec<_> = events
            .iter()
            .map(|(_, event)| event.code.as_str())
            .collect();
        assert_eq!(
            codes,
            ["MetaLeft", "AltLeft", "KeyA", "KeyA", "AltLeft", "MetaLeft"]
        );
        // The main key sees the whole mask; the releases drop it.
        assert_eq!(events[2].1.modifiers, META | ALT);
        assert_eq!(events[4].1.modifiers, 0);

        let (lone, _) = main_events("Ctrl");
        assert_eq!(
            (lone.key.as_str(), lone.code.as_str()),
            ("Control", "ControlLeft")
        );
    }

    #[test]
    fn unknown_modifier_and_malformed_combos_are_errors() {
        assert!(plan_key_press("Hyper+v").is_err());
        assert!(plan_key_press("").is_err());
        assert!(plan_key_press("+a").is_err());
        assert!(plan_key_press("Shift++").is_ok());
    }

    #[test]
    fn enter_and_printable_keys_produce_text_so_a_char_event_fires() {
        assert_eq!(main_events("Enter").0.text.as_deref(), Some("\r"));
        assert_eq!(main_events("a").0.text.as_deref(), Some("a"));
        assert_eq!(main_events("7").0.text.as_deref(), Some("7"));
        assert_eq!(main_events("Space").0.text.as_deref(), Some(" "));
        assert_eq!(main_events("Shift+a").0.text.as_deref(), Some("A"));
        assert_eq!(main_events("Shift+a").0.key, "A");
        // The release never repeats the text.
        assert_eq!(main_events("Enter").1.text, None);
    }

    #[test]
    fn navigation_and_modifier_keys_insert_no_text() {
        for combo in [
            "Tab",
            "Escape",
            "Backspace",
            "Delete",
            "ArrowLeft",
            "F5",
            "Shift",
        ] {
            assert_eq!(main_events(combo).0.text, None, "{combo}");
        }
        // Alt chords type dead-key or symbol characters; none is intended here.
        assert_eq!(main_events("Alt+a").0.text, None);
    }

    #[test]
    fn extended_named_keys_carry_a_virtual_key_code() {
        for (name, vk) in [
            ("Delete", 46),
            ("Home", 36),
            ("End", 35),
            ("PageUp", 33),
            ("PageDown", 34),
            ("Insert", 45),
            ("Space", 32),
            ("F1", 112),
            ("F5", 116),
            ("F12", 123),
        ] {
            assert_eq!(
                main_events(name).0.windows_virtual_key_code,
                Some(vk),
                "{name}"
            );
        }
        // Outside F1..F12 a name is passed through rather than invented a code.
        assert_eq!(main_events("F13").0.windows_virtual_key_code, None);
    }

    #[test]
    fn editing_command_events_use_the_platform_accelerator() {
        let [(down_phase, down), (up_phase, up)] = editing_command_events("selectAll", "a");
        assert_eq!((down_phase, up_phase), (KeyPhase::Down, KeyPhase::Up));
        assert_eq!(down.commands, vec!["selectAll"]);
        assert!(up.commands.is_empty());
        assert_eq!(down.modifiers, accelerator_modifier_bitmask());
        assert_eq!(down.code, "KeyA");
    }
}
