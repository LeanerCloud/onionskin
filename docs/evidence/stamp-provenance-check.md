# Stamp provenance check evidence

Recorded 2026-09-23 at `fdbe290` in the isolated
`codex/stamp-provenance-check` worktree.

## Baseline and resolution

`python3 tools/stamps.py --check` exited 1 and reported all 22 committed SVG
assets as different. Each SVG has a root-level SVG `metadata` element with one
`{http://c2pa.org/manifest}manifest` payload. The generator does not emit this
provenance block, while the drawing content and Rust catalog are unchanged.

The check now keeps exact string comparison as the first path. For an SVG
string mismatch only, it parses both trees, removes at most one root-level
metadata node when its structure is exactly the allowed C2PA manifest shape,
transfers its tail whitespace, and compares the remaining complete XML trees.
Malformed XML, DOCTYPE, processing instructions, unexpected metadata, and
artwork changes remain mismatches. The check preserves the provenance bytes and
does not authenticate the C2PA manifest.

## Tests

The Rust integration test creates a fresh temporary fixture containing the
script, catalog, and all 22 assets at their real paths. It invokes the actual
`--check` CLI and asserts fixture bytes are unchanged. It covers committed
provenance, a plain generated SVG obtained in memory without generator write
mode, root size, geometry, valid fill-color, text, drawable siblings before and
after metadata, drawable content inside the manifest, metadata attributes,
manifest attributes, wrong namespace, nested or multiple metadata, significant
text before metadata, malformed XML, DOCTYPE, processing instructions including
an XML stylesheet, and changed catalog or extra or missing files. A valid XML
declaration remains accepted. Each negative case requires exit 1 naming its
mutated fixture target; every fixture is byte-stable after `--check`.

Commands:

```text
python3 tools/stamps.py --check
cargo test --locked -p onionskin-tools-comment --test stamps -- --nocapture
```

The Cargo command used the shared `lockf` build lock, low-debug settings, and
the shared target directory. Final result: 10 tests passed. Native UI checks
were deferred because the Mac was locked.

## Unchanged output hashes

The following SHA-256 values were recorded before implementation and compared
again after verification. The source assets and generated catalog were not
modified.

```text
f0afb259c7b5a488d104ad72032a09c02bd6b0d0f2f8c76deee4ad9a75f793d3  assets/stamps/business-approved.svg
963e96e7c37e6af213b5276befd106dc6c4bbb224e60a3a5cb8e36f59cd6b0d4  assets/stamps/business-completed.svg
26c115f1907b57f8334877eb6f23c0cbfc99cf65440f9f01c1b3d0f9f0eda68a  assets/stamps/business-confidential.svg
de28ad003f7c047649be0bbf415477aa442b5c1ef7a3147ff24ce2daec7d93d6  assets/stamps/business-draft.svg
6f6bdbd637746dbc91045bf0b76b64b0766338914e57391e2ea10c9d1044c3fe  assets/stamps/business-final.svg
65c2d647b3e506321a58370c2774be25bd3ad3ec40cc57fd2e1503f3a817f8af  assets/stamps/business-for-comment.svg
a69fb1c8b93d985b29d546d3fafd5efa75de18b6890f2e0e82f8639ef24ec034  assets/stamps/business-for-public-release.svg
26c970216b4459cf50738d6e271896ac181d9358d5b81086cb9d944cf6e6311a  assets/stamps/business-information-only.svg
76d56bfc6450b20b46232c5da0097dc62b597fed7f5cbc8979db6584aeb857bf  assets/stamps/business-not-approved.svg
b026e0a614568dd1b1ae9ea96c458066c84f41ea34036f22d199a7f9b035efb8  assets/stamps/business-not-for-public-release.svg
d17398ed73a13f43ccc455040efeb2186f74912c3902ec1edf1e7bb55663d683  assets/stamps/business-preliminary-results.svg
7736a42e4681d85de118302f97cf3c53bf684936049d26cb2d6832603d83d1bc  assets/stamps/business-void.svg
0481c5ac947be49ffe7edec5ec0160e5caa773daacc4c3ba4262037a052af1ba  assets/stamps/dynamic-approved.svg
c066bb6bf08cf51077ae7a3e255dc22fedcc23a06bb839169df854731e64fea0  assets/stamps/dynamic-confidential.svg
076767271fd980e36fe960681ef138e5e29edb83fb981131da109b85b8d89a5a  assets/stamps/dynamic-received.svg
77432ba60a621e5dfbca0ce74b17e6055057790f8862b9811d09479a533b45fc  assets/stamps/dynamic-reviewed.svg
70cda9350984fdd46f8848c31cc32cb45dcb21423ff83b7862ea1150f5f0401f  assets/stamps/dynamic-revised.svg
58e2ad3da17650c4c527e83821f1bccf7b4f193ec03e6d2bbdd19d6c757492d4  assets/stamps/sign-accepted.svg
768710e8c5cc84e9261594b20d78a1caefb0b7484f33fb5ba49b1b332a7207b1  assets/stamps/sign-initial-here.svg
d3f7d3c7eb45b4302d91e4d47fc92dba69ea000f67e00e878c529893e85cc1a8  assets/stamps/sign-rejected.svg
ffe82d2600c6a925d6a7a66a5b9e6118e644475a6c862597fac8794d4bdd2da1  assets/stamps/sign-sign-here.svg
890f7c5e9b06d0492ac57c6bf614a0a8f0151ea15a06fdc06710194b3fd44b8e  assets/stamps/sign-witness.svg
2bcd67f415fdd845feba0359580e0dc3aa59765cf957a286cad56f65c65ecdb3  plugins/tools-comment/src/stamp/catalog.rs
```
