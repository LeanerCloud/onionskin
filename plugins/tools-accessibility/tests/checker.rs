//! The checker on documents written to break one rule each: what it finds,
//! and what it finds new after a change.

use onionskin_cos::{BytesSource, Document as CosDocument};
use onionskin_tools_accessibility::checker::{check, Finding, Report};

fn pdf(objects: &[&str]) -> CosDocument {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", index + 1).as_bytes());
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
    CosDocument::open(Box::new(BytesSource::new(out))).expect("opens")
}

fn stream(content: &str) -> String {
    format!(
        "<< /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

#[test]
fn an_untagged_document_passes_with_nothing_to_say() {
    let doc = pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>",
        "<< /Type /Page /Parent 2 0 R >>",
    ]);
    assert!(check(&doc).expect("checks").is_clean());
}

#[test]
fn each_way_the_tree_and_the_content_disagree_is_found() {
    let page_one = stream("/P << /MCID 0 >> BDC EMC /P << /MCID 7 >> BDC EMC");
    let page_two = stream("/P << /MCID 0 >> BDC EMC");
    let doc = pdf(&[
        "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 5 0 R /MarkInfo << /Marked true >> >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 100 100] >>",
        "<< /Type /Page /Parent 2 0 R /StructParents 0 /Contents 9 0 R >>",
        "<< /Type /Page /Parent 2 0 R /Contents 10 0 R >>",
        "<< /Type /StructTreeRoot /K [6 0 R 7 0 R 8 0 R] /ParentTree << /Nums [0 [6 0 R]] >> >>",
        "<< /Type /StructElem /S /P /P 5 0 R /Pg 3 0 R /K [0 << /Type /MCR /MCID 5 >>] >>",
        "<< /Type /StructElem /S /P /P 5 0 R /Pg 4 0 R /K 0 >>",
        "<< /Type /StructElem /S /P /P 5 0 R /Pg 99 0 R /K 0 >>",
        &page_one,
        &page_two,
    ]);
    let report = check(&doc).expect("checks");
    let found: Vec<_> = report.findings.iter().cloned().collect();
    assert!(
        found.contains(&Finding::ContentMissing { page: 0, mcid: 5 }),
        "{found:?}"
    );
    assert!(
        found.contains(&Finding::ContentUnnamed { page: 0, mcid: 7 }),
        "{found:?}"
    );
    assert!(
        found.contains(&Finding::PageUnlinked { page: 1 }),
        "{found:?}"
    );
    assert!(
        found
            .iter()
            .any(|finding| matches!(finding, Finding::Structure(violation) if violation.contains("ElementPageMissing"))),
        "{found:?}"
    );
    assert!(!report.is_clean());

    let before = Report {
        findings: report.findings.iter().take(2).cloned().collect(),
    };
    let new = report.new_since(&before);
    assert_eq!(new.len(), report.findings.len() - 2);
    assert!(
        report.new_since(&report).is_empty(),
        "nothing new since itself"
    );
}

#[test]
fn the_plugin_names_itself_and_registers_no_tools_yet() {
    use onionskin_plugin_api::{PluginManifest, PluginRegistry};
    use onionskin_tools_accessibility::AccessibilityToolsPlugin;
    assert_eq!(
        AccessibilityToolsPlugin.id(),
        "onionskin.tools-accessibility"
    );
    assert_eq!(AccessibilityToolsPlugin.name(), "Accessibility");
    let mut registry = PluginRegistry::default();
    AccessibilityToolsPlugin.register(&mut registry);
    assert_eq!(registry.tools().count(), 0);
}
