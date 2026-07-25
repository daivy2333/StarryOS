#!/usr/bin/env bash

set -u
set -o pipefail

failures=0

gate() {
    local name=$1
    shift
    printf '\n[%s]\n' "$name"
    "$@"
    local rc=$?
    printf 'exit=%s\n' "$rc"
    if [ "$rc" -ne 0 ]; then
        failures=$((failures + 1))
    fi
}

g1_references() {
    printf '%s\n' "command: compare all R IDs with the eight-ID allowlist"
    diff -u \
        <(printf '%s\n' R14 R23 R24 R25 R26 R38 R39 R40) \
        <(rg -o 'R[0-9]+' openspec/specs/references/spec.md | LC_ALL=C sort -u)
}

g2_carrier_pointer() {
    printf '%s\n' "command: count cleanup carrier paths in references"
    local count
    count=$(rg -F -o 'openspec/changes/archive/2026-07-25-cleanup-uart-docs/' \
        openspec/specs/references/spec.md | wc -l)
    printf 'count=%s\n' "$count"
    test "$count" -eq 1
}

g3_state() {
    printf '%s\n' "command: require canonical deferred state and reject completion claims"
    local canonical='UART 文档已归档；q17 multi-hart SMP 验证 deferred（task 6.1 未完成）'
    rg -F "$canonical" .claude/docs/SNAPSHOT.md || return 1
    rg -F "$canonical" .claude/docs/tasks.md || return 1
    ! rg -n 'UART 阶段.*全部完成|Q0.?Q32.*全部完成|UART 工作.*全部完成|^## UART 阶段回顾' \
        .claude/docs/SNAPSHOT.md .claude/docs/tasks.md
}

g4_control_docs() {
    printf '%s\n' "command: reject old ARC paths and Q-stage IDs in active control docs"
    ! rg -n 'openspec/changes/ARC-202607251326|Q([0-9]+)' \
        openspec/specs/{project-model,decisions,knowledge,references,improvements,quality-gate-baseline,platform-descriptor-early-console}/spec.md \
        .claude/docs/SNAPSHOT.md .claude/docs/tasks.md .claude/analysis/README.md
}

g5_references_content() {
    printf '%s\n' "command: reject STALE, removed learned references, and malformed comment row"
    ! rg -n 'STALE|`learned`|^<!--.*-->.*\|$' openspec/specs/references/spec.md
}

g6_analysis_index() {
    printf '%s\n' "command: compare Active analysis entries and check Archived heading"
    diff -u \
        <(printf '%s\n' arceos-async-network-driver-analysis.md arceos-true-board-validation.md async-network-project-overview.md embassy-network-module-evaluation.md starryos-async-network-roadmap.md) \
        <(awk '/^## Active$/{on=1;next}/^## /{on=0}on' .claude/analysis/README.md \
            | sed -n 's/^- `\([^`]*\.md\)`.*/\1/p' | LC_ALL=C sort -u) || return 1
    test "$(rg -c '^## Archived$' .claude/analysis/README.md)" -eq 1 || return 1
    ! rg -n '[。）；）]## ' .claude/analysis/README.md
}

g7_runbook() {
    printf '%s\n' "command: reject D1, Q-stage, UART benchmark, and old threshold in board Runbook"
    ! rg -n 'D1|Q[0-9]+|UART|串口 benchmark|93%' .claude/runbooks/board-bringup-ladder.md
}

g8_archive_notes() {
    printf '%s\n' "command: verify incomplete counts and reject phase-complete claims"
    rg -n '18/19|Task 6\.1.*NOT executed' \
        openspec/changes/archive/2026-07-25-q17-smp-memory-ordering/ARCHIVE_NOTE.md || return 1
    rg -n '16/18|2 tasks incomplete' \
        openspec/changes/archive/2026-07-25-arc-202607251326/ARCHIVE_NOTE.md || return 1
    ! rg -n 'phase complete|phase completed|阶段.*完成' \
        openspec/changes/archive/2026-07-25-q17-smp-memory-ordering/ARCHIVE_NOTE.md \
        openspec/changes/archive/2026-07-25-arc-202607251326/ARCHIVE_NOTE.md
}

