//! Navigation bindings only. Text fields and modal dialogs deliberately do not remap typing.
use anyhow::{Result, bail};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::{BTreeMap, HashMap};

struct Spec {
    name: &'static str,
    label: &'static str,
    defaults: &'static [&'static str],
    canonical: KeyEvent,
}
fn specs() -> Vec<Spec> {
    use KeyCode::*;
    let plain = KeyModifiers::NONE;
    let ctrl = KeyModifiers::CONTROL;
    [
        ("saved", "Saved section", &["1"][..], Char('1'), plain),
        ("history", "History section", &["2"][..], Char('2'), plain),
        ("search", "Search section", &["3"][..], Char('3'), plain),
        (
            "focus",
            "Switch pane focus",
            &["tab", "shift+tab"][..],
            Tab,
            plain,
        ),
        ("up", "Move up", &["k", "up"][..], Up, plain),
        ("down", "Move down", &["j", "down"][..], Down, plain),
        ("top", "First row (also gg)", &["home"][..], Home, plain),
        ("bottom", "Last row", &["G", "end"][..], End, plain),
        ("page_up", "Page up", &["pageup"][..], PageUp, plain),
        ("page_down", "Page down", &["pagedown"][..], PageDown, plain),
        (
            "half_page_up",
            "Half-page up",
            &["ctrl+u"][..],
            Char('u'),
            ctrl,
        ),
        (
            "half_page_down",
            "Half-page down",
            &["ctrl+d"][..],
            Char('d'),
            ctrl,
        ),
        (
            "play",
            "Play selected station",
            &["enter"][..],
            Enter,
            plain,
        ),
        ("favorite", "Toggle favorite", &["f"][..], Char('f'), plain),
        ("add", "Add stream / .pls", &["a"][..], Char('a'), plain),
        (
            "remove",
            "Remove favorite (Saved)",
            &["d"][..],
            Char('d'),
            plain,
        ),
        (
            "pause",
            "Play / pause",
            &["p", "space"][..],
            Char('p'),
            plain,
        ),
        ("stop", "Stop stream", &["s"][..], Char('s'), plain),
        (
            "volume_up",
            "Volume up",
            &["v", "+", "="][..],
            Char('v'),
            plain,
        ),
        (
            "volume_down",
            "Volume down",
            &["V", "-"][..],
            Char('V'),
            plain,
        ),
        ("mute", "Toggle mute", &["m"][..], Char('m'), plain),
        (
            "filter",
            "Edit search / filter",
            &["/"][..],
            Char('/'),
            plain,
        ),
        ("sort", "Cycle search sort", &["o"][..], Char('o'), plain),
        (
            "reverse",
            "Reverse search sort",
            &["O"][..],
            Char('O'),
            plain,
        ),
        ("refresh", "Refresh search", &["r"][..], Char('r'), plain),
        ("details", "Station details", &["i"][..], Char('i'), plain),
        ("help", "Help", &["?"][..], Char('?'), plain),
        ("quit", "Quit (also Ctrl-c)", &["q"][..], Char('q'), plain),
    ]
    .into_iter()
    .map(|(name, label, defaults, code, modifiers)| Spec {
        name,
        label,
        defaults,
        canonical: KeyEvent::new(code, modifiers),
    })
    .collect()
}

pub struct Keymap {
    map: HashMap<(KeyCode, KeyModifiers), KeyEvent>,
    bindings: BTreeMap<String, Vec<String>>,
}
impl Keymap {
    pub fn new(overrides: &BTreeMap<String, Vec<String>>) -> Result<Self> {
        let specs = specs();
        for name in overrides.keys() {
            if !specs.iter().any(|spec| spec.name == name) {
                bail!("Unknown keybinding action: {name}");
            }
        }
        let mut map = HashMap::new();
        let mut bindings = BTreeMap::new();
        for spec in specs {
            let keys = overrides
                .get(spec.name)
                .cloned()
                .unwrap_or_else(|| spec.defaults.iter().map(|s| (*s).into()).collect());
            for key in &keys {
                let parsed = parse(key)?;
                if parsed == (KeyCode::Char('g'), KeyModifiers::NONE)
                    || parsed == (KeyCode::Char('c'), KeyModifiers::CONTROL)
                {
                    bail!("{key} is reserved for gg / Ctrl-c");
                }
                if map.insert(parsed, spec.canonical).is_some() {
                    bail!(
                        "Conflicting navigation keybinding: {key}. Override the old action as well to free this key."
                    );
                }
            }
            bindings.insert(spec.name.into(), keys);
        }
        Ok(Self { map, bindings })
    }
    pub fn translate(&self, key: KeyEvent) -> Option<KeyEvent> {
        self.map.get(&normalize(key.code, key.modifiers)).copied()
    }
    pub fn hint(&self, action: &str) -> &str {
        self.bindings
            .get(action)
            .and_then(|v| v.first())
            .map(String::as_str)
            .unwrap_or("—")
    }
    pub fn help(&self) -> Vec<String> {
        specs()
            .into_iter()
            .map(|spec| {
                format!(
                    "{:<22} {}",
                    self.bindings[spec.name].join(" / "),
                    spec.label
                )
            })
            .collect()
    }
}
fn normalize(code: KeyCode, mut modifiers: KeyModifiers) -> (KeyCode, KeyModifiers) {
    // Terminals report uppercase characters both with and without the Shift flag.
    if matches!(code, KeyCode::Char(_)) {
        modifiers.remove(KeyModifiers::SHIFT);
    }
    if code == KeyCode::BackTab {
        modifiers.insert(KeyModifiers::SHIFT);
    }
    (code, modifiers)
}
fn parse(value: &str) -> Result<(KeyCode, KeyModifiers)> {
    let (name, modifiers) = if let Some(rest) = value.strip_prefix("ctrl+") {
        (rest, KeyModifiers::CONTROL)
    } else {
        (value, KeyModifiers::NONE)
    };
    let code = match name {
        "enter" => KeyCode::Enter,
        "space" => KeyCode::Char(' '),
        "tab" => KeyCode::Tab,
        "shift+tab" => return Ok((KeyCode::BackTab, KeyModifiers::SHIFT)),
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "delete" => KeyCode::Delete,
        "backspace" => KeyCode::Backspace,
        _ if name.chars().count() == 1 => KeyCode::Char(name.chars().next().unwrap()),
        _ => bail!(
            "Invalid binding {value:?}; use a character, ctrl+x, or a named key such as enter, space, up, shift+tab"
        ),
    };
    Ok(normalize(code, modifiers))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_validate_and_uppercase_handles_shift() {
        let map = Keymap::new(&BTreeMap::new()).unwrap();
        assert_eq!(
            map.translate(KeyEvent::new(KeyCode::Char('V'), KeyModifiers::SHIFT))
                .unwrap()
                .code,
            KeyCode::Char('V')
        );
    }
    #[test]
    fn remapping_replaces_old_binding_and_rejects_conflicts() {
        let map = Keymap::new(&BTreeMap::from([("favorite".into(), vec!["x".into()])])).unwrap();
        assert_eq!(
            map.translate(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
                .unwrap()
                .code,
            KeyCode::Char('f')
        );
        assert!(
            map.translate(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE))
                .is_none()
        );
        assert!(Keymap::new(&BTreeMap::from([("favorite".into(), vec!["q".into()])])).is_err());
        assert!(Keymap::new(&BTreeMap::from([("typo".into(), vec!["x".into()])])).is_err());
    }
}
