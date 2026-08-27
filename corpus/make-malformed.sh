#!/usr/bin/env bash
#
# Generate corpus/malformed/ from the seed PDFs in corpus/seeds/.
#
# Every variant is a deterministic byte transform of one seed, so the same seeds
# always produce the same malformed bytes. The suffix on each output name says
# what is broken; guarantee test 6 (repair) consumes the whole directory.
#
# Existing outputs are overwritten in place. Nothing is deleted.

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
readonly SEEDS_DIR="$SCRIPT_DIR/seeds"
readonly OUT_DIR="$SCRIPT_DIR/malformed"

# Exported because the perl one-liners below read them from the environment,
# which keeps the perl source single-quoted and free of shell escaping.

# Bytes added to every in-use xref offset. Large enough to land outside the
# object the entry claims to point at, small enough to stay inside the file.
declare -rx OFFSET_SHIFT=137
# Extra entries the xref subsection header claims beyond the ones present.
declare -rx COUNT_OVERSTATE=3

# Fraction of the original size kept by the truncation variant, in percent.
# Chosen so the xref table and trailer are always gone.
readonly TRUNCATE_PERCENT=60

# 31 characters, so each junk line is 32 bytes with its newline and JUNK_LINES
# of them come to exactly 1024 bytes. That pushes %PDF- past the window a naive
# header scan looks at.
readonly JUNK_LINE="HTTP/1.1 200 OK -- junk padding"
readonly JUNK_LINES=32
readonly JUNK_LINE_LEN=31

die() {
	printf 'make-malformed: %s\n' "$*" >&2
	exit 1
}

need_cmd() {
	command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

# Assert the generated file exists, is non-empty and actually differs from its seed.
check_variant() {
	local seed="$1" out="$2"
	[[ -s "$out" ]] || die "produced an empty file: $out"
	if cmp -s "$seed" "$out"; then
		die "variant is byte-identical to its seed, transform did not apply: $out"
	fi
}

# Shift every in-use xref offset so the table points at the wrong bytes. The
# startxref value stays correct, so a reader finds the table and then has to
# notice that none of its offsets land on an object header.
variant_xref_bad_offsets() {
	local seed="$1" out="$2"
	perl -0777 -pe \
		's/(\d{10}) (\d{5}) n \n/sprintf("%010d %s n \n", $1 + $ENV{OFFSET_SHIFT}, $2)/ge' \
		<"$seed" >"$out"
	check_variant "$seed" "$out"
}

# Prepend junk ahead of %PDF-. Every xref offset is now short by the junk length
# as well, which is how these files behave in the wild.
variant_junk_header() {
	local seed="$1" out="$2" i
	{
		for ((i = 0; i < JUNK_LINES; i++)); do
			printf '%s\n' "$JUNK_LINE"
		done
		cat "$seed"
	} >"$out"
	check_variant "$seed" "$out"
}

# Cut the tail, losing the xref table, the trailer and %%EOF, and leaving the
# last surviving object incomplete.
variant_truncated() {
	local seed="$1" out="$2" size keep
	size="$(wc -c <"$seed" | tr -d '[:space:]')"
	keep=$((size * TRUNCATE_PERCENT / 100))
	[[ "$keep" -gt 0 ]] || die "seed too small to truncate: $seed"
	head -c "$keep" <"$seed" >"$out"
	check_variant "$seed" "$out"
}

# Drop the trailing %%EOF marker only. Everything a reader needs is still
# present and correct, so this isolates "missing end-of-file marker" from every
# other kind of damage.
variant_no_eof() {
	local seed="$1" out="$2" size marker
	marker="$(tail -c 6 <"$seed")"
	[[ "$marker" == "%%EOF" ]] ||
		die "seed does not end with %%EOF and a newline: $seed"
	size="$(wc -c <"$seed" | tr -d '[:space:]')"
	head -c "$((size - 6))" <"$seed" >"$out"
	check_variant "$seed" "$out"
}

# Overstate the entry count in the xref subsection header, so the declared range
# runs past the entries that are actually there and into the trailer.
variant_xref_count_mismatch() {
	local seed="$1" out="$2"
	perl -0777 -pe \
		's/\nxref\n0 (\d+)\n/"\nxref\n0 " . ($1 + $ENV{COUNT_OVERSTATE}) . "\n"/e' \
		<"$seed" >"$out"
	check_variant "$seed" "$out"
}

main() {
	need_cmd perl
	[[ "${#JUNK_LINE}" -eq "$JUNK_LINE_LEN" ]] ||
		die "JUNK_LINE is ${#JUNK_LINE} characters, expected $JUNK_LINE_LEN"
	[[ -d "$SEEDS_DIR" ]] || die "seeds directory is missing: $SEEDS_DIR"

	local seeds=()
	while IFS= read -r path; do
		seeds+=("$path")
	done < <(find "$SEEDS_DIR" -maxdepth 1 -name '*.pdf' | sort)
	[[ "${#seeds[@]}" -gt 0 ]] ||
		die "no seed PDFs in $SEEDS_DIR (run make-seeds.py first)"

	mkdir -p -- "$OUT_DIR"

	local produced=0 seed stem
	for seed in "${seeds[@]}"; do
		stem="$(basename "$seed" .pdf)"
		variant_xref_bad_offsets "$seed" "$OUT_DIR/$stem-xref-bad-offsets.pdf"
		variant_junk_header "$seed" "$OUT_DIR/$stem-junk-header.pdf"
		variant_truncated "$seed" "$OUT_DIR/$stem-truncated.pdf"
		variant_no_eof "$seed" "$OUT_DIR/$stem-no-eof.pdf"
		variant_xref_count_mismatch "$seed" "$OUT_DIR/$stem-xref-count-mismatch.pdf"
		produced=$((produced + 5))
	done

	printf 'make-malformed: %d seeds -> %d variants in %s\n' \
		"${#seeds[@]}" "$produced" "$OUT_DIR"
}

main "$@"
