//! Guarantee test 5, Schist's honesty test: with every `plugins/` entry
//! compiled out the app still builds and boots, to a workspace that can
//! do nothing.
//!
//! The assertions hold for any feature combination, not just all-on and
//! all-off, because the expected count is summed from the same cfgs
//! Cargo.toml gates the dependencies on. A plugin whose feature is on but
//! whose manifest never reached `first_party_manifests` fails here.

use onionskin_app::{boot_summary, build_registry};

/// How many first-party plugins this build compiled in.
const COMPILED_IN_PLUGINS: usize = cfg!(feature = "codecs-common") as usize
    + cfg!(feature = "commands-core") as usize
    + cfg!(feature = "redact") as usize
    + cfg!(feature = "tools-accessibility") as usize
    + cfg!(feature = "tools-basic") as usize
    + cfg!(feature = "tools-comment") as usize
    + cfg!(feature = "tools-edit") as usize
    + cfg!(feature = "tools-fill-sign") as usize
    + cfg!(feature = "tools-form") as usize
    + cfg!(feature = "tools-measure") as usize
    + cfg!(feature = "tools-organize") as usize
    + cfg!(feature = "tools-protect") as usize;

#[test]
fn the_registry_holds_exactly_the_plugins_compiled_in() {
    let registry = build_registry();
    assert_eq!(
        registry.plugins().len(),
        COMPILED_IN_PLUGINS,
        "the registry disagrees with the enabled features; registered {:?}",
        registry.plugins()
    );
}

#[cfg(not(any(
    feature = "codecs-common",
    feature = "commands-core",
    feature = "redact",
    feature = "tools-accessibility",
    feature = "tools-basic",
    feature = "tools-comment",
    feature = "tools-edit",
    feature = "tools-fill-sign",
    feature = "tools-form",
    feature = "tools-measure",
    feature = "tools-organize",
    feature = "tools-protect",
)))]
#[test]
fn a_kernel_with_no_plugins_registers_nothing_at_all() {
    let registry = build_registry();
    assert_eq!(registry.plugins().len(), 0);
    assert_eq!(registry.tools().count(), 0);
    assert_eq!(registry.commands().len(), 0);
    assert_eq!(registry.codecs().count(), 0);
    assert_eq!(
        boot_summary(&registry),
        "onionskin: 0 plugins, 0 tools, 0 commands, 0 codecs"
    );
}

/// The export formats are as much a part of what a build can do as its tools,
/// so the honesty test counts them too: with `codecs-common` compiled out
/// there is nothing to export to, and with it in there are exactly three.
#[test]
fn the_registry_holds_a_codec_only_when_the_plugin_that_owns_it_is_compiled_in() {
    let registry = build_registry();

    let expected = if cfg!(feature = "codecs-common") {
        3
    } else {
        0
    };
    assert_eq!(
        registry.codecs().count(),
        expected,
        "registered {:?}",
        registry.codecs().map(|c| c.id()).collect::<Vec<_>>()
    );
}

#[test]
fn headless_boot_reports_the_registry_and_exits_zero() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_onionskin"))
        .arg("--headless-boot")
        .output()
        .expect("run the onionskin binary");
    assert!(
        output.status.success(),
        "--headless-boot exited with {}",
        output.status
    );
    let stdout = String::from_utf8(output.stdout).expect("the summary is UTF-8");
    assert_eq!(stdout.trim_end(), boot_summary(&build_registry()));
}
