#!/usr/bin/env bash
#
# Download the public PDF corpora that Onionskin's guarantee tests run against
# into corpus/external/. Run with --help for usage, see README.md for what each
# set contains and how it is licensed.

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
readonly EXTERNAL_DIR="$SCRIPT_DIR/external"
readonly CHECKSUM_DIR="$SCRIPT_DIR/checksums"
readonly R2_HELPER="$SCRIPT_DIR/r2.py"
readonly SHA256_VERIFIER="$SCRIPT_DIR/verify-sha256.py"
readonly STAMP_NAME=".fetch-stamp"

# Per-run temporary directory, created by main and removed on exit.
SCRATCH_DIR=""

# --- Pinned upstream revisions ------------------------------------------------
#
# A codeload tarball is not byte-stable across git server versions, so the
# reproducibility pin is the commit SHA, not a tarball checksum. The SHA fixes
# the file set and the file contents exactly.

readonly HAYRO_REPO="LaurenzV/hayro"
readonly HAYRO_REV="5a5f0e247c970df948505ee0bb36e2df2504bf86"

readonly VERAPDF_REPO="veraPDF/veraPDF-corpus"
readonly VERAPDF_REV="49de56cd987929932c9e4fbbbe67d052bf44ef83"

readonly PDF20EXAMPLES_REPO="pdf-association/pdf20examples"
readonly PDF20EXAMPLES_REV="c20f2c17bfcc4baab7cfe62e70fae64caf14d5fa"

readonly SAFEDOCS_REPO="pdf-association/safedocs"
readonly SAFEDOCS_REV="a6fd37308c91a0d2c17ebcace970367181bc0da7"

readonly PDFDIFFERENCES_REPO="pdf-association/pdf-differences"
readonly PDFDIFFERENCES_REV="26caa8795933269f3a38530369c70813486eabee"

# hayro keeps only ~360 of its test PDFs in git. The rest live in a Cloudflare
# R2 bucket keyed by the ids in the manifests, and are fetched one file at a
# time from this base. The manifests come from the pinned hayro revision, so the
# id list is pinned even though the objects themselves carry no version.
readonly HAYRO_ASSETS_BASE="https://hayro-assets.dev"

# --- Set catalogue ------------------------------------------------------------

# Sets fetched when no arguments are given: ~3340 files, ~200 MB.
readonly DEFAULT_SETS=(hayro pdf-association verapdf)

# Opt-in sets, all of them hayro's R2-hosted extensions. hayro-corpus is only 41
# files but 159 MB of them, most of that in a handful of large scans;
# hayro-pdfjs, hayro-pdfbox and hayro-pdfium add ~1260 files of unbounded total
# size. Leaving all four out is the cut that keeps a default fetch inside a few
# hundred megabytes.
readonly OPTIONAL_SETS=(hayro-corpus hayro-pdfjs hayro-pdfbox hayro-pdfium)

log() {
	printf 'fetch: %s\n' "$*"
}

die() {
	printf 'fetch: %s\n' "$*" >&2
	exit 1
}

