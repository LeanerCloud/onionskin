#!/usr/bin/env python3
"""Generate the signed fixtures in corpus/signed/.

Every fixture is signed by pyHanko, an independent implementation of PDF
signing, with keys and certificates from a test certificate authority this
script makes on each run. The CA's certificate is written beside the
fixtures as `test-ca.pem`, so a test can trust it the way a user trusts a
certificate, and nothing else is trusted.

Fixtures, by what they exercise:

  rsa-sha256.pdf        adbe.pkcs7.detached, RSA 2048 PKCS#1 v1.5, SHA-256
  rsa-pss.pdf           RSA-PSS, SHA-256
  ecdsa-p256.pdf        ECDSA P-256, SHA-256
  ecdsa-p384.pdf        ECDSA P-384, SHA-384
  rsa-sha1.pdf          SHA-1: valid, and weak
  cades.pdf             ETSI.CAdES.detached (PAdES baseline B)
  certified-p1.pdf      a certification signature, no changes allowed
  certified-p2.pdf      a certification signature, form filling allowed
  two-signatures.pdf    signed, then signed again in a second section
  annotated-after.pdf   signed, then a note added in a later section by
                        pyHanko: the signature still covers its revision
  tampered.pdf          a byte of the signed content changed: invalid
  unsigned-field.pdf    an empty signature field

Not byte-deterministic: keys and signing times change each run. The tests
assert what a validator decides, never bytes.
"""

from __future__ import annotations

import datetime
import io
import sys
from pathlib import Path

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, rsa
from cryptography.x509.oid import NameOID
from pyhanko.pdf_utils.incremental_writer import IncrementalPdfFileWriter
from pyhanko.pdf_utils import generic
from pyhanko.sign import fields, signers
from pyhanko.sign.fields import MDPPerm, SigSeedSubFilter
from pyhanko_certvalidator.registry import SimpleCertificateStore

HERE = Path(__file__).resolve().parent
OUT = HERE / "signed"
SEED = HERE / "seeds" / "hello.pdf"
NOW = datetime.datetime.now(datetime.timezone.utc)


def name(common: str) -> x509.Name:
    return x509.Name(
        [
            x509.NameAttribute(NameOID.COMMON_NAME, common),
            x509.NameAttribute(NameOID.ORGANIZATION_NAME, "Onionskin Test"),
        ]
    )


def certificate(subject, issuer, public, signer, ca: bool, hash_alg=hashes.SHA256()):
    builder = (
        x509.CertificateBuilder()
        .subject_name(subject)
        .issuer_name(issuer)
        .public_key(public)
        .serial_number(x509.random_serial_number())
        .not_valid_before(NOW - datetime.timedelta(days=1))
        .not_valid_after(NOW + datetime.timedelta(days=3650))
        .add_extension(x509.BasicConstraints(ca=ca, path_length=None), critical=True)
    )
    if ca:
        builder = builder.add_extension(
            x509.KeyUsage(False, False, False, False, False, True, True, False, False),
            critical=True,
        )
    else:
        builder = builder.add_extension(
            x509.KeyUsage(True, True, False, False, False, False, False, False, False),
            critical=True,
        )
    return builder.sign(signer, hash_alg)


def pem(key) -> bytes:
    return key.private_bytes(
        serialization.Encoding.PEM,
        serialization.PrivateFormat.PKCS8,
        serialization.NoEncryption(),
    )


def make_signer(ca_key, ca_cert, key, common: str, prefer_pss=False):
    cert = certificate(name(common), ca_cert.subject, key.public_key(), ca_key, ca=False)
    from asn1crypto import keys as asn1_keys, x509 as asn1_x509

    return signers.SimpleSigner(
        signing_cert=asn1_x509.Certificate.load(cert.public_bytes(serialization.Encoding.DER)),
        signing_key=asn1_keys.PrivateKeyInfo.load(
            key.private_bytes(
                serialization.Encoding.DER,
                serialization.PrivateFormat.PKCS8,
                serialization.NoEncryption(),
            )
        ),
        cert_registry=SimpleCertificateStore.from_certs(
            [asn1_x509.Certificate.load(ca_cert.public_bytes(serialization.Encoding.DER))]
        ),
        prefer_pss=prefer_pss,
    )


