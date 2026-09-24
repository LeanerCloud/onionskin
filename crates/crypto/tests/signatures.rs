//! The CMS verifier over signatures pyHanko made (`corpus/make-signed.py`),
//! whose verdicts poppler's pdfsig agrees with.

use onionskin_crypto::signature::{verify_cms, Algorithm, Digest, SignatureCheck};

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/signed")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// Every signature's byte range and contents, read straight from the bytes:
/// this crate parses no PDF, and the test should not lean on one that does.
fn signatures(pdf: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let text = String::from_utf8_lossy(pdf);
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = text[from..].find("/ByteRange") {
        let start = from + at;
        let open = start + text[start..].find('[').expect("[");
        let close = open + text[open..].find(']').expect("]");
        let range: Vec<usize> = text[open + 1..close]
            .split_whitespace()
            .map(|number| number.parse().expect("a number"))
            .collect();
        let data = [
            &pdf[range[0]..range[0] + range[1]],
            &pdf[range[2]..range[2] + range[3]],
        ]
        .concat();
        let hex = &text[range[0] + range[1] + 1..range[2] - 1];
        let contents = (0..hex.len() / 2)
            .map(|index| u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).expect("hex"))
            .collect();
        found.push((contents, data));
        from = close;
    }
    found
}

fn first(name: &str) -> onionskin_crypto::signature::CmsCheck {
    let pdf = fixture(name);
    let (contents, data) = signatures(&pdf).into_iter().next().expect("a signature");
    verify_cms(&contents, &data).expect("checks")
}

#[test]
fn each_algorithm_verifies() {
    for (name, algorithm, digest) in [
        ("rsa-sha256.pdf", Algorithm::RsaPkcs1, Digest::Sha256),
        ("rsa-pss.pdf", Algorithm::RsaPss, Digest::Sha256),
        ("ecdsa-p256.pdf", Algorithm::EcdsaP256, Digest::Sha256),
        ("ecdsa-p384.pdf", Algorithm::EcdsaP384, Digest::Sha384),
        ("rsa-sha1.pdf", Algorithm::RsaPkcs1, Digest::Sha1),
        ("cades.pdf", Algorithm::RsaPkcs1, Digest::Sha256),
        ("certified-p1.pdf", Algorithm::RsaPkcs1, Digest::Sha256),
    ] {
        let check = first(name);
        assert!(check.is_valid(), "{name}: {check:?}");
        assert_eq!(check.algorithm, Some(algorithm), "{name}");
        assert_eq!(check.digest, Some(digest), "{name}");
        assert_eq!(check.is_weak(), digest == Digest::Sha1, "{name}");
        let signer = check.signer.expect("a signer");
        assert!(
            signer.display_name().contains("Signer") || signer.display_name().starts_with("Pat")
        );
        assert!(
            signer.issuer.contains("Onionskin Test CA"),
            "{}",
            signer.issuer
        );
        assert!(
            check.certificates.len() >= 2,
            "{name}: the signer's and the CA's"
        );
    }
}

#[test]
fn a_changed_byte_breaks_the_digest_and_not_the_signature() {
    let check = first("tampered.pdf");
    assert!(!check.digest_matches);
    assert_eq!(
        check.signature,
        SignatureCheck::Valid,
        "the attributes are untouched"
    );
    assert!(!check.is_valid());
}

#[test]
fn a_second_signature_and_a_later_note_leave_each_signature_valid() {
    let pdf = fixture("two-signatures.pdf");
    let found = signatures(&pdf);
    assert_eq!(found.len(), 2);
    for (contents, data) in found {
        assert!(verify_cms(&contents, &data).expect("checks").is_valid());
    }
    assert!(first("annotated-after.pdf").is_valid());
}

#[test]
fn a_signature_over_other_bytes_is_invalid() {
    let pdf = fixture("rsa-sha256.pdf");
    let (contents, mut data) = signatures(&pdf).remove(0);
    data.push(b'\n');
    assert!(!verify_cms(&contents, &data).expect("checks").digest_matches);

    let mut forged = contents.clone();
    // The signature value is near the end of the CMS: flip a byte in it.
    let end = onionskin_crypto::signature::verify_cms(&contents, &data)
        .map(|_| forged.iter().rposition(|byte| *byte != 0).expect("content"))
        .expect("checks");
    forged[end - 10] ^= 0xFF;
    let check = verify_cms(&forged, &data);
    assert!(check.map_or(true, |check| !check.is_valid()));
}
