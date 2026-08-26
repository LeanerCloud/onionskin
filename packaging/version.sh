# shellcheck shell=bash
# Resolve the version to package as. Sourced, not executed.
#
# VERSION is authoritative when set: CI exports it from the tag it is
# building. With VERSION_REQUIRED=1, which a tag build sets, a missing or
# malformed VERSION fails instead of falling back - publishing assets stamped
# with the manifest version because someone tagged v1.0 or mistyped is worse
# than publishing nothing. A manual run has no tag and falls back.
onionskin_version() {
    local root="$1"
    # Fully anchored: VERSION arrives from a git tag, and only a plain
    # semver-shaped string is allowed to reach a filename or an installer
    # define.
    if printf '%s' "${VERSION:-}" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+[A-Za-z0-9.+-]*$'; then
        printf '%s' "$VERSION"
        return
    fi
    if [ "${VERSION_REQUIRED:-}" = 1 ]; then
        echo "VERSION '${VERSION:-}' is not semver-shaped; tag releases as vX.Y.Z" >&2
        return 1
    fi
    local from_manifest
    from_manifest=$(grep -m1 '^version = ' "$root/Cargo.toml" | cut -d'"' -f2)
    if [ -z "$from_manifest" ]; then
        echo "no version in $root/Cargo.toml and none in VERSION" >&2
        return 1
    fi
    printf '%s' "$from_manifest"
}
