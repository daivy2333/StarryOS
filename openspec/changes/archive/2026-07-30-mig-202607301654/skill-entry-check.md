# Cross-platform Skill Entry Check

Verified: 2026-07-30

## Layout

- Canonical project copy: `.claude/skills/`.
- Codex/OpenCode entry: `.agents/skills -> ../.claude/skills`.
- Current role suite: `openspec-act`, `openspec-archivist`, `openspec-assistant`, `openspec-compressor`, `openspec-docs-maintainer`, `openspec-experience-recorder`, `openspec-explorer`, `openspec-init`, `openspec-milestone-planner`, `openspec-plan`.
- Retained CLI compatibility entries: `openspec-apply-change`, `openspec-archive-change`, `openspec-explore`, `openspec-propose`, `openspec-sync-specs`.

## Verification

```text
current_role_skills=10
project_openspec_skills=15
missing_files=0
content_mismatches=0
bad_frontmatter=0
agents_link=../.claude/skills
```

All 10 current role-skill files and references are byte-identical to the installed source suite. Every `openspec-*/SKILL.md` frontmatter contains exactly `name` and `description`.
