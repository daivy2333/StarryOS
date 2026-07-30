# Migration Verification

Verified: 2026-07-30

## Source Integrity

```text
(cd openspec/changes/mig-202607301654 && sha256sum -c active-sources.sha256)
2/2 OK

sha256sum -c openspec/changes/mig-202607301654/historical-carriers.sha256
39/39 OK
```

The two archived active originals are byte-identical to their pre-migration sources. The previous MIG and six historical ARC carriers remain byte-identical to the registered baseline.

## Unit and Reverse Coverage

Persisted artifacts were compared byte-for-byte against fresh script output:

```text
cmp unit-coverage.tsv <(migration_unit_audit.py --format tsv)       PASS
cmp source-registry.tsv <(migration_unit_audit.py --format sources) PASS
cmp numbering-map.md <(migration_unit_audit.py --format map)        PASS
cmp target-coverage.tsv <(migration_unit_audit.py --format targets) PASS

source_units=2743
mapped_source_units=2743
verified_source_units=2743
unmapped=0
skipped=0
coverage=100.00%
```

## Framework and Skill Entries

```text
CLAUDE.md == current openspec-init public template                 PASS
change-iteration.md == current openspec-init iteration template   PASS
current_role_skills=10
project_openspec_skills=15
missing_files=0
content_mismatches=0
bad_frontmatter=0
agents_link=../.claude/skills
```

## OpenSpec and Diff Gate

```text
openspec validate --specs --strict --no-interactive
24 passed, 0 failed

openspec validate --changes --strict --no-interactive
2 passed, 0 failed

git diff --check
exit 0
```

No product-code build was required: this change modifies only OpenSpec, project-state, templates, references and skill entry documents.
