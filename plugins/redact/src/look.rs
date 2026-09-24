//! A mark's look to and from the preference that keeps it between runs,
//! which holds whole numbers so the file round-trips exactly.

use onionskin_core::redactions::{Align, Overlay, RedactionLook};
use onionskin_plugin_api::{OverlayDefault, RedactionDefault};

/// The look a preference describes.
pub fn look_of(default: &RedactionDefault) -> RedactionLook {
    RedactionLook {
        fill: default.fill.map(unit),
        outline: unit(default.outline),
        overlay: default.overlay.as_ref().map(|overlay| Overlay {
            text: overlay.text.clone(),
            size: f64::from(overlay.size_tenths) / 10.0,
            color: unit(overlay.color),
            align: match overlay.align {
                1 => Align::Centre,
                2 => Align::Right,
                _ => Align::Left,
            },
            repeat: overlay.repeat,
        }),
    }
}

/// The preference that keeps `look`.
pub fn default_of(look: &RedactionLook) -> RedactionDefault {
    RedactionDefault {
        fill: look.fill.map(byte),
        outline: byte(look.outline),
        overlay: look.overlay.as_ref().map(|overlay| OverlayDefault {
            text: overlay.text.clone(),
            size_tenths: (overlay.size.max(0.0) * 10.0).round() as u32,
            color: byte(overlay.color),
            align: match overlay.align {
                Align::Left => 0,
                Align::Centre => 1,
                Align::Right => 2,
            },
            repeat: overlay.repeat,
        }),
    }
}

fn unit(color: [u8; 3]) -> [f64; 3] {
    color.map(|channel| f64::from(channel) / 255.0)
}

fn byte(color: [f64; 3]) -> [u8; 3] {
    color.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_look_survives_its_preference() {
        let look = RedactionLook {
            fill: None,
            outline: [0.0, 0.0, 1.0],
            overlay: Some(Overlay {
                text: "(b)(6)".to_owned(),
                size: 9.5,
                color: [1.0, 1.0, 1.0],
                align: Align::Right,
                repeat: true,
            }),
        };
        assert_eq!(look_of(&default_of(&look)), look);
        for align in Align::ALL {
            let look = RedactionLook {
                overlay: Some(Overlay {
                    align,
                    ..Overlay::default()
                }),
                ..RedactionLook::default()
            };
            assert_eq!(look_of(&default_of(&look)), look);
        }
        assert_eq!(
            look_of(&default_of(&RedactionLook::default())),
            RedactionLook::default()
        );
    }
}