need_cmd() {
	command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

is_python() {
	[[ "$("$1" -c 'import sys; sys.stdout.write("onionskin-python") if sys.version_info >= (3, 10) else sys.exit(1)' 2>/dev/null)" == "onionskin-python" ]]
}

python_exe() {
	local candidate
	if [[ -n "${PYTHON:-}" ]]; then
		candidate="$PYTHON"
		if is_python "$candidate"; then
			printf '%s' "$candidate"
			return 0
		fi
		die "PYTHON is not a runnable Python 3.10+ interpreter: $candidate"
	fi

	for candidate in python3 python; do
		if command -v "$candidate" >/dev/null 2>&1 &&
			is_python "$candidate"; then
			printf '%s' "$candidate"
			return 0
		fi
	done

	die "required Python 3.10+ interpreter not found (tried PYTHON, python3, python)"
}

# Path of the stamp that marks a fetched unit as complete.
stamp_path() {
	printf '%s/%s' "$1" "$STAMP_NAME"
}

is_present() {
	[[ -f "$(stamp_path "$1")" ]]
}

write_stamp() {
	local dest="$1" source="$2" revision="$3"
	{
		printf 'source=%s\n' "$source"
		printf 'revision=%s\n' "$revision"
	} >"$(stamp_path "$dest")"
}

# A set directory without a stamp is an interrupted fetch. Say so instead of
# deleting it, so nothing this script did not create gets removed silently.
require_absent_or_stamped() {
	local dest="$1"
	if [[ -e "$dest" ]] && ! is_present "$dest"; then
		die "$dest exists but has no $STAMP_NAME (interrupted fetch?). Remove it and re-run."
	fi
}

# Download a repository tarball at a pinned revision and place one subdirectory
# of it (or the whole tree, when subpath is ".") at dest.
#
# The tarball is extracted into a scratch directory first because the top-level
# directory name is chosen by the server. Reading it back from the archive
# listing avoids hardcoding the repo-SHA convention, and avoids tar wildcard
# flags, which differ between GNU tar and the bsdtar on macOS.
fetch_tarball() {
	local dest="$1" repo="$2" revision="$3" subpath="$4"
	local url="https://codeload.github.com/$repo/tar.gz/$revision"

	if is_present "$dest"; then
		log "present, skipping: $dest"
		return 0
	fi
	require_absent_or_stamped "$dest"

	# Guard the rm below: an empty SCRATCH_DIR would make it rm -rf /tarball.
	[[ -d "$SCRATCH_DIR" ]] || die "scratch directory was never created"
	local scratch="$SCRATCH_DIR/tarball"
	rm -rf -- "$scratch"
	mkdir -p -- "$scratch"

	log "downloading $repo@${revision:0:12}"
	curl --fail --show-error --silent --location --retry 3 \
		--output "$scratch/archive.tar.gz" "$url" ||
		die "download failed: $url"

	# Write the listing to a file rather than piping it into head. GNU tar dies
	# of SIGPIPE once head closes the pipe, which pipefail turns into a fatal
	# exit 141 for every archive larger than the pipe buffer. bsdtar tolerates
	# the EPIPE, so this only ever failed on Linux.
	tar -tzf "$scratch/archive.tar.gz" >"$scratch/listing.txt"
	local top first=""
	IFS= read -r first <"$scratch/listing.txt" || true
	top="${first%%/*}"
	[[ -n "$top" ]] || die "empty or unreadable archive for $repo@$revision"

	tar -xzf "$scratch/archive.tar.gz" -C "$scratch"

	local extracted="$scratch/$top"
	[[ "$subpath" == "." ]] || extracted="$scratch/$top/$subpath"
	[[ -d "$extracted" ]] || die "$subpath is not in $repo@$revision"

	# Stage next to the destination so the final move is a same-filesystem
	# rename; the scratch directory may be on another device.
	mkdir -p -- "$(dirname -- "$dest")"
	rm -rf -- "$dest.partial"
	mv -- "$extracted" "$dest.partial"
	mv -- "$dest.partial" "$dest"
	write_stamp "$dest" "$repo/$subpath" "$revision"
	log "fetched $dest"
}

# Read the pdf ids out of one of hayro's manifest files. The helper validates
# the whole manifest before anything reaches a URL or a filesystem path.
manifest_ids() {
	local manifest="$1"
	local python
	require_r2_helper
	python="$(python_exe)"
	PYTHONDONTWRITEBYTECODE=1 "$python" "$R2_HELPER" ids "$manifest"
}

checksum_manifest_for_remote_set() {
	local kind="$1"
	case "$kind" in
	corpus)
		printf '%s' "$CHECKSUM_DIR/hayro-corpus.sha256"
		;;
	pdfjs | pdfbox | pdfium)
		return 1
		;;
	*)
		die "unknown hayro remote set: $kind"
		;;
	esac
}

verify_remote_set() {
	local dest="$1" manifest="$2"
	local python

	require_checksum_inputs "$manifest"
	python="$(python_exe)"
	PYTHONDONTWRITEBYTECODE=1 "$python" "$SHA256_VERIFIER" "$manifest" "$dest" ||
		die "checksum verification failed for $dest using $manifest"
}

require_r2_helper() {
	[[ -f "$R2_HELPER" ]] || die "R2 helper missing: $R2_HELPER"
}

require_checksum_inputs() {
	local manifest="$1"

	[[ -f "$manifest" ]] || die "checksum manifest missing: $manifest"
	require_r2_helper
	[[ -f "$SHA256_VERIFIER" ]] || die "checksum verifier missing: $SHA256_VERIFIER"
}

r2_destination_state() {
	local dest="$1"
	local python
	require_r2_helper
	python="$(python_exe)"
	PYTHONDONTWRITEBYTECODE=1 "$python" "$R2_HELPER" check-dest "$dest"
}

