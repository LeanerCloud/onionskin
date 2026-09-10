#!/usr/bin/env bash
#
# Run CI's corpus rerun locally, against a corpus with pieces taken away.
#
# The rerun exists to turn a silent skip into a failure. That claim is only
# worth anything if somebody has watched it fail, so this reads the command
# list out of the workflow itself - not a copy of it - and runs it against a
# corpus root assembled here, with the named parts omitted.
#
#   ./corpus/proof/rerun.sh                      the whole corpus, all pass
#   ./corpus/proof/rerun.sh --without external   the fetch step removed
#   ./corpus/proof/rerun.sh --empty external     the fetch step half-done
#   ./corpus/proof/rerun.sh --without malformed --without bench
#
# The assembled root is symlinks into the repository's own corpus, so nothing
# is copied and nothing in corpus/ is written to.

set -uo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
readonly CORPUS_DIR="$(dirname -- "$SCRIPT_DIR")"
readonly WORKSPACE="$(dirname -- "$CORPUS_DIR")"
readonly WORKFLOW="$WORKSPACE/.github/workflows/ci.yml"
readonly RERUN_STEP='name: Prove every corpus suite measured its corpus'

omit=()
empty=()
while [[ $# -gt 0 ]]; do
	case "$1" in
	--without)
		omit+=("$2")
		shift 2
		;;
	--empty)
		empty+=("$2")
		shift 2
		;;
	*)
		printf 'unknown argument: %s\n' "$1" >&2
		exit 2
		;;
	esac
done

is_named() {
	local needle="$1" name
	shift
	for name in "$@"; do
		[[ "$name" == "$needle" ]] && return 0
	done
	return 1
}

root="$(mktemp -d "${TMPDIR:-/tmp}/onionskin-rerun.XXXXXX")"
trap 'rm -rf -- "$root"' EXIT

for part in "$CORPUS_DIR"/*/; do
	name="$(basename -- "$part")"
	[[ "$name" == "proof" || "$name" == "checksums" ]] && continue
	if is_named "$name" ${omit[@]+"${omit[@]}"}; then
		continue
	fi
	if is_named "$name" ${empty[@]+"${empty[@]}"}; then
		mkdir -p -- "$root/$name"
		continue
	fi
	ln -s -- "$part" "$root/$name"
done

printf 'corpus root: %s\n' "$root"
for part in "$root"/*; do
	[[ -e "$part" ]] || continue
	printf '  %-12s %s\n' "$(basename -- "$part")" \
		"$(find -L "$part" -type f -iname '*.pdf' | wc -l | tr -d '[:space:]') PDFs"
done
printf '\n'

export ONIONSKIN_CORPUS="$root"
export ONIONSKIN_CORPUS_REQUIRED=1
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0

passed=0
failed=0
while IFS= read -r command; do
	log="$(mktemp)"
	if $command --manifest-path "$WORKSPACE/Cargo.toml" >"$log" 2>&1; then
		printf 'PASS  %s\n' "$command"
		passed=$((passed + 1))
	else
		reason="$(grep -m1 -A1 'panicked at' "$log" | tail -1)"
		printf 'FAIL  %s\n        %s\n' "$command" "${reason:-see the run log}"
		failed=$((failed + 1))
	fi
	rm -f -- "$log"
done < <(sed -n "/$RERUN_STEP/,/^      - /p" "$WORKFLOW" |
	grep -E '^ +cargo test ' | sed 's/^ *//')

printf '\n%d passed, %d failed\n' "$passed" "$failed"
[[ "$failed" -eq 0 ]]
