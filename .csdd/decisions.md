# Decisions

<!--
Record consequential decisions when useful. A decision may use stable DEC-NNN
identity, status and date, context, decision, rationale, and consequences;
add alternatives or supersession only when useful. Do not infer rationale or
accepted direction from implementation alone.
-->

## DEC-001 — GitHub is the work tracker

- Status: accepted
- Date: 2026-10-15
- Context: CSDD initialization. The maintainer directed init without
  `.csdd/todo.md` because the tracker is GitHub.
- Decision: This repository does not keep `.csdd/todo.md`. Operational work
  is tracked in GitHub Issues and pull requests on
  https://github.com/ValenFelizia/narrative-universe-spike.
- Rationale: Explicit maintainer instruction at initialization. Contribution
  guidance already requires an approved, scoped issue and a pull request.
- Consequences: `.csdd/` holds specifications, decisions, and handoffs only.
  Task state, ownership, and scope live in GitHub. Do not recreate
  `todo.md` unless this decision is superseded. The default four-document
  CSDD layout is intentionally not adopted in full.