publish_r2_set() {
	local staging="$1" dest="$2" source="$3" revision="$4" checksum_manifest="$5"
	local python
	require_r2_helper
	python="$(python_exe)"
	PYTHONDONTWRITEBYTECODE=1 "$python" "$R2_HELPER" publish \
		"$staging" "$dest" "$source" "$revision" "$checksum_manifest"
}

# Fetch one of hayro's R2-hosted sets, one pdf per id in the pinned manifest.
# Downloads land in a per-run staging directory and are published only after
# destination and checksum validation have both passed.
fetch_hayro_remote_set() {
	local kind="$1"
	local dest="$EXTERNAL_DIR/hayro-$kind"
	local manifest="$EXTERNAL_DIR/hayro/manifest_$kind.json"
	local checksum_manifest=""
	if checksum_manifest="$(checksum_manifest_for_remote_set "$kind")"; then
		readonly checksum_manifest
	fi

	local dest_state
	dest_state="$(r2_destination_state "$dest")" ||
		die "invalid R2 destination: $dest"
	if [[ "$dest_state" == "stamped" ]]; then
		if [[ -n "$checksum_manifest" ]]; then
			verify_remote_set "$dest" "$checksum_manifest"
		else
			log "present without checksum enforcement: $dest"
		fi
		log "present, skipping: $dest"
		return 0
	fi

	if [[ -n "$checksum_manifest" ]]; then
		require_checksum_inputs "$checksum_manifest"
	fi

	ensure_set hayro
	dest_state="$(r2_destination_state "$dest")" ||
		die "invalid R2 destination after fetching hayro manifest set: $dest"
	[[ "$dest_state" == "absent" ]] ||
		die "R2 destination changed before download: $dest"
	[[ -f "$manifest" ]] || die "manifest missing after fetching hayro: $manifest"

	# Read the ids through a file rather than a process substitution, so a
	# python failure is reported as itself instead of surfacing later as a
	# misleading "no ids" message.
	[[ -d "$SCRATCH_DIR" ]] || die "scratch directory was never created"
	local id_list="$SCRATCH_DIR/$kind.ids"
	manifest_ids "$manifest" >"$id_list" ||
		die "could not read ids from $manifest"

	local ids=()
	while IFS= read -r id; do
		ids+=("$id")
	done <"$id_list"
	[[ "${#ids[@]}" -gt 0 ]] || die "manifest lists no ids: $manifest"

	[[ -d "$SCRATCH_DIR" ]] || die "scratch directory was never created"
	local staging="$SCRATCH_DIR/hayro-$kind"
	rm -rf -- "$staging"
	mkdir -p -- "$staging"
	log "downloading ${#ids[@]} files from $HAYRO_ASSETS_BASE/$kind/"

	local downloaded=0 id target
	for id in "${ids[@]}"; do
		target="$staging/$id.pdf"
		curl --fail --show-error --silent --location --retry 3 \
			--output "$target.partial" "$HAYRO_ASSETS_BASE/$kind/$id.pdf" ||
			die "download failed: $HAYRO_ASSETS_BASE/$kind/$id.pdf"
		mv -- "$target.partial" "$target"
		downloaded=$((downloaded + 1))
	done

	local unchecked=false
	local publish_checksum_manifest="--no-checksum"
	if [[ -n "$checksum_manifest" ]]; then
		verify_remote_set "$staging" "$checksum_manifest"
		publish_checksum_manifest="$checksum_manifest"
	else
		unchecked=true
	fi
	publish_r2_set "$staging" "$dest" \
		"$HAYRO_ASSETS_BASE/$kind/ (ids from hayro manifest_$kind.json)" \
		"$HAYRO_REV" "$publish_checksum_manifest" ||
		die "could not publish fetched R2 set: $dest"
	if [[ "$unchecked" == true ]]; then
		log "fetched without checksum enforcement: $dest"
	fi
	log "fetched $dest ($downloaded downloaded)"
}

# --- Sets ---------------------------------------------------------------------

set_hayro() {
	fetch_tarball "$EXTERNAL_DIR/hayro" "$HAYRO_REPO" "$HAYRO_REV" "hayro-tests"
}

set_hayro_corpus() { fetch_hayro_remote_set corpus; }
set_hayro_pdfjs() { fetch_hayro_remote_set pdfjs; }
set_hayro_pdfbox() { fetch_hayro_remote_set pdfbox; }
set_hayro_pdfium() { fetch_hayro_remote_set pdfium; }

