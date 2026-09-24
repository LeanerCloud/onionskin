//! Redaction Properties' defaults as the preferences file writes them:
//! `{"fill": "#000000", "outline": "#ff0000", "overlay": {"text": "(b)(6)",
//! "size": 0, "color": "#ffffff", "align": "centre", "repeat": false}}`.

use onionskin_plugin_api::{OverlayDefault, RedactionDefault};

use super::parse_hex;

pub(super) const REDACTION: &str = "{\"fill\": \"#rrggbb\" or null, \"outline\": \"#rrggbb\", \
     \"overlay\": null or {\"text\", \"size\" in tenths of a point, \"color\", \"align\": \
     \"left\", \"centre\" or \"right\", \"repeat\"}}";

const ALIGNS: [&str; 3] = ["left", "centre", "right"];

/// The setting, or `None` when any part of it is malformed.
pub(super) fn parse(value: &serde_json::Value) -> Option<RedactionDefault> {
    let fill = match value.get("fill")? {
        serde_json::Value::Null => None,
        color => Some(parse_hex(color.as_str()?)?),
    };
    let outline = parse_hex(value.get("outline")?.as_str()?)?;
    let overlay = match value.get("overlay") {
        None | Some(serde_json::Value::Null) => None,
        Some(overlay) => {
            let align = overlay.get("align")?.as_str()?;
            Some(OverlayDefault {
                text: overlay.get("text")?.as_str()?.to_owned(),
                size_tenths: u32::try_from(overlay.get("size")?.as_u64()?).ok()?,
                color: parse_hex(overlay.get("color")?.as_str()?)?,
                align: ALIGNS
                    .iter()
                    .position(|known| *known == align)
                    .and_then(|at| u8::try_from(at).ok())?,
                repeat: overlay.get("repeat")?.as_bool()?,
            })
        }
    };
    Some(RedactionDefault {
        fill,
        outline,
        overlay,
    })
}

pub(super) fn json(default: &RedactionDefault) -> serde_json::Value {
    let hex = |[r, g, b]: [u8; 3]| format!("#{r:02x}{g:02x}{b:02x}");
    let overlay = default
        .overlay
        .as_ref()
        .map_or(serde_json::Value::Null, |overlay| {
            serde_json::json!({
                "text": overlay.text,
                "size": overlay.size_tenths,
                "color": hex(overlay.color),
                "align": ALIGNS.get(usize::from(overlay.align)).copied().unwrap_or("left"),
                "repeat": overlay.repeat,
            })
        });
    serde_json::json!({
        "fill": default.fill.map_or(serde_json::Value::Null, |fill| hex(fill).into()),
        "outline": hex(default.outline),
        "overlay": overlay,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_default_round_trips_and_a_bad_one_is_refused() {
        let default = RedactionDefault {
            fill: None,
            outline: [255, 0, 0],
            overlay: Some(OverlayDefault {
                text: "(b)(6)".to_owned(),
                size_tenths: 95,
                color: [255, 255, 255],
                align: 2,
                repeat: true,
            }),
        };
        assert_eq!(parse(&json(&default)), Some(default.clone()));
        let plain = RedactionDefault {
            fill: Some([0, 0, 0]),
            overlay: None,
            ..default
        };
        assert_eq!(parse(&json(&plain)), Some(plain));
        for bad in [
            r##"{"fill": null}"##,
            r##"{"fill": "red", "outline": "#ff0000"}"##,
            r##"{"fill": null, "outline": "#ff0000", "overlay": {"text": "x", "size": 0, "color": "#ffffff", "align": "middle", "repeat": false}}"##,
        ] {
            let value: serde_json::Value = serde_json::from_str(bad).expect("json");
            assert_eq!(parse(&value), None, "{bad}");
        }
    }
}
