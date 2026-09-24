//! Whether a validated signature's signer is trusted: the certificates it
//! carries, judged against the user's trusted ones at the chosen time.
//!
//! Kept apart from [`super::validate`], which is cached per version of the
//! file: trust changes when the user adds a certificate, not when the file
//! does, so it is judged on each read.

use std::time::SystemTime;

use onionskin_crypto::signature::trust::{self, Identity, Purpose, TrustAnchor};

use super::Validation;

/// Which time a signer's certificates must be valid at, as Acrobat's
/// Signature Verification preferences name the choice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VerificationTime {
    /// Now. Acrobat's fallback when a signature has no timestamp, and the
    /// default here, where timestamps are not yet verified.
    #[default]
    Current,
    /// The time the signature says it was created. The signer's computer
    /// states it, and nothing proves it.
    Creation,
}

/// The signer's identity, for `validation`, with `anchors` trusted and the
/// certificates judged at the time `time` picks.
pub fn identity(
    validation: &Validation,
    anchors: &[TrustAnchor],
    time: VerificationTime,
    now: SystemTime,
) -> Identity {
    let check = match &validation.check {
        Ok(check) => check,
        Err(reason) => return Identity::Unknown(format!("the signature is unreadable: {reason}")),
    };
    let Some(signer) = &check.signer else {
        return Identity::Unknown("the signature names no signer's certificate".to_owned());
    };
    let now = trust::rfc3339(now);
    let at = match time {
        VerificationTime::Current => now,
        VerificationTime::Creation => check.signing_time.clone().unwrap_or(now),
    };
    let purpose = match validation.certification {
        Some(_) => Purpose::Certification,
        None => Purpose::Approval,
    };
    trust::identity(signer, &check.certificates, anchors, &at, purpose)
}

/// The identity, in the sentence the pane and the properties end with.
pub fn identity_sentence(identity: &Identity) -> String {
    match identity {
        Identity::Valid { .. } => "The signer's identity is valid.".to_owned(),
        Identity::Unknown(reason) => format!("The signer's identity is unknown: {reason}."),
        Identity::Invalid(reason) => format!("The signer's identity is invalid: {reason}."),
    }
}
