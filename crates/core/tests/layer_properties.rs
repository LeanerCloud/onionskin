//! Layer Properties written back: a layer's name, intent and default state,
//! read back by us, by pikepdf, and checked by qpdf.

mod common;

use std::process::Command;

use onionskin_core::{DocumentFile, LayerIntent, LayerProperties};

/// One page, two layers: Stamp on by default, and Notes (its default
/// configuration written as its own object, with a referenced `/OFF`).
fn document(config_inline: bool) -> Vec<u8> {
    let catalog = if config_inline {
        "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [4 0 R 5 0 R] /D << /OFF [5 0 R] >> >> >>"
    } else {
        "<< /Type /Catalog /Pages 2 0 R /OCProperties 6 0 R >>"
    };
    common::pdf(&[
        catalog.as_bytes().to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] >>".to_vec(),
        b"<< /Type /OCG /Name (Stamp) >>".to_vec(),
        b"<< /Type /OCG /Name (Notes) /Intent [/View /Design] >>".to_vec(),
        b"<< /OCGs [4 0 R 5 0 R] /D 7 0 R >>".to_vec(),
        b"<< /OFF 8 0 R >>".to_vec(),
        b"[5 0 R]".to_vec(),
    ])
}

fn saved_after(config_inline: bool, name: &str) -> (std::path::PathBuf, DocumentFile) {
    let dir = std::env::temp_dir().join(format!(
        "onionskin-layer-properties-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("layers.pdf");
    std::fs::write(&path, document(config_inline)).unwrap();
    let mut file = DocumentFile::open(&path).expect("opens");
    let layers = file.layers().expect("reads").to_vec();
    assert_eq!(
        layers[1].intent,
        LayerIntent::View,
        "View and Design is View"
    );
    assert!(!layers[1].visible);

    file.set_layer_properties(
        layers[0].id,
        &LayerProperties {
            name: "Approved stamp".to_owned(),
            intent: LayerIntent::Design,
            default_on: false,
        },
    )
    .expect("the first layer's properties are written");
    file.set_layer_properties(
        layers[1].id,
        &LayerProperties {
            name: "Notizen".to_owned(),
            intent: LayerIntent::View,
            default_on: true,
        },
    )
    .expect("the second layer's properties are written");
    let now = file.layers().expect("reads").to_vec();
    assert_eq!(now[0].name, "Approved stamp");
    assert_eq!(now[0].intent, LayerIntent::Design);
    assert!(!now[0].visible && now[1].visible, "the defaults changed");
    file.save().expect("saves");
    (path, file)
}

#[test]
fn a_layer_is_renamed_and_its_default_state_and_intent_are_written() {
    for inline in [true, false] {
        let (path, _) = saved_after(inline, if inline { "inline" } else { "referenced" });
        let reopened = DocumentFile::open(&path).expect("reopens");
        let mut reopened = reopened;
        let layers = reopened.layers().expect("reads").to_vec();
        let names: Vec<_> = layers.iter().map(|layer| layer.name.as_str()).collect();
        assert_eq!(names, ["Approved stamp", "Notizen"]);
        assert!(!layers[0].visible && layers[1].visible);
        assert_eq!(layers[0].intent, LayerIntent::Design);

        if let Ok(output) = Command::new("qpdf").arg("--check").arg(&path).output() {
            assert!(output.status.success(), "qpdf --check: {output:?}");
        }
        let script = "import pikepdf,sys\n\
            pdf = pikepdf.open(sys.argv[1])\n\
            p = pdf.Root.OCProperties\n\
            print(str(p.OCGs[0].Name), str(p.OCGs[0].Intent))\n\
            print([str(g.Name) for g in p.D.ON], [str(g.Name) for g in p.D.OFF])";
        if let Ok(output) = Command::new("python3")
            .args(["-c", script])
            .arg(&path)
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                assert_eq!(
                    text, "Approved stamp /Design\n['Notizen'] ['Approved stamp']\n",
                    "pikepdf reads it back"
                );
            }
        }
    }
}