g9_manifest() {
    printf '%s\n' "command: compare 003 manifest with 48 carrier files and verify every SHA-256"
    local manifest='openspec/changes/cleanup-uart-documentation-system/evidence/003-contract-closeout/archive-manifest.tsv'
    local actual_count manifest_count checked expected path got
    actual_count=$(find \
        openspec/changes/archive/2026-07-25-cleanup-uart-docs \
        openspec/changes/archive/2026-07-25-arc-202607251326 \
        openspec/changes/archive/2026-07-25-q17-smp-memory-ordering \
        -type f | wc -l)
    manifest_count=$(awk -F '\t' 'NF >= 2 {count++} END {print count+0}' "$manifest")
    printf 'actual=%s manifest=%s\n' "$actual_count" "$manifest_count"
    test "$actual_count" -eq 48 || return 1
    test "$manifest_count" -eq 48 || return 1
    checked=0
    while IFS=$'\t' read -r expected path; do
        got=$(sha256sum "$path" | awk '{print $1}')
        test "$expected" = "$got" || return 1
        checked=$((checked + 1))
    done < "$manifest"
    printf 'hash_checked=%s\n' "$checked"
    test "$checked" -eq 48 || return 1
}

g10_archive_delta() {
    printf '%s\n' "command: compare 002 and 003 manifests; allow only two ARCHIVE_NOTE paths"
    local old_manifest='openspec/changes/cleanup-uart-documentation-system/evidence/002-closeout/archive-manifest-complete.tsv'
    local new_manifest='openspec/changes/cleanup-uart-documentation-system/evidence/003-contract-closeout/archive-manifest.tsv'
    diff -u \
        <(printf '%s\n' \
            openspec/changes/archive/2026-07-25-arc-202607251326/ARCHIVE_NOTE.md \
            openspec/changes/archive/2026-07-25-q17-smp-memory-ordering/ARCHIVE_NOTE.md) \
        <(join -t $'\t' \
            <(awk -F ' \\| ' '!/^#/ && NF >= 3 {print $2 "\t" $1}' "$old_manifest" | LC_ALL=C sort) \
            <(awk -F '\t' 'NF >= 2 {print $2 "\t" $1}' "$new_manifest" | LC_ALL=C sort) \
            | awk -F '\t' '$2 != $3 {print $1}')
}

g11_historical_evidence() {
    printf '%s\n' "command: find 000-002 Evidence newer than the pre-edit RED baseline"
    local red='openspec/changes/cleanup-uart-documentation-system/evidence/003-contract-closeout/red-baseline.txt'
    local changed
    changed=$(find \
        openspec/changes/cleanup-uart-documentation-system/evidence/000-initial \
        openspec/changes/cleanup-uart-documentation-system/evidence/001-review-fixes \
        openspec/changes/cleanup-uart-documentation-system/evidence/002-closeout \
        -type f -newer "$red" -print)
    printf '%s' "$changed"
    test -z "$changed"
}

g12_product_scope() {
    printf '%s\n' "command: inspect tracked and untracked status for product-code paths"
    local paths
    paths=$(git status --short | awk '{print $2}' \
        | rg '\.(rs|c|h|S|s)$|(^|/)(Cargo\.(toml|lock)|Makefile)$' || true)
    printf '%s' "$paths"
    test -z "$paths"
}

g13_validation() {
    printf '%s\n' "command: openspec validate --all; openspec list; openspec list --specs"
    openspec validate --all &&
        openspec list &&
        openspec list --specs
}

g14_diff() {
    printf '%s\n' "command: git diff --check"
    git diff --check
}

gate G1_REFERENCES g1_references
gate G2_CARRIER_POINTER g2_carrier_pointer
gate G3_STATE g3_state
gate G4_CONTROL_DOCS g4_control_docs
gate G5_REFERENCES_CONTENT g5_references_content
gate G6_ANALYSIS_INDEX g6_analysis_index
gate G7_RUNBOOK g7_runbook
gate G8_ARCHIVE_NOTES g8_archive_notes
gate G9_MANIFEST g9_manifest
gate G10_ARCHIVE_DELTA g10_archive_delta
gate G11_HISTORICAL_EVIDENCE g11_historical_evidence
gate G12_PRODUCT_SCOPE g12_product_scope
gate G13_OPENSPEC g13_validation
gate G14_DIFF g14_diff

printf '\nfailures=%s\n' "$failures"
exit "$failures"
