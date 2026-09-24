//! The form the plugin's tests fill.

fn js(script: &str) -> String {
    format!("<< /S /JavaScript /JS ({script}) >>")
}

/// qty times price is total, shown as dollars; age between 0 and 130; a
/// check box, a radio pair, and a field whose calculation cannot run.
pub fn document() -> Vec<u8> {
    let number = format!(
        "/AA << /K {} /F {} >>",
        js("AFNumber_Keystroke\\(2, 0, 0, 0, \"\", true\\);"),
        js("AFNumber_Format\\(2, 0, 0, 0, \"\", true\\);")
    );
    let total = format!(
        "/AA << /C {} /F {} >>",
        js("AFSimple_Calculate\\(\"PRD\", new Array \\(\"qty\", \"price\"\\)\\);"),
        js("AFNumber_Format\\(2, 0, 0, 0, \"$\", true\\);")
    );
    let age = format!(
        "/AA << /V {} >>",
        js("AFRange_Validate\\(true, 0, true, 130\\);")
    );
    let broken = format!("/AA << /C {} >>", js("event.value = this.mailForm\\(\\);"));
    let widget = |rest: &str| -> Vec<u8> {
        format!("<< /Type /Annot /Subtype /Widget /P 3 0 R {rest} >>").into_bytes()
    };
    let objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R 6 0 R 7 0 R 8 0 R 9 0 R 12 0 R] \
           /DA (/Helv 0 Tf 0 g) /CO [6 0 R 12 0 R] >> >>"
            .to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 612 792] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Annots [4 0 R 5 0 R 6 0 R 7 0 R 8 0 R 10 0 R 11 0 R 12 0 R] >>".to_vec(),
        widget("/FT /Tx /T (qty) /Rect [10 700 110 720] /V (1)"),
        widget(&format!("/FT /Tx /T (price) /Rect [10 670 110 690] {number}")),
        widget(&format!("/FT /Tx /T (total) /Rect [10 640 110 660] {total}")),
        widget(&format!("/FT /Tx /T (age) /Rect [10 610 110 630] {age}")),
        widget("/FT /Btn /T (agree) /Rect [10 580 22 592] /AP << /N << /Yes 13 0 R /Off 13 0 R >> >> /AS /Off"),
        b"<< /FT /Btn /Ff 49152 /T (size) /Kids [10 0 R 11 0 R] >>".to_vec(),
        widget("/Parent 9 0 R /Rect [10 550 22 562] /AP << /N << /S 13 0 R /Off 13 0 R >> >> /AS /Off"),
        widget("/Parent 9 0 R /Rect [40 550 52 562] /AP << /N << /L 13 0 R /Off 13 0 R >> >> /AS /Off"),
        widget(&format!("/FT /Tx /T (broken) /Rect [10 520 110 540] /V (old) {broken}")),
        b"<< /Type /XObject /Subtype /Form /BBox [0 0 12 12] /Length 0 >>\nstream\n\nendstream".to_vec(),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}
