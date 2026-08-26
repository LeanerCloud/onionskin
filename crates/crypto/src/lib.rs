//! Signature verification, PAdES signing later, and the encryption and
//! decryption handlers `cos` uses. Appending to an encrypted document
//! means encrypting new objects with the existing key material, so this
//! is a kernel concern from the start rather than a late addition.
