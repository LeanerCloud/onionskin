//! The trust path builder against OpenSSL, over `corpus/chains/`.

use std::path::{Path, PathBuf};
use std::process::Command;

use onionskin_crypto::signature::trust::{identity, Identity, Purpose, TrustAnchor};
use onionskin_crypto::signature::Certificate;

/// Every chain is valid 2024 to 2039 but the expired and future leaves.
const AT: &str = "2030-01-01T00:00:00Z";

fn chains() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/chains")
}

fn certificate(name: &str) -> Certificate {
    let bytes = std::fs::read(chains().join(name)).expect("the chain file reads");
    let mut found = Certificate::from_file_bytes(&bytes);
    assert_eq!(found.len(), 1, "{name} holds one certificate");
    found.remove(0)
}

fn trusted(name: &str) -> TrustAnchor {
    TrustAnchor {
        certificate: certificate(name),
        for_signatures: true,
        for_certified: true,
    }
}

/// What the builder decides for `leaf`, with the root trusted and every
/// other CA carried by the signature.
fn decide(leaf: &str) -> Identity {
    let carried = [certificate("intermediate.pem"), certificate("not-ca.pem")];
    identity(
        &certificate(leaf),
        &carried,
        &[trusted("root.pem")],
        AT,
        Purpose::Approval,
    )
}

/// Whether `openssl verify` accepts `leaf` for signing, when OpenSSL is
/// installed.
fn openssl_accepts(leaf: &str) -> Option<bool> {
    let chains = chains();
    let output = Command::new("openssl")
        .args(["verify", "-attime", "1893456000", "-purpose", "smimesign"])
        .arg("-CAfile")
        .arg(chains.join("root.pem"))
        .arg("-untrusted")
        .arg(chains.join("intermediate.pem"))
        .arg("-untrusted")
        .arg(chains.join("not-ca.pem"))
        .arg(chains.join(leaf))
        .output()
        .ok()?;
    Some(output.status.success())
}

#[test]
fn each_chain_is_judged_as_openssl_judges_it() {
    for (leaf, expected) in [
        ("leaf.pem", "valid"),
        ("leaf-expired.pem", "expired"),
        ("leaf-future.pem", "not valid until"),
        ("leaf-under-not-ca.pem", "not a certificate authority"),
        ("leaf-cert-sign.pem", "not for signing documents"),
        ("stranger-leaf.pem", "unknown"),
    ] {
        let found = decide(leaf);
        let ours = matches!(found, Identity::Valid { .. });
        match &found {
            Identity::Valid { path } => {
                assert_eq!(expected, "valid", "{leaf}");
                let names: Vec<_> = path.iter().map(|c| c.display_name().to_owned()).collect();
                assert_eq!(names, ["Chain Signer", "Chain Intermediate", "Chain Root"]);
            }
            Identity::Invalid(reason) => assert!(reason.contains(expected), "{leaf}: {reason}"),
            Identity::Unknown(_) => assert_eq!(expected, "unknown", "{leaf}"),
        }
        if let Some(openssl) = openssl_accepts(leaf) {
            assert_eq!(ours, openssl, "OpenSSL disagrees on {leaf}");
        }
    }
}

#[test]
fn trust_is_by_purpose_and_a_trusted_intermediate_is_enough() {
    let leaf = certificate("leaf.pem");
    let carried = [certificate("intermediate.pem")];
    let approvals_only = TrustAnchor {
        for_certified: false,
        ..trusted("root.pem")
    };
    let certified = identity(
        &leaf,
        &carried,
        std::slice::from_ref(&approvals_only),
        AT,
        Purpose::Certification,
    );
    assert!(matches!(certified, Identity::Unknown(reason) if reason.contains("certified")));
    assert!(matches!(
        identity(&leaf, &carried, &[approvals_only], AT, Purpose::Approval),
        Identity::Valid { .. }
    ));

    let not_for_anything = TrustAnchor {
        for_signatures: false,
        for_certified: false,
        ..trusted("root.pem")
    };
    assert!(matches!(
        identity(&leaf, &carried, &[not_for_anything], AT, Purpose::Approval),
        Identity::Unknown(_)
    ));

    // The intermediate trusted and the root not carried: the path stops at
    // the intermediate.
    let found = identity(
        &leaf,
        &[],
        &[trusted("intermediate.pem")],
        AT,
        Purpose::Approval,
    );
    let Identity::Valid { path } = found else {
        panic!("a trusted intermediate is a trust anchor: {found:?}")
    };
    assert_eq!(path.len(), 2);

    // Missing intermediate: nothing to chain through.
    assert!(matches!(
        identity(&leaf, &[], &[trusted("root.pem")], AT, Purpose::Approval),
        Identity::Unknown(_)
    ));
}

#[test]
fn a_certificate_file_may_be_pem_several_or_der() {
    let root = std::fs::read(chains().join("root.pem")).unwrap();
    let intermediate = std::fs::read(chains().join("intermediate.pem")).unwrap();
    let both = [root.clone(), intermediate].concat();
    assert_eq!(Certificate::from_file_bytes(&both).len(), 2);
    let der = certificate("root.pem").der;
    assert_eq!(
        Certificate::from_file_bytes(&der),
        [certificate("root.pem")]
    );
    assert!(Certificate::from_file_bytes(b"not a certificate").is_empty());
    let pem = certificate("root.pem").to_pem();
    assert!(pem.starts_with("-----BEGIN CERTIFICATE-----"));
    assert_eq!(
        Certificate::from_file_bytes(pem.as_bytes()),
        [certificate("root.pem")]
    );
}