def sign(source: bytes, signer, field: str, **meta) -> bytes:
    writer = IncrementalPdfFileWriter(io.BytesIO(source))
    subfilter = meta.pop("subfilter", SigSeedSubFilter.ADOBE_PKCS7_DETACHED)
    digest = meta.pop("md_algorithm", "sha256")
    certify = meta.pop("certify", False)
    docmdp = meta.pop("docmdp", None)
    signature_meta = signers.PdfSignatureMetadata(
        field_name=field,
        md_algorithm=digest,
        subfilter=subfilter,
        certify=certify,
        docmdp_permissions=docmdp or MDPPerm.FILL_FORMS,
        reason="Testing Onionskin",
        location="Berlin",
        name=meta.pop("signer_name", None),
    )
    out = io.BytesIO()
    signers.PdfSigner(
        signature_meta,
        signer=signer,
        new_field_spec=fields.SigFieldSpec(sig_field_name=field, on_page=0, box=(72, 72, 272, 132)),
    ).sign_pdf(writer, output=out)
    return out.getvalue()


def add_note(source: bytes) -> bytes:
    """A text annotation on page one, in a new section."""
    writer = IncrementalPdfFileWriter(io.BytesIO(source))
    page = writer.root["/Pages"]["/Kids"][0]
    page_obj = page.get_object()
    note = generic.DictionaryObject(
        {
            generic.NameObject("/Type"): generic.NameObject("/Annot"),
            generic.NameObject("/Subtype"): generic.NameObject("/Text"),
            generic.NameObject("/Rect"): generic.ArrayObject(
                [generic.FloatObject(v) for v in (300, 300, 320, 320)]
            ),
            generic.NameObject("/Contents"): generic.TextStringObject("Added after signing"),
        }
    )
    note_ref = writer.add_object(note)
    annots = page_obj.get("/Annots", generic.ArrayObject())
    annots = generic.ArrayObject(list(annots.get_object()) + [note_ref])
    page_obj[generic.NameObject("/Annots")] = annots
    writer.update_container(page_obj)
    out = io.BytesIO()
    writer.write(out)
    return out.getvalue()


def empty_field(source: bytes) -> bytes:
    writer = IncrementalPdfFileWriter(io.BytesIO(source))
    fields.append_signature_field(
        writer, fields.SigFieldSpec(sig_field_name="Empty", on_page=0, box=(72, 200, 272, 260))
    )
    out = io.BytesIO()
    writer.write(out)
    return out.getvalue()


def tamper(signed: bytes) -> bytes:
    """Change one byte inside the first signed range: the page's text."""
    at = signed.find(b"Hello")
    assert at > 0, "the seed says Hello"
    changed = bytearray(signed)
    changed[at] = ord("J")
    return bytes(changed)


def main() -> int:
    OUT.mkdir(exist_ok=True)
    seed = SEED.read_bytes()
    ca_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    ca_name = name("Onionskin Test CA")
    ca_cert = certificate(ca_name, ca_name, ca_key.public_key(), ca_key, ca=True)
    (OUT / "test-ca.pem").write_bytes(ca_cert.public_bytes(serialization.Encoding.PEM))

    rsa_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    rsa_signer = make_signer(ca_key, ca_cert, rsa_key, "Ada Signer")
    pss_signer = make_signer(ca_key, ca_cert, rsa_key, "Ada Signer", prefer_pss=True)
    p256_signer = make_signer(ca_key, ca_cert, ec.generate_private_key(ec.SECP256R1()), "Pat P256")
    p384_signer = make_signer(ca_key, ca_cert, ec.generate_private_key(ec.SECP384R1()), "Pat P384")

    fixtures = {
        "rsa-sha256.pdf": sign(seed, rsa_signer, "Signature1"),
        "rsa-pss.pdf": sign(seed, pss_signer, "Signature1"),
        "ecdsa-p256.pdf": sign(seed, p256_signer, "Signature1"),
        "ecdsa-p384.pdf": sign(seed, p384_signer, "Signature1", md_algorithm="sha384"),
        "rsa-sha1.pdf": sign(seed, rsa_signer, "Signature1", md_algorithm="sha1"),
        "cades.pdf": sign(seed, rsa_signer, "Signature1", subfilter=SigSeedSubFilter.PADES),
        "certified-p1.pdf": sign(
            seed, rsa_signer, "Certification", certify=True, docmdp=MDPPerm.NO_CHANGES
        ),
        "certified-p2.pdf": sign(
            seed, rsa_signer, "Certification", certify=True, docmdp=MDPPerm.FILL_FORMS
        ),
    }
    first = fixtures["rsa-sha256.pdf"]
    fixtures["two-signatures.pdf"] = sign(first, p256_signer, "Signature2")
    fixtures["annotated-after.pdf"] = add_note(first)
    fixtures["tampered.pdf"] = tamper(first)
    fixtures["unsigned-field.pdf"] = empty_field(seed)

    for file_name, data in fixtures.items():
        (OUT / file_name).write_bytes(data)
        print(f"wrote {file_name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
