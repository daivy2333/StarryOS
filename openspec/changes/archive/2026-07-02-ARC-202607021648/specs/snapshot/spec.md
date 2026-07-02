# Spec Delta: snapshot — ARC-202607021648

## ADDED Requirements

### Requirement: Snapshot remains current-state oriented

`.claude/docs/SNAPSHOT.md` MUST present current state first and compress old phase history into archive pointers.

#### Scenario: Restoring old snapshot expansion

- **WHEN** developers need Q5-Q15 historical expansion
- **THEN** they SHOULD use existing archived changes, `tasks.md` milestone summaries, and this carrier as the cleanup record.
