# Changelog

All notable changes to Paper Guard are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/) and this project adheres to
[Semantic Versioning](https://semver.org/).

## [1.1.3] — 2026-09-03

### Fixed
- **Deterministic bibliography cache eviction**: the bounded on-disk cache
  evicted by filesystem mtime (truncated to seconds), falling back to
  filesystem enumeration order on ties — so which entry was evicted could
  differ between local machines and CI. Eviction now uses the canonical
  nanosecond `stored_at` insertion timestamp with a deterministic filename
  tie-break, and is platform-independent.
- **Windows x86_64 release build**: `resolve_platform_editor` on Windows now
  probes for `notepad.exe` via `command_on_path`, fixing the `-D warnings`
  dead-code failure that blocked the v1.1.1 release pipeline.

## [1.1.0] — 2026-09-03

### Added
- **Logo integration**: Added official Paper Guard logo to docs/logo.png and integrated into GUI and documentation
- **External Prompt System**: 
  - `paper-guard prompts init` command to initialize external prompts directory
  - `paper-guard prompts list` command to list available external prompts
  - Support for loading prompts from `~/.paper-guard/config/prompts/`
  - Prompt hashing in ledger for integrity verification (no prompt content stored)
- **User Configuration**:
  - `paper-guard setup` command to initialize user configuration
  - `paper-guard config show` to display current configuration
  - `paper-guard config edit` to edit user configuration
  - Configuration stored in `~/.paper-guard/config/config.toml`
  - Prompts stored in `~/.paper-guard/config/prompts/`
- **Rolling File Logger**:
  - Log files stored in `~/.paper-guard/logs/`
  - 10 MiB per file, maximum 5 old files plus current
  - JSON Lines format with secret scrubbing
  - Human-readable console output preserved
- **Enhanced Bibliography System**:
  - `paper-guard bibliography` command
  - arXiv provider for fetching bibliographic metadata
  - Mock provider for testing
  - Google Scholar as honest `unavailable` slot (no scraping)
  - Local caching of bibliography results
  - Metadata verification without automatic correction
- **Review System Improvements**:
  - Five reviewer types: scientific, adversarial, evidence, references, figures
  - Judge system for consolidating reviewer feedback
  - Evidence system with domain validation
  - Revision engine with approval gating
  - Ledger for tracking review runs
  - Claims and findings tracking
- **Local LLM Support**:
  - OpenAI-compatible provider interface
  - LM Studio integration with structured output
  - Ollama-compatible OpenAI API support
  - `structured_output` support for `json_object` and `json_schema`
  - Strict JSON Schema validation with no fallback on invalid schema
  - Reviewer-side domain validation
- **Report Output Formats**:
  - Human-readable output (default)
  - JSON output (--output json)
  - Summary output (--output summary)
  - Three presentation styles: neutral, funny, insulting
  - Insulting style never attacks author personally
- **Input Format Support**:
  - `.tex` files as primary input
  - `\\input{...}` and `\\include{...}` directive support
  - Support for nested includes
  - Relative path resolution within paper context
  - Cycle detection and duplicate inclusion prevention
  - Clean error messages for missing files and path traversal attempts
- **PDF Support**:
  - PDF manuscript recognition and text extraction
  - Per-page provenance tracking
  - Handling of malformed, encrypted, and text-unavailable PDFs
- **GUI**:
  - `paper-guard --gui` launches local web interface
  - Dashboard showing version/provider/config/recent runs
  - Review interface for starting reviews and watching progress
  - Results view with findings, filters, and style switching
  - JSON export of canonical RunRecord
  - Security boundaries preventing bypass of validation

### Security & Integrity
- Input manuscript (LaTeX/PDF) remains byte-identical before and after review
- Presentation styles never alter:
  - Finding IDs, severity, confidence, evidence, claims, categories
  - Judge results or revision scope
- External prompts can modify reviewer instructions but cannot:
  - Remove integrity preamble
  - Alter wrapper or arrangement
  - Bypass domain validation
- Bibliographic metadata from external sources treated as untrusted
- No revision without explicit human approval (`--approve-all` not used in release testing)
- Secret scrubbing in logs prevents credential leakage
- Path traversal blocks prevent access outside paper context
- Response limits and timeouts on all network requests

### Changed
- Updated all workspace crate versions from 1.0.0 to 1.1.0
- Updated Cargo.lock dependencies to latest compatible versions
- Improved cross-platform path normalization in LaTeX parser
- Enhanced Windows support in CI/CD pipeline
- Updated documentation to reflect actual implemented features
- Refactored internal module organization for better maintainability

### Added

- **LaTeX project support** (`\input` / `\include`) with deterministic,
  document-order resolution. Nested includes, extensionless references,
  Unicode filenames, and paths containing spaces are supported. Cycles
  (`LATEX_INCLUDE_CYCLE`) and missing files (`LATEX_INCLUDE_NOT_FOUND`) are
  reported as deterministic diagnostics. Path traversal and symlink escape
  outside the project root are blocked (fail closed). Source content is cached
  within a review run and include depth/fragment-count caps prevent
  exponential include graphs. Resolved relative paths and per-paragraph
  provenance are normalized to forward slashes so persisted run records are
  identical across POSIX and Windows.
- **PDF manuscripts** as a first-class review source. In-process text
  extraction via `lopdf` (no OCR, no embedded content execution, no shell)
  with per-page provenance. Malformed PDFs fail with `PDF_INVALID`; encrypted
  PDFs with `PDF_ENCRYPTED`; image-only / no-text PDFs with
  `PDF_TEXT_UNAVAILABLE`. Extraction is bounded per page.
- **`paper-guard --gui`** — a small local web GUI (presentation/control layer
  only) that binds to `127.0.0.1` by default, selects an available local port,
  prints the URL, and optionally opens the default browser. Views: Dashboard
  (version/provider/config/recent runs), Review (select `.tex`/`.pdf`, start a
  review, watch the five reviewers + Judge), Results (findings + filters +
  human-readable report + style switch), and JSON export of the canonical
  `RunRecord`. The GUI reuses the existing service/API/domain layers exactly;
  no second review engine and no duplicated domain logic.
- **`paper-guard inspect`** — report how a source document resolves (LaTeX
  project includes, PDF page counts, missing/cyclic diagnostics) without
  running a review. Never modifies the source.
- **Cross-platform release matrix** now includes **macOS x86_64** in addition
  to macOS ARM64, Linux ARM64, Linux x86_64, and Windows x86_64. Archives are
  ZIP files named by the Rust target triple:
  `paper-guard-1.0.0-aarch64-apple-darwin.zip`,
  `-x86_64-apple-darwin.zip`, `-aarch64-unknown-linux-gnu.zip`,
  `-x86_64-unknown-linux-gnu.zip`, `-x86_64-pc-windows-msvc.zip`.
  `SHA256SUMS` is generated and verified before publishing.
- **Release pipeline** — a reusable `.github/workflows/release-core.yml` is
  invoked by both the automatic tag trigger
  (`.github/workflows/release.yml`, `on: push: tags: ['v*']`) and a manual
  historical backfill workflow (`.github/workflows/release-manual.yml`,
  `workflow_dispatch`). Every job checks out and verifies the exact tagged
  commit (never `main`), runs validation + `cargo audit`, the five native
  platform builds, binary smoke tests, the Trivy security gate, and generates
  + verifies `SHA256SUMS` before creating the GitHub Release. Binary build
  provenance (`PAPER_GUARD_BUILD_COMMIT`) is embedded and surfaced by
  `paper-guard info` / `diagnostics`.
- **GUI+API tests** — startup, localhost binding, API availability, review
  creation, style switching, JSON export, security boundary checks.

### Security & integrity

- LaTeX parsing never executes LaTeX (`pdflatex`, `xelatex`, `lualatex`,
  BibTeX, Biber, `\write18`, shell escape, Makefile) — the parser only reads
  source files as untrusted text.
- PDF extraction never executes embedded content; figure interpretation is
  only claimed when the text layer actually contains it.
- The GUI cannot bypass reviewer validation, Judge validation, evidence
  isolation, authorization boundaries, revision approval, or ledger rules; all
  mutations flow through the same domain/API boundaries as the CLI.
- Presentation styles (`neutral`/`funny`/`insulting`) in the GUI are
  deterministic, client-selected renderings of the canonical `RunRecord`; a
  style switch never triggers an LLM request and never alters finding content.

### Changed

- Release archives are now **ZIP files named by the Rust target triple**
  (e.g. `paper-guard-1.0.0-linux-x86_64.zip` becomes
  `paper-guard-1.0.0-x86_64-unknown-linux-gnu.zip`), making the intended
  platform unambiguous. The GitHub Actions release pipeline is refactored into
  a reusable `release-core.yml` plus automatic (`release.yml`) and manual
  historical-backfill (`release-manual.yml`) entry points.

### Configuration Wizard (planned for v1.1)

- Documented the planned `paper-guard --wizard` interactive configuration
  flow. Not implemented in v1.0.

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
