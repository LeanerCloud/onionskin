//! The digests a PDF signature is made over.

use der::asn1::ObjectIdentifier;
use sha1::Sha1;
use sha2::{Digest as _, Sha256, Sha384, Sha512};

/// A message digest algorithm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Digest {
    Sha1,
    Sha256,
    Sha384,
    Sha512,
}

const SHA1: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.14.3.2.26");
const SHA256: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.16.840.1.101.3.4.2.1");
const SHA384: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.16.840.1.101.3.4.2.2");
const SHA512: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.16.840.1.101.3.4.2.3");

impl Digest {
    pub(crate) fn from_oid(oid: ObjectIdentifier) -> Option<Digest> {
        match oid {
            SHA1 => Some(Digest::Sha1),
            SHA256 => Some(Digest::Sha256),
            SHA384 => Some(Digest::Sha384),
            SHA512 => Some(Digest::Sha512),
            _ => None,
        }
    }

    /// What a signature properties dialog calls it.
    pub fn name(self) -> &'static str {
        match self {
            Digest::Sha1 => "SHA-1",
            Digest::Sha256 => "SHA-256",
            Digest::Sha384 => "SHA-384",
            Digest::Sha512 => "SHA-512",
        }
    }

    /// Whether collisions are practical: SHA-1's are.
    pub fn is_weak(self) -> bool {
        self == Digest::Sha1
    }

    pub fn hash(self, data: &[u8]) -> Vec<u8> {
        match self {
            Digest::Sha1 => Sha1::digest(data).to_vec(),
            Digest::Sha256 => Sha256::digest(data).to_vec(),
            Digest::Sha384 => Sha384::digest(data).to_vec(),
            Digest::Sha512 => Sha512::digest(data).to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_digest_is_known_by_its_oid_and_hashes_to_its_length() {
        for (oid, digest, length) in [
            (SHA1, Digest::Sha1, 20),
            (SHA256, Digest::Sha256, 32),
            (SHA384, Digest::Sha384, 48),
            (SHA512, Digest::Sha512, 64),
        ] {
            assert_eq!(Digest::from_oid(oid), Some(digest));
            assert_eq!(digest.hash(b"abc").len(), length);
        }
        assert_eq!(
            Digest::from_oid(ObjectIdentifier::new_unwrap("1.2.3")),
            None
        );
        assert!(Digest::Sha1.is_weak() && !Digest::Sha256.is_weak());
        assert_eq!(Digest::Sha384.name(), "SHA-384");
        assert_eq!(
            Digest::Sha256.hash(b"abc")[..4],
            [0xba, 0x78, 0x16, 0xbf],
            "the published SHA-256 of abc"
        );
    }
}
