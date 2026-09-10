#!/usr/bin/env bash
#
# Prove the corpus tripwire bites.
#
# `crates/app/tests/guarantees.rs` asserts that CI still fetches the corpus and
# still re-runs every suite that can reach it. A gate nobody has watched fail
# is a gate nobody should trust, so each mutation below is applied to a copy of
# the reviewed workflow, the tripwire is run against it, and the original is
# put back before the next one. The last line is the baseline, which must pass.
#
#   ./corpus/proof/tripwire.sh
#
# Nothing here is committed state: the workflow is restored on every path out,
# including a failed mutation.

set -uo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
readonly WORKSPACE="$(dirname -- "$(dirname -- "$SCRIPT_DIR")")"
readonly WORKFLOW="$WORKSPACE/.github/workflows/ci.yml"

reviewed="$(mktemp)"
trap 'cp -- "$reviewed" "$WORKFLOW"; rm -f -- "$reviewed"' EXIT
cp -- "$WORKFLOW" "$reviewed"

export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0

tripwire() {
	cargo test --manifest-path "$WORKSPACE/Cargo.toml" -p onionskin-app --test guarantees \
		-- --exact every_corpus_suite_is_rerun_with_the_corpus_required 2>&1
}

# Applies one edit to the workflow, runs the tripwire, restores. The edit is a
# Python program given the workflow path as argv[1]; it must fail loudly rather
# than silently not applying, or a mutation that stopped matching would read as
# a gate that stopped biting.
mutate() {
	local label="$1" program="$2"
	cp -- "$reviewed" "$WORKFLOW"
	printf '\n=== %s ===\n' "$label"
	if ! python3 -c "$program" "$WORKFLOW"; then
		printf 'MUTATION DID NOT APPLY - this proves nothing\n'
		cp -- "$reviewed" "$WORKFLOW"
		return
	fi
	tripwire | grep -E 'panicked at|assertion .* failed|test result' || true
	cp -- "$reviewed" "$WORKFLOW"
}

drop_step() {
	printf '%s' "
import sys
from pathlib import Path
p = Path(sys.argv[1]); t = p.read_text()
step = '''$1'''
assert t.count(step) == 1, t.count(step)
p.write_text(t.replace(step, '', 1))
"
}

mutate "remove the fetch step" "$(drop_step "      - name: Fetch the corpus the guarantee suites walk
        if: runner.os == 'Linux'
        run: ./corpus/fetch.sh
")"

mutate "remove the hayro-corpus fetch step" "$(drop_step "      - name: Fetch the corpus set the round-trip and extraction suites name
        if: runner.os == 'Linux'
        run: ./corpus/fetch.sh hayro-corpus
")"

mutate "remove the malformed generator" "$(drop_step "      - name: Generate the malformed corpus set
        if: runner.os == 'Linux'
        run: ./corpus/make-malformed.sh
")"

mutate "remove the thousand-page generator" "$(drop_step "      - name: Generate the thousand-page corpus file
        if: runner.os == 'Linux'
        run: ./corpus/make-bench.py
")"

mutate "remove the extraction oracle install" "$(drop_step "      - name: Install the extraction oracle
        if: runner.os == 'Linux'
        run: sudo apt-get install -y poppler-utils
")"

mutate "remove one suite from the rerun list" "
import sys
from pathlib import Path
p = Path(sys.argv[1]); t = p.read_text()
old = '          cargo test -p onionskin-cos --test lazy\n'
assert t.count(old) == 1
p.write_text(t.replace(old, '', 1))
"

mutate "add a suite that reaches no corpus" "
import sys
from pathlib import Path
p = Path(sys.argv[1]); t = p.read_text()
old = '          cargo test -p onionskin-cos --test lazy\n'
assert t.count(old) == 1
p.write_text(t.replace(old, old + '          cargo test -p onionskin-core --test geometry\n', 1))
"

mutate "condition the rerun away" "
import sys
from pathlib import Path
p = Path(sys.argv[1]); t = p.read_text()
old = '''      - name: Prove every corpus suite measured its corpus
        if: runner.os == 'Linux'
'''
assert t.count(old) == 1
p.write_text(t.replace(old, old.replace(chr(39) + 'Linux' + chr(39), 'false'), 1))
"

mutate "swallow the rerun's failures" "
import sys
from pathlib import Path
p = Path(sys.argv[1]); t = p.read_text()
old = '          cargo test -p onionskin-codecs-common --test roundtrip\n'
assert t.count(old) == 1
p.write_text(t.replace(old, '          set +e\n' + old, 1))
"

mutate "drop the corpus-required env from the rerun" "
import sys
from pathlib import Path
p = Path(sys.argv[1]); t = p.read_text()
old = '''        env:
          ONIONSKIN_CORPUS_REQUIRED: 1
        run: |
          cargo test -p onionskin-codecs-common'''
assert t.count(old) == 1
p.write_text(t.replace(old, '''        run: |
          cargo test -p onionskin-codecs-common''', 1))
"

mutate "fetch the corpus after the suites that read it" "
import sys
from pathlib import Path
p = Path(sys.argv[1]); t = p.read_text()
step = '''      - name: Fetch the corpus the guarantee suites walk
        if: runner.os == 'Linux'
        run: ./corpus/fetch.sh
'''
assert t.count(step) == 1
t = t.replace(step, '', 1)
run = chr(32) * 6 + '- run: cargo test --workspace' + chr(10)
assert t.count(run) == 1
p.write_text(t.replace(run, run + step, 1))
"

mutate "add a fetch step the cache key does not name" "
import sys
from pathlib import Path
p = Path(sys.argv[1]); t = p.read_text()
step = '''      - name: Fetch the corpus the guarantee suites walk
        if: runner.os == 'Linux'
        run: ./corpus/fetch.sh
'''
assert t.count(step) == 1
extra = '''      - name: Fetch another set
        if: runner.os == 'Linux'
        run: ./corpus/fetch.sh hayro-pdfjs
'''
p.write_text(t.replace(step, step + extra, 1))
"

mutate "point the cache key at nothing that pins a revision" "
import sys
from pathlib import Path
p = Path(sys.argv[1]); t = p.read_text()
q = chr(39)
old = 'hashFiles(' + q + 'corpus/fetch.sh' + q + ', '
assert t.count(old) == 2, t.count(old)
p.write_text(t.replace(old, 'hashFiles(', 1))
"

mutate "let the corpus cache fall back to a prefix" "
import sys
from pathlib import Path
p = Path(sys.argv[1]); t = p.read_text()
old = '          path: corpus/external\n'
assert t.count(old) == 2, t.count(old)
p.write_text(t.replace(old, old + '          restore-keys: corpus-test-\n', 1))
"

mutate "let the whole test job fail without failing the build" "
import sys
from pathlib import Path
p = Path(sys.argv[1]); t = p.read_text()
old = '''  test:
    strategy:
'''
assert t.count(old) == 1
p.write_text(t.replace(old, '''  test:
    continue-on-error: true
    strategy:
''', 1))
"

mutate "condition the whole test job away" "
import sys
from pathlib import Path
p = Path(sys.argv[1]); t = p.read_text()
old = '''  test:
    strategy:
'''
assert t.count(old) == 1
p.write_text(t.replace(old, '''  test:
    if: false
    strategy:
''', 1))
"

printf '\n=== baseline, must pass ===\n'
cp -- "$reviewed" "$WORKFLOW"
tripwire | grep -E 'panicked at|assertion .* failed|test result' || true
