# Project Analysis

Collected: 2026-07-30T16:54:17+08:00

## Project

- Type: `no_std` Rust operating-system kernel and local driver/platform crates.
- Package: `starryos` / `starry-kernel`, version `0.2.0-preview.2`, Rust edition 2024.
- Toolchain: `rustc 1.95.0-nightly (859951e3c 2026-02-24)`, pinned by `rust-toolchain.toml` to `nightly-2026-02-25`.
- Framework: ArceOS `0.3.0-preview.2`.
- Primary targets: RISC-V 64-bit QEMU virt and Lichee RV Dock D1; manifests also expose x86_64, AArch64 and LoongArch targets.
- Current focus: async UART core, StarryOS adapter, TTY integration, platform descriptors and hardware/QEMU benchmark evidence.

## Commands

| Purpose | Command |
|---|---|
| Default build | `make build` |
| QEMU run | `make run` |
| CI test entry | `make ci-test` |
| Host logic tests | `make host-test` |
| D1 smoke image | `make lichee` |
| D1 async UART benchmark images | `make lichee-kbench`, `make lichee-userbench`, `make lichee-fullbench-mem`, `make lichee-fullbench-command` |
| Format check | `cargo fmt --all -- --check` |
| Static analysis | `cargo clippy` with the feature/target matrix required by the affected change |
| OpenSpec specs | `openspec validate --specs --strict --no-interactive` |
| OpenSpec full | `openspec validate --all --strict --no-interactive` |

Hardware commands require the matching board, toolchain, rootfs/image and serial environment. QEMU evidence does not substitute for true-board throughput or SMP evidence.

## Directories

- Product source: `src/`, `kernel/src/`.
- Local crates: `crates/uart_16550/`, `crates/axplat-riscv64-lichee-d1/`, `crates/axfs-ng/`.
- Tests and benchmarks: `tests/`, `scripts/`, `tools/`, `kernel/resources/`.
- Persistent project documents: `docs/`, `.claude/analysis/`, `.claude/runbooks/`.
- OpenSpec: `openspec/specs/`, `openspec/changes/`.
- State and templates: `.claude/docs/`.

`.claude/incidents/` is absent and remains on-demand. The untracked `crates/smoltcp/` directory existed before this migration and is excluded from project analysis and all writes.

## Git Baseline

- Branch: `uart-lichee`.
- Revision: `79a31dd`.
- Pre-existing worktree change: untracked `crates/smoltcp/`.
- Migration-created change: `openspec/changes/mig-202607301654/`.
- Pre-existing active OpenSpec change: `q17-smp-memory-ordering` (18/19 tasks; multi-hart validation remains deferred).

## Rules and Platform Entrypoints

- Public rules: `CLAUDE.md`.
- Thin adapter: `AGENTS.md`.
- Claude Code skills: `.claude/skills/`.
- Codex skills: `.agents/skills`, currently a symlink to `../.claude/skills`.
- OpenCode: reuses `.agents/skills`; no separate content copy is required.
- `.codex/` exists but is empty.

Existing generated OpenSpec skills use legacy multi-field frontmatter and only expose five CLI-oriented roles. Phase 7 will install the current role suite into the shared skill directory and normalize retained compatibility skills to `name`/`description` frontmatter.
