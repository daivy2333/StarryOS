# Implementation — MS05 Iteration 011 / Cycle 002

- Change: ms05-qemu-bounded-bidirectional-device-data-plane
- Iteration: 011-independent-manual-qemu-runtime-and-closeout-review
- Cycle: 002-rework
- Revision: `2af394e6cc8e6aa9ae7026d7ede136382258a98b` (net-k3; worktree carries
  the accepted task 6.2-R1 first-TX wake product diff)

## Files and symbols changed (this Cycle)

- `scripts/ms05_evidence_capture.py`
  - `SCHEMA_VERSION = 2`; added `CHANGE_EVIDENCE_ROOT`.
  - New `evidence_exclusion(cwd, root)`: derive the fixed change Evidence
    subtree, reject a root outside it / outside the repository / unsafe
    boundary.
  - New `git_readonly_untracked(cwd, exclusion)`: untracked enumeration with
    `--exclude-per-directory=.gitignore` only (ignores `.git/info/exclude` and
    global excludes), then filters the Evidence subtree by exact prefix.
  - `source_identity(cwd, exclusion=None)`: applies the exclusion as explicit
    Git pathspecs to index identity, tracked binary diff and untracked content
    identity.
  - `freeze_source` / `verify_frozen` / `build_manifest`: record and re-check
    `evidence_exclusion`; manifest writes schema v2 with the exclusion.
- `scripts/ms05_evidence_audit.py`
  - `SCHEMA_VERSION_V1/V2` + `SUPPORTED_SCHEMA_VERSIONS`; `load_manifest`
    accepts v1 and v2.
  - `audit_source_freeze`: for v2 derives the fixed exclusion and rejects
    missing/different (`EXCLUSION_MISSING`/`EXCLUSION_MISMATCH`); for v1 rejects
    an unexpected exclusion field (`EXCLUSION_UNEXPECTED`); identity computed
    with the exclusion for v2, without for v1 (v1 binding preserved).
  - `build_valid_fixture` writes a v2 manifest with the exclusion; new negative
    fixtures `make_missing_exclusion`, `make_forged_exclusion`,
    `make_v1_unexpected_exclusion`.
- `tests/test_ms05_evidence_tools.py`
  - New `TestPortableIdentityContract`: self-reference does not drift excluded
    identity; without exclusion it drifts; info/exclude-hidden source still
    drifts v2 identity; untracked enumeration ignores info/exclude/global;
    `evidence_exclusion` rejects out-of-subtree and returns the fixed subtree.
- `.git/info/exclude`
  - Removed the single local exclude line hiding the Cycle 001 Evidence root
    (required state cleanup); the original `.codegraph` rule is unchanged.
- `evidence/011-.../002-rework/`
  - `manifest.json`, `logs/*`, `qualification.json`, `env-blocked.json`,
    `evidence-audit.log`, `artifacts.sha256`, `commands.txt`, `README.md`,
    `implementation.md` (this file).

## Verification summary

- Gate 1 RED: (1) self-reference drift without exclusion; (2) info/exclude
  hidden source absent from old identity; (3) no forged-exclusion rejection in
  old audit. All reproduced before the fix.
- Tools: capture self-test PASS; audit negative fixtures PASS (including the
  three new exclusion fixtures); `python3 -m unittest
  tests.test_ms05_evidence_tools` 15/15 PASS.
- Fresh automatic capture (schema v2): 44/44 records `pass`, no env-blocked.
- v2 positive audit PASS; qualification binding VERIFIED;
  `sha256sum -c artifacts.sha256` 6/6 OK.
- Historical v1 Cycle 001 qualification binding still VERIFIED.
- Static four-session command audit of `commands.txt` PASS (19 checks).

## Gates reached

- Gate 3 (test witness): RED observed, GREEN after fix.
- Gate 4 (spec/code review): the tooling diff is scoped to the evidence-tool
  identity/exclusion contract; product data-plane behavior unchanged.
- Gate 5 (static command audit): PASS.
- Gates 6-7 (manual QEMU runtime) are R44 capability boundaries — Act stops
  after Gate 5 and resumes when the user returns the raw outputs.
