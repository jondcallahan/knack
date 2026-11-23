#!/bin/sh
# Seeds a clean, synthetic skill library with zero personal data for screenshots.
set -eu

if [ $# -gt 0 ]; then
  DEMO_HOME="$1"
  if [ -z "$DEMO_HOME" ]; then
    echo "Destination path must not be empty." >&2
    exit 1
  fi
  if [ -e "$DEMO_HOME" ]; then
    echo "Refusing to overwrite existing destination: $DEMO_HOME" >&2
    exit 1
  fi
else
  DEMO_HOME="$(mktemp -d /tmp/knack-demo-home.XXXXXX)"
fi

mkdir -p \
  "$DEMO_HOME/.agents/skills" \
  "$DEMO_HOME/.claude/skills" \
  "$DEMO_HOME/.codex/skills" \
  "$DEMO_HOME/.cursor/skills" \
  "$DEMO_HOME/.gemini/skills" \
  "$DEMO_HOME/dev/acme-web/.cursor/skills" \
  "$DEMO_HOME/dev/atlas-api/.agents/skills" \
  "$DEMO_HOME/dev/atlas-api/.claude/commands" \
  "$DEMO_HOME/dev/swift-notes/.agents/skills" \
  "$DEMO_HOME/.config/knack"

# 1. Primary shared skill (rich Markdown + TypeScript snippet + supporting script)
mkdir -p "$DEMO_HOME/.agents/skills/pr-review-checklist/scripts"
cat > "$DEMO_HOME/.agents/skills/pr-review-checklist/SKILL.md" <<'MD'
---
name: pr-review-checklist
description: Structured pull request review covering correctness, edge cases, test coverage, and backwards compatibility.
license: MIT
allowed-tools: Read, Grep, Glob, Bash
---

# PR Review Checklist

Review the diff as an owner before leaving comments. Focus on behavior regressions, data races, and unguarded inputs rather than style nitpicks.

```ts
export async function reviewPullRequest(diff: DiffFile[], runChecks: () => Promise<boolean>) {
  // Block approval when migrations drop columns without passing checks.
  const risky = diff.filter((file) => file.path.includes("migrations/") && file.deletions > 0);
  if (risky.length > 0 && !(await runChecks())) {
    return { approved: false, reason: "destructive migration without passing checks" };
  }
  return { approved: true, filesReviewed: diff.length };
}
```

## Pass Order

1. **Correctness & invariants** — verify null handling, transaction boundaries, and error propagation.
2. **Test fidelity** — confirm tests exercise real state transitions rather than mocking away the unit under test.

| Severity | Meaning | Blocking |
| --- | --- | --- |
| `must` | Data loss, auth bypass, or crash | Yes |
| `should` | Missing test or unclear failure mode | Author choice |
| `nit` | Naming or small simplification | No |
MD
cat > "$DEMO_HOME/.agents/skills/pr-review-checklist/scripts/check-diff.sh" <<'SH'
#!/bin/sh
set -eu
git diff --stat main...HEAD
SH
chmod +x "$DEMO_HOME/.agents/skills/pr-review-checklist/scripts/check-diff.sh"
ln -s "$DEMO_HOME/.agents/skills/pr-review-checklist" "$DEMO_HOME/.claude/skills/pr-review-checklist"
ln -s "$DEMO_HOME/.agents/skills/pr-review-checklist" "$DEMO_HOME/.codex/skills/pr-review-checklist"

# 2. SQL Migration Guard
mkdir -p "$DEMO_HOME/.agents/skills/sql-migration-guard"
cat > "$DEMO_HOME/.agents/skills/sql-migration-guard/SKILL.md" <<'MD'
---
name: sql-migration-guard
description: Catch table locks, unindexed foreign keys, and non-reversible schema changes in SQL migrations.
---

# SQL Migration Guard

Check every `ALTER TABLE` statement before running against staging or production.

```sql
-- Prefer concurrent index creation to avoid blocking writes.
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_invoices_account_created
  ON invoices (account_id, created_at DESC);
```
MD
ln -s "$DEMO_HOME/.agents/skills/sql-migration-guard" "$DEMO_HOME/.claude/skills/sql-migration-guard"
ln -s "$DEMO_HOME/.agents/skills/sql-migration-guard" "$DEMO_HOME/.cursor/skills/sql-migration-guard"

# 3. Release Notes
mkdir -p "$DEMO_HOME/.agents/skills/release-notes"
cat > "$DEMO_HOME/.agents/skills/release-notes/SKILL.md" <<'MD'
---
name: release-notes
description: Turn merged pull requests into concise, customer-facing changelog entries grouped by impact.
---

# Release Notes

Summarize user-visible improvements first, followed by bug fixes and breaking changes. Omit internal refactors unless they alter behavior.
MD
ln -s "$DEMO_HOME/.agents/skills/release-notes" "$DEMO_HOME/.claude/skills/release-notes"
ln -s "$DEMO_HOME/.agents/skills/release-notes" "$DEMO_HOME/.gemini/skills/release-notes"

# 4. Commit Hygiene
mkdir -p "$DEMO_HOME/.agents/skills/commit-hygiene"
cat > "$DEMO_HOME/.agents/skills/commit-hygiene/SKILL.md" <<'MD'
---
name: commit-hygiene
description: Split unrelated hunks into atomic commits with imperative summaries and verification notes.
---

# Commit Hygiene

Stage only files touched for the current task, verify the test suite passes on the staged tree, and avoid committing generated artifacts.
MD
ln -s "$DEMO_HOME/.agents/skills/commit-hygiene" "$DEMO_HOME/.codex/skills/commit-hygiene"

# 5. Disabled global skill
mkdir -p "$DEMO_HOME/.agents/skills/legacy-deploy"
cat > "$DEMO_HOME/.agents/skills/legacy-deploy/SKILL.md.disabled" <<'MD'
---
name: legacy-deploy
description: Previous rsync deployment helper replaced by GitHub Actions release workflow.
---

# Legacy Deploy

Superseded by automated release builds.
MD

# 6. Project skills under ~/dev
mkdir -p "$DEMO_HOME/dev/atlas-api/.agents/skills/api-design-guide"
cat > "$DEMO_HOME/dev/atlas-api/.agents/skills/api-design-guide/SKILL.md" <<'MD'
---
name: api-design-guide
description: REST pagination, idempotency keys, and structured problem+json error contracts for Atlas API.
---

# API Design Guide

All mutating endpoints accept an `Idempotency-Key` header and return RFC 9457 error objects on 4xx/5xx responses.
MD
mkdir -p "$DEMO_HOME/dev/atlas-api/.claude/skills"
ln -s "$DEMO_HOME/dev/atlas-api/.agents/skills/api-design-guide" "$DEMO_HOME/dev/atlas-api/.claude/skills/api-design-guide"

# 7. Claude slash command in project
cat > "$DEMO_HOME/dev/atlas-api/.claude/commands/ship.md" <<'MD'
---
description: Run migrations test suite, tag release candidate, and post deployment checklist.
---

Verify tests pass cleanly, confirm schema diff is backwards compatible, and prepare the release PR.
MD

# 8. Acme Web Cursor project skill
mkdir -p "$DEMO_HOME/dev/acme-web/.cursor/skills/component-audit"
cat > "$DEMO_HOME/dev/acme-web/.cursor/skills/component-audit/SKILL.md" <<'MD'
---
name: component-audit
description: Check keyboard focus rings, color contrast, and reduced-motion fallbacks across UI primitives.
---

# Component Audit

Verify every interactive element has a visible focus indicator and respects `prefers-reduced-motion`.
MD

# 9. Swift Notes skill
mkdir -p "$DEMO_HOME/dev/swift-notes/.agents/skills/swift-concurrency"
cat > "$DEMO_HOME/dev/swift-notes/.agents/skills/swift-concurrency/SKILL.md" <<'MD'
---
name: swift-concurrency
description: Enforce strict Sendable boundaries and cancellation checks inside async SwiftUI tasks.
---

# Swift Concurrency

Isolate view models with `@MainActor` and check `Task.isCancelled` before writing persisted store updates.
MD
mkdir -p "$DEMO_HOME/dev/swift-notes/.claude/skills" "$DEMO_HOME/dev/swift-notes/.codex/skills"
ln -s "$DEMO_HOME/dev/swift-notes/.agents/skills/swift-concurrency" "$DEMO_HOME/dev/swift-notes/.claude/skills/swift-concurrency"
ln -s "$DEMO_HOME/dev/swift-notes/.agents/skills/swift-concurrency" "$DEMO_HOME/dev/swift-notes/.codex/skills/swift-concurrency"

cat > "$DEMO_HOME/.config/knack/prefs.json" <<'JSON'
{
  "folders": ["~/dev"],
  "scope": "all",
  "status": "all",
  "selected": null,
  "new_dirs": ["~/.agents/skills", "~/.claude/skills"],
  "side_width": 236.0,
  "list_width": 320.0
}
JSON

echo "Seeded demo home at $DEMO_HOME"
