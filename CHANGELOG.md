# Changelog

Notable changes to Knack. Releases are on [GitHub](https://github.com/jondcallahan/knack/releases).

## Unreleased

### Added

- **GitHub source tracking.** For skills installed with `npx skills add`, Knack reads the `skills` CLI's lock files (`~/.agents/.skill-lock.json` and each project's `skills-lock.json`) and checks GitHub on launch and on ⌘R. It shows each skill's source repository and tells you when an update is available, when you've edited your copy since installing it, when both happened, or when the repository no longer has the skill. It signs in with `GITHUB_TOKEN`, `GH_TOKEN`, or the GitHub CLI when available.

## 0.1.2 — 2026-11-23

### Added

- DMG installer artwork.

## 0.1.1 — 2026-11-23

### Changed

- Releases are signed with a Developer ID and notarized by Apple.

## 0.1.0 — 2026-11-23

First release: browse, edit, enable or disable, and share agent skills across Claude Code, Codex, Cursor, Gemini, OpenCode, and project folders.
