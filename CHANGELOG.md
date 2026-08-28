# Changelog

All notable changes to Paper Guard are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/) and this project adheres to
[Semantic Versioning](https://semver.org/).

## [0.9.0] — 2026-08-28

### Added

- **Human-readable review report** (a new `paper-guard-report` presentation
  layer). The default CLI output of `paper-guard review` / `paper-guard run` is
  a human-readable report that makes the multi-agent workflow visible: each
  reviewer's purpose, status, and findings, the Judge's consolidated issues
  (with source reviewers), required human approvals, and the integrity /
  validation footer. A failed or disabled reviewer is shown explicitly, never
  silently omitted. The canonical JSON artifacts (`findings.json`,
  `judge.json`, `claims.json`, the ledger) remain unchanged and
  machine-readable.
- **Three presentation styles**, purely presentational and implemented as
  deterministic formatters (no LLM involved in restyling):
  - `neutral` (default) — sober, scientific, professional;
  - `funny` — humorous, lightly ironic;
  - `insulting` — deliberately sharp/biting toward the paper, argument, or
    problem (never ad hominem toward real authors).
  - In all styles the underlying findings — severity, confidence, evidence,
    claims, category, recommendation, Judge decisions, revision scopes — are
    byte-for-byte identical. No evidence, claims, references, results, or
    experiments are ever generated.
- **`--style` CLI flag** (`neutral` / `funny` / `insulting`) on `review` and
  `run`, plus a `[review] style = "neutral"` config key. Resolution priority:
  CLI `--style` > `[review] style` config > `neutral` default. No implicit
  switching via environment variables. Invalid values are rejected with a
  clear error.
- **`--output` CLI flag** (`human` / `summary`) on `review` and `run`. `human`
  (default) prints the full readable report; `summary` prints the compact
  one-liner. JSON artifacts are always written regardless.

### Changed

- The default `paper-guard review/run` output is now the human-readable report
  instead of only the compact summary. Use `--output summary` for the previous
  concise form.

### Security & integrity

- The report is a pure presentation layer generated **from** the canonical run
  record; it can never become a second source of truth and cannot alter any
  scientific content. Failure data (e.g. a failed reviewer) is surfaced, not
  hidden.
- Finding contents are treated as untrusted input and rendered verbatim as
  plain text; no code-execution path exists in the report layer, and no secrets
  or manuscript content are emitted beyond what is already in the canonical
  findings.

## [0.8.0] — 2026-08-28

### Added

- **Robust JSON Schema structured output** for the generic
  `OpenAICompatibleProvider`. `structured_output` now accepts
  `false` / `true` / `"json_object"` / `"json_schema"` (backward compatible
  with the historical `bool`). In `json_schema` mode, a strict JSON Schema is
  derived from the reviewer's strongly-typed finding type (numeric confidence,
  required fields) via `schemars` and sent as `response_format`. The provider
  fails explicitly (no silent downgrade) when a requested mode/capability
  cannot be honoured.
- **LM Studio capability** confirmed via a real end-to-end run against
  `qwen/qwen3.5-9b` with `structured_output = "json_schema"`: all five
  reviewers succeeded, 23 findings opened, 20 Judge entries, 0 revisions, and
  `phobos.tex` remained byte-for-byte unchanged.
- **Ollama-compatibility contract tests** verifying the keyless local path.

### Fixed

- LM Studio E2E where all reviewers returned `REVIEWER_OUTPUT_INVALID` because
  the model produced string `"High"` for the numeric `confidence` and LM Studio
  rejects `{"type":"json_object"}` (it requires `json_schema`).

## [0.7.1] — 2026-08-26

### Fixed

- Config parsing: the documented `[providers.openai-compatible]` (hyphenated)
  TOML key is now honored (mapped to the `openai_compatible` Rust field) via
  `rename_all = "kebab-case"` on the providers config.

## [0.7.0] — 2026-08-25

### Added

- M6: Cross-platform release & Windows team client (`paper-guard info` /
  `diagnostics`, native Windows/CI release artifacts, cross-platform paths).

## [0.6.0] — 2026-08-24

### Added

- M5.1: Optional LAN service discovery (mDNS/DNS-SD) with `paper-guard discover`
  and `[discovery]` config. Discovery is off by default and never uploads a
  manuscript.

## [0.4.0] — 2026-08-22

### Added

- M4: Review Memory & team learning foundation (approval-gated retrieval,
  storage backends, Qdrant).
