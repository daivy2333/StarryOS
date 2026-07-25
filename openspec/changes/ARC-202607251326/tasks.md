# ARC-202607251326 — UART Archive Tasks

## 1. Carrier creation
- [x] 1.1 Create carrier directory and metadata
- [x] 1.2 Write proposal with full mapping table
- [ ] 1.3 Write carrier specs with preserved content
- [ ] 1.4 Verify carrier completeness

## 2. Source edits — specs
- [x] 2.1 Edit project-model/spec.md: archive 28 M entries, keep 12
- [x] 2.2 Edit decisions/spec.md: archive 16 D entries, keep 5
- [x] 2.3 Edit knowledge/spec.md: archive 18 K entries, keep 12
- [x] 2.4 Edit references/spec.md: archive 11 R entries, keep 10
- [x] 2.5 Edit improvements/spec.md: archive 8 I entries, keep 3

## 3. Source edits — analysis/runbooks
- [x] 3.1 Move 3 UART analyses to _archive/ (q17, q20, async-uart-cpu-efficiency)
- [x] 3.2 Move 3 UART runbooks to _archive/ (qemu-build, d1-build-and-flash, benchmark-qemu-d1)
- [x] 3.3 Update analysis README.md

## 4. Source edits — state docs
- [x] 4.1 Rewrite SNAPSHOT.md for net-k3 branch
- [x] 4.2 Rewrite tasks.md: compress UART history, add NIC N0-N5 roadmap

## 5. Validation
- [x] 5.1 Check all source files for structural integrity (7 files, 856 lines total)
- [x] 5.2 Verify all arc tombstones reference correct carrier (5 spec files confirmed)
- [x] 5.3 Verify keep entries are intact (M:12, D:5, K:12, I:3, R:~10)
- [x] 5.4 git diff --check (exit 0, clean)
