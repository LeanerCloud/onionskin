//! What every measuring tool reads against: the scale, what it snaps to, and
//! whether a measurement is kept on the page. One set for the three tools,
//! as Acrobat's Measuring Tool has one: a scale chosen while measuring a
//! distance is the one an area is measured at.

use std::sync::{Arc, Mutex, MutexGuard};

use onionskin_core::measure::Scale;
use onionskin_plugin_api::ToolChoice;

use crate::snap::SnapOptions;

/// The scales offered by name. Any other is chosen by its words.
pub const SCALES: [&str; 8] = [
    "1 in = 1 in",
    "1 mm = 1 mm",
    "1 in = 1 ft",
    "1 in = 10 ft",
    "1 in = 100 ft",
    "1 cm = 1 m",
    "1 cm = 10 m",
    "1 cm = 1 km",
];

const SCALE: &str = "Scale";
const SNAP: &str = "Snap to";
const MARKUP: &str = "Measurement";
const SCALE_PREFIX: &str = "scale:";
const MARKUP_ID: &str = "markup";

/// The settings the tools share.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub scale: Scale,
    pub snap: SnapOptions,
    /// Measurement markup: each measurement kept as a comment on the page.
    pub markup: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            scale: Scale::default(),
            snap: SnapOptions::default(),
            markup: true,
        }
    }
}

impl Settings {
    /// Every setting, as choices: the scales, what to snap to, and markup.
    pub fn choices(&self) -> Vec<ToolChoice> {
        let current = self.scale.label();
        let mut scales: Vec<String> = SCALES.iter().map(|&scale| scale.to_owned()).collect();
        if !scales.contains(&current) {
            scales.insert(0, current);
        }
        let mut choices: Vec<ToolChoice> = scales
            .into_iter()
            .map(|scale| choice(format!("{SCALE_PREFIX}{scale}"), scale, SCALE))
            .collect();
        choices.extend(
            SnapOptions::NAMES
                .iter()
                .map(|(id, label)| choice(format!("snap:{id}"), (*label).to_owned(), SNAP)),
        );
        choices.push(choice(
            MARKUP_ID.to_owned(),
            "Keep as a comment".to_owned(),
            MARKUP,
        ));
        choices
    }

    /// Choose `id`: a scale by its words, or a snap or markup setting
    /// turned the other way. `false` for anything else.
    pub fn choose(&mut self, id: &str) -> bool {
        if let Some(words) = id.strip_prefix(SCALE_PREFIX) {
            return match Scale::parse(words) {
                Some(scale) => {
                    self.scale = scale;
                    true
                }
                None => false,
            };
        }
        if let Some(name) = id.strip_prefix("snap:") {
            return self.snap.toggle(name);
        }
        if id == MARKUP_ID {
            self.markup = !self.markup;
            return true;
        }
        false
    }

    pub fn picked(&self, id: &str) -> bool {
        if let Some(words) = id.strip_prefix(SCALE_PREFIX) {
            return Scale::parse(words) == Some(self.scale);
        }
        if let Some(name) = id.strip_prefix("snap:") {
            return self.snap.is_on(name);
        }
        id == MARKUP_ID && self.markup
    }

    /// The scale's choice id.
    pub fn chosen(&self) -> String {
        format!("{SCALE_PREFIX}{}", self.scale.label())
    }
}

fn choice(id: String, label: String, category: &str) -> ToolChoice {
    ToolChoice {
        id,
        label,
        category: category.to_owned(),
    }
}

/// The settings, held once for every tool.
#[derive(Clone, Debug, Default)]
pub struct Shared(Arc<Mutex<Settings>>);

impl Shared {
    pub fn lock(&self) -> MutexGuard<'_, Settings> {
        // A panic while holding the lock leaves plain values behind, never a
        // half-made one, so what is there is still good to use.
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn get(&self) -> Settings {
        *self.lock()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onionskin_core::measure::Unit;

    #[test]
    fn a_scale_is_chosen_by_name_or_by_its_words() {
        let mut settings = Settings::default();
        assert!(settings.picked("scale:1 in = 1 in"));
        assert_eq!(settings.chosen(), "scale:1 in = 1 in");
        assert!(settings.choose("scale:1 in = 10 ft"));
        assert_eq!(
            settings.scale,
            Scale::new(1.0, Unit::Inch, 10.0, Unit::Foot)
        );
        assert!(!settings.choose("scale:one inch"), "not a scale");
        assert!(settings.choose("scale:3 cm = 2 km"));
        let choices = settings.choices();
        assert_eq!(
            choices[0].id, "scale:3 cm = 2 km",
            "a scale of its own is listed first"
        );
        assert!(settings.picked(&choices[0].id));
        assert_eq!(
            choices
                .iter()
                .filter(|choice| choice.category == SCALE)
                .count(),
            9
        );
        assert!(!settings.choose("anything"));
    }

    #[test]
    fn snapping_and_markup_are_turned_on_and_off() {
        let mut settings = Settings::default();
        let choices = settings.choices();
        assert_eq!(
            choices
                .iter()
                .filter(|choice| choice.category == SNAP)
                .count(),
            4
        );
        assert_eq!(
            choices.last().map(|choice| choice.id.as_str()),
            Some("markup")
        );
        assert!(settings.picked("markup") && settings.picked("snap:endpoints"));
        assert!(settings.choose("markup") && !settings.picked("markup"));
        assert!(settings.choose("snap:endpoints") && !settings.picked("snap:endpoints"));
        assert!(!settings.choose("snap:corners"));
        let shared = Shared::default();
        shared.lock().markup = false;
        assert!(!shared.get().markup, "one set, however it is reached");
    }
}
