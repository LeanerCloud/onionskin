#!/usr/bin/env bash
#
# Regenerate corpus/proof/recorded.txt: every run the corpus CI steps are
# claimed on, in one file, so the claim and its evidence move together.
#
#   ./corpus/proof/record.sh > corpus/proof/recorded.txt
#
# Wall times are not in here. They depend on the machine and on whether cargo
# has a warm cache, so a number recorded once and read later is worse than no
# number; the ones in the pull request say what they were measured on.

set -uo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR

banner() {
	printf '\n================================================================\n'
	printf '%s\n' "$1"
	printf '================================================================\n'
}

printf 'Recorded by corpus/proof/record.sh. Regenerate rather than edit.\n'
printf 'rustc: %s\n' "$(rustc --version)"

banner 'The rerun over the corpus CI fetches. Every suite must pass.'
"$SCRIPT_DIR/rerun.sh"

banner 'The fetch step removed. The rerun must fail.'
"$SCRIPT_DIR/rerun.sh" --without external

banner 'The fetch step half-done, external/ present and empty. Must fail.'
"$SCRIPT_DIR/rerun.sh" --empty external

banner 'Both generators removed. The suites that need them must fail.'
"$SCRIPT_DIR/rerun.sh" --without malformed --without bench

banner 'The tripwire, one mutation of the workflow at a time.'
"$SCRIPT_DIR/tripwire.sh"