set_pdf_association() {
	fetch_tarball "$EXTERNAL_DIR/pdf-association/pdf20examples" \
		"$PDF20EXAMPLES_REPO" "$PDF20EXAMPLES_REV" "."
	fetch_tarball "$EXTERNAL_DIR/pdf-association/safedocs" \
		"$SAFEDOCS_REPO" "$SAFEDOCS_REV" "."
	fetch_tarball "$EXTERNAL_DIR/pdf-association/pdf-differences" \
		"$PDFDIFFERENCES_REPO" "$PDFDIFFERENCES_REV" "."
}

set_verapdf() {
	fetch_tarball "$EXTERNAL_DIR/verapdf" "$VERAPDF_REPO" "$VERAPDF_REV" "."
}

# --- Driver -------------------------------------------------------------------

all_sets() {
	printf '%s\n' "${DEFAULT_SETS[@]}" "${OPTIONAL_SETS[@]}"
}

set_function() {
	local name="$1"
	local candidate="set_${name//-/_}"
	declare -F "$candidate" >/dev/null 2>&1 || return 1
	printf '%s' "$candidate"
}

ensure_set() {
	local name="$1" fn
	fn="$(set_function "$name")" || die "unknown set: $name"
	"$fn"
}

set_root() {
	printf '%s/%s' "$EXTERNAL_DIR" "$1"
}

list_sets() {
	local name root state
	printf '%-20s %-10s %s\n' SET DEFAULT STATE
	while IFS= read -r name; do
		root="$(set_root "$name")"
		if [[ -d "$root" ]]; then state=present; else state=absent; fi
		local is_default=no
		local candidate
		for candidate in "${DEFAULT_SETS[@]}"; do
			if [[ "$candidate" == "$name" ]]; then
				is_default=yes
			fi
		done
		printf '%-20s %-10s %s\n' "$name" "$is_default" "$state"
	done < <(all_sets)
}

summarise() {
	local name root count size
	printf '\n%-24s %8s %10s\n' SET PDFS SIZE
	while IFS= read -r name; do
		root="$(set_root "$name")"
		[[ -d "$root" ]] || continue
		count="$(find "$root" -type f -iname '*.pdf' | wc -l | tr -d '[:space:]')"
		size="$(du -sh "$root" | cut -f1 | tr -d '[:space:]')"
		printf '%-24s %8s %10s\n' "$name" "$count" "$size"
	done < <(all_sets)

	count="$(find "$EXTERNAL_DIR" -type f -iname '*.pdf' | wc -l | tr -d '[:space:]')"
	size="$(du -sh "$EXTERNAL_DIR" | cut -f1 | tr -d '[:space:]')"
	printf '%-24s %8s %10s\n' TOTAL "$count" "$size"
}

usage() {
	cat <<'EOF'
Download the public PDF corpora used by Onionskin's guarantee tests.

Usage:
  ./fetch.sh                 fetch the default sets
  ./fetch.sh SET [SET ...]   fetch only the named sets
  ./fetch.sh --list          list every set and whether it is already present

Sets are pinned to exact upstream git revisions. A set that is already fetched
is skipped, so re-running is cheap and works offline. To refresh a set after
bumping its pinned revision, delete its directory under corpus/external/ and
run again. See README.md for contents and licensing.

Sets:
EOF
	list_sets
}

main() {
	need_cmd curl
	need_cmd tar

	case "${1-}" in
	-h | --help)
		usage
		return 0
		;;
	--list)
		list_sets
		return 0
		;;
	esac

	local requested=("$@")
	if [[ "${#requested[@]}" -eq 0 ]]; then
		requested=("${DEFAULT_SETS[@]}")
	fi

	local name
	for name in "${requested[@]}"; do
		set_function "$name" >/dev/null || die "unknown set: $name (try --list)"
	done

	SCRATCH_DIR="$(mktemp -d "${TMPDIR:-/tmp}/onionskin-corpus.XXXXXX")"
	# Single-quoted so SCRATCH_DIR expands when the trap fires, not now. Quoting
	# the value into the trap string instead would break on a TMPDIR containing
	# a single quote.
	trap 'rm -rf -- "$SCRATCH_DIR"' EXIT

	mkdir -p -- "$EXTERNAL_DIR"
	for name in "${requested[@]}"; do
		ensure_set "$name"
	done

	summarise
}

main "$@"
