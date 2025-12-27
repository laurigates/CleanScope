Create a new Architecture Decision Record.

Arguments:
- $ARGUMENTS: ADR title (required)

Template location: .claude/blueprints/adr/

Steps:
1. Count existing ADRs in .claude/blueprints/adr/
2. Generate next number: 004, 005, etc.
3. Create filename: NNN-<kebab-case-title>.md
4. Use ADR template with sections:
   - Status (Proposed)
   - Context
   - Decision
   - Options Considered
   - Consequences
   - Related Decisions
   - References
5. Update .manifest.json if present

ADR Template:
```markdown
# <Title>

## Status
Proposed

## Context
[Describe the context and problem that needs a decision]

## Decision
[Describe the decision that was made]

## Options Considered
### Option 1: [Name]
- Pros: ...
- Cons: ...

### Option 2: [Name]
- Pros: ...
- Cons: ...

## Consequences
### Positive
- ...

### Negative
- ...

## Related Decisions
- [Link to related ADRs]

## References
- [External links and documentation]
```

Output:
- Created ADR file path
- Next steps for filling in content
