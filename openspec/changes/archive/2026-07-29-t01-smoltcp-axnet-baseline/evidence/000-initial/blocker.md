# Blocker: dependency graph still resolves the fork

- Discovered at: task 2.1, dependency source Gate
- Result: BLOCKED
- Date: 2026-07-29

## Expected

The path dependency should make the QEMU tree use only local `axnet-ng` and
smoltcp 0.13.1. `starry-smoltcp` should disappear.

## Actual

The local dependency replaces the kernel's direct `axnet-ng` edge. Other
edges remain:

```text
starry-smoltcp 0.12.1-preview.1
├── axnet 0.3.0-preview.2
│   ├── axfeat 0.3.0-preview.2
│   └── axruntime 0.3.0-preview.2
└── axnet-ng 0.3.0-preview.2
    └── axruntime 0.3.0-preview.2
```

Command:

```sh
cargo tree --offline -p starryos --features qemu -i starry-smoltcp
```

The command exited 0. The Gate failed because the forbidden dependency
remains.

## Impact

Task 2.1 needs a new dependency strategy. Options include Cargo patching,
feature changes, or localizing another upstream edge. The approved design
rejects patching and does not select another option.

Tasks 3.1, 4.1, and 4.2 contain partial code that compiles in the isolated
axnet check. They cannot be completed before task 2.1 passes.

## Resume condition

`openspec-plan` must choose and approve a dependency strategy. A new
iteration may resume after its RED/GREEN commands and source assertions are
defined.
