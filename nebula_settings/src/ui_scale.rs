//! Application-chrome scale, independent of terminal font size and monitor DPI.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiScale(u16);

impl Default for UiScale {
    fn default() -> Self {
        Self(100)
    }
}

impl UiScale {
    pub const VALUES: &'static [&'static str] = &["75", "100", "125", "150", "175", "200"];

    pub fn from_settings(value: &str) -> Option<Self> {
        Self::VALUES.contains(&value).then(|| Self(value.parse().expect("static scale value")))
    }

    pub fn settings_value(self) -> &'static str {
        Self::VALUES.iter().copied().find(|value| value.parse::<u16>() == Ok(self.0)).unwrap_or("100")
    }

    pub fn factor(self) -> f32 {
        f32::from(self.0) / 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RawSettings, RuntimeSettings, apply_updates};

    #[test]
    fn scale_defaults_to_one_and_rejects_invalid_or_unbounded_values() {
        for input in ["", "0", "-100", "NaN", "inf", "201", "10000", "1.25"] {
            let raw = RawSettings::from_text(&format!("ui_scale={input}\n"));
            assert_eq!(RuntimeSettings::from_raw(&raw).ui_scale, UiScale::default());
        }
        assert_eq!(UiScale::default().factor(), 1.0);
    }

    #[test]
    fn every_scale_round_trips_without_changing_terminal_font_size() {
        for value in UiScale::VALUES {
            let text = apply_updates("font_size=17\n", &[("ui_scale", (*value).into())]);
            let settings = RuntimeSettings::from_raw(&RawSettings::from_text(&text));
            assert_eq!(settings.ui_scale.settings_value(), *value);
            assert_eq!(settings.font_size_px, Some(17.0));
        }
        assert_eq!(UiScale::from_settings("150").unwrap().factor(), 1.5);
    }
}
