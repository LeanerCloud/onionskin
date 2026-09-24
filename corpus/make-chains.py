#!/usr/bin/env python3
"""Generate the certificate chains in corpus/chains/.

Each chain is built with the `cryptography` package, and what OpenSSL's
`openssl verify` decides about it is the reference a test compares the
trust path builder with. The names say what each certificate is:

  root.pem              a self-signed CA: the trust anchor
  intermediate.pem      a CA issued by root
  leaf.pem              a signer issued by intermediate: a good chain
  leaf-expired.pem      a signer whose validity ended in 2021
  leaf-future.pem       a signer valid only from 2040
  not-ca.pem            issued by root with CA:FALSE
  leaf-under-not-ca.pem a signer issued by not-ca: a broken chain
  leaf-cert-sign.pem    a signer whose key usage is keyCertSign only
  stranger-root.pem     a CA nothing else chains to
  stranger-leaf.pem     a signer issued by stranger-root

Validity is fixed, not relative to now, so the verdicts do not drift:
everything but the expired and future leaves is valid 2024 to 2039, and
tests verify at 2030-01-01.
"""

from __future__ import annotations

import datetime
import sys
from pathlib import Path

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.x509.oid import NameOID

OUT = Path(__file__).resolve().parent / "chains"
UTC = datetime.timezone.utc
FROM = datetime.datetime(2024, 1, 1, tzinfo=UTC)
UNTIL = datetime.datetime(2039, 12, 31, tzinfo=UTC)


def name(common: str) -> x509.Name:
    return x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, common)])


def usage(signer: bool, cert_sign: bool) -> x509.KeyUsage:
    return x509.KeyUsage(
        digital_signature=signer,
        content_commitment=signer,
        key_encipherment=False,
        data_encipherment=False,
        key_agreement=False,
        key_cert_sign=cert_sign,
        crl_sign=cert_sign,
        encipher_only=False,
        decipher_only=False,
    )


def issue(common, key, issuer_name, issuer_key, ca, key_usage, start=FROM, end=UNTIL):
    public = key.public_key()
    builder = (
        x509.CertificateBuilder()
        .subject_name(name(common))
        .issuer_name(issuer_name)
        .public_key(public)
        .serial_number(x509.random_serial_number())
        .not_valid_before(start)
        .not_valid_after(end)
        .add_extension(x509.BasicConstraints(ca=ca, path_length=None), critical=True)
        .add_extension(key_usage, critical=True)
        .add_extension(x509.SubjectKeyIdentifier.from_public_key(public), critical=False)
    )
    if issuer_key is not key:
        builder = builder.add_extension(
            x509.AuthorityKeyIdentifier.from_issuer_public_key(issuer_key.public_key()),
            critical=False,
        )
    return builder.sign(issuer_key, hashes.SHA256())


def key():
    return ec.generate_private_key(ec.SECP256R1())


def main() -> int:
    OUT.mkdir(exist_ok=True)
    root_key, intermediate_key, not_ca_key, stranger_key = key(), key(), key(), key()
    ca_usage = usage(signer=False, cert_sign=True)
    signer_usage = usage(signer=True, cert_sign=False)

    root = issue("Chain Root", root_key, name("Chain Root"), root_key, True, ca_usage)
    intermediate = issue(
        "Chain Intermediate", intermediate_key, root.subject, root_key, True, ca_usage
    )
    not_ca = issue("Chain Not A CA", not_ca_key, root.subject, root_key, False, ca_usage)
    stranger = issue(
        "Stranger Root", stranger_key, name("Stranger Root"), stranger_key, True, ca_usage
    )

    def leaf(common, issuer, issuer_key, key_usage=signer_usage, **when):
        return issue(common, key(), issuer.subject, issuer_key, False, key_usage, **when)

    certificates = {
        "root.pem": root,
        "intermediate.pem": intermediate,
        "leaf.pem": leaf("Chain Signer", intermediate, intermediate_key),
        "leaf-expired.pem": leaf(
            "Chain Expired",
            intermediate,
            intermediate_key,
            start=datetime.datetime(2020, 1, 1, tzinfo=UTC),
            end=datetime.datetime(2021, 1, 1, tzinfo=UTC),
        ),
        "leaf-future.pem": leaf(
            "Chain Future",
            intermediate,
            intermediate_key,
            start=datetime.datetime(2040, 1, 1, tzinfo=UTC),
            end=datetime.datetime(2041, 1, 1, tzinfo=UTC),
        ),
        "not-ca.pem": not_ca,
        "leaf-under-not-ca.pem": leaf("Chain Under Not A CA", not_ca, not_ca_key),
        "leaf-cert-sign.pem": leaf(
            "Chain Cert Sign Only", intermediate, intermediate_key, key_usage=ca_usage
        ),
        "stranger-root.pem": stranger,
        "stranger-leaf.pem": leaf("Stranger Signer", stranger, stranger_key),
    }
    for file_name, certificate in certificates.items():
        (OUT / file_name).write_bytes(certificate.public_bytes(serialization.Encoding.PEM))
        print(f"wrote {file_name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
