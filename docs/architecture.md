# Paper Guard — Architecture

This document captures the key architecture decisions and the design of the
core domain model, the pipeline, and the scientific-integrity guarantees.

## 1. Guiding requirements

Paper Guard is **not** a generic writing assistant. It is a reproducible,
multi-agent scientific **review and revision workflow**. Its most important
rule:

> Paper Guard must never invent scientific facts.

Every architectural decision below is chosen to serve that rule, plus
reproducibility, traceability, and provider independence.

## 2. Design decisions

### 2.1 Workspace layout (crates)

The system is split into focused crates so that no layer depends on the rest:

| Crate                | Responsibility                                             |
|----------------------|------------------------------------------------------------|
| `paper-guard-core`   | Canonical paper model, findings, integrity domain, ledger types. Contains **no** I/O, LLM, or CLI code. |
| `paper-guard-llm`    | `LlmProvider` trait + deterministic `MockProvider`.         |
| `paper-guard-parser` | `Parser` trait + a LaTeX parser producing the canonical model. |
| `paper-guard-review` | The five reviewers + the runner (parallel) + the judge.    |
| `paper-guard-agents` | The revision engine (strictly scoped, auditable edits).    |
| `paper-guard-renderer` | Emits a source representation from the canonical model.   |
| `paper-guard-validation` | Text/structural validation after re-rendering.          |
| `paper-guard-ledger` | Persistent review ledger and run tracking.                 |
| `paper-guard-cli`    | Command-line interface orchestrating the pipeline.         |

### 2.2 Canonical Paper Model (in `paper-guard-core`)

Reviewers never operate on raw PDF text. The parser normalizes a document into
a canonical model with typed entities: `Document`, `Section`, `Paragraph`,
`Claim`, `Evidence`, `Result`, `Method`, `Figure`, `Table`, `Equation`,
`Reference`, `Citation`, `Revision`. Claims are uniquely addressable
(`claim_id`, `location`, `text`, `type`, `confidence`, links to evidence,
results, and citations). This is the foundation for the Evidence / Claim
checker.

### 2.3 Scientific-integrity domain (in `paper-guard-core::integrity`)

Two ideas make "never invent" more than a policy:

1. **Structural impossibility.** `EvidenceState` has *no* "fabricated"
   variant; missing evidence is `INSUFFICIENT_EVIDENCE`, and unverifiable
   references are `NOT_VERIFIED`. An agent literally cannot report a
   fabricated support state as a first-class value.
2. **Explicit guard.** `assert_not_fabricated(origin, has_real_artifacts,
   state)` rejects a `Supported` / `PartiallySupported` / `WeaklySupported`
   assertion when no real artifact backs it.

### 2.4 Provider abstraction (in `paper-guard-llm`)

Reviewers depend on `LlmProvider` (with an async `generate`), never on a
concrete vendor. The `MockProvider` returns only explicitly scripted responses
and, for unknown input, a neutral `[]` (no fabricated findings). This lets the
whole pipeline run offline and deterministically. Reviewer→model assignment is
driven by `paper-guard.toml`, so `openai`, `anthropic`, `openai-compatible`,
`local`, and `mock` are all conceptually supported.

The production backend is a single **`OpenAICompatibleProvider`**
(`crates/paper-guard-llm/src/openai_compatible.rs`), an HTTP client for the
`/chat/completions` endpoint. It is deliberately *not* tied to a vendor SDK:
the same implementation connects to OpenAI, Mammoth.ai, a local server, or any
other OpenAI-compatible endpoint purely via configuration (`base_url`, `model`,
`api_key_env`). Switching backends is a configuration change, never a code
change; the rest of the system only ever sees `LlmProvider`.

Design properties of the real provider:

- **Secrets**: the API key is read once from the environment variable named by
  `api_key_env` at construction. It is never stored in a committed config, a
  ledger entry, a log line, or a test fixture. Logging and errors never echo
  the key.
- **Capability model**: a provider declares what its endpoint/model actually
  supports (`TEXT`, `STRUCTURED_OUTPUT`, `VISION`). A reviewer that requires a
  capability the endpoint lacks fails explicitly with a capability error; it
  never silently drops a modality (e.g. it never claims to have visually
  reviewed a figure when only text was sent).
- **Structured output**: when the endpoint supports it, the provider requests
  JSON mode (`response_format: {"type":"json_object"}`). The reviewer layer
  then *validates* the reply strictly — see §2.10.
- **Bounded retries**: only transient errors (timeout, connection, `429`, `5xx`)
  are retried, with exponential backoff and a strict cap. Auth, invalid-request,
  config, and schema errors are never retried (no retry storms).
- **Usage accounting**: token usage flows from the provider response into the
  reviewer's output and then into the ledger as generic `provider_usage`
  metadata (input/output tokens + provider/model). The ledger is provider-agnostic.

### 2.5 Multi-agent review (in `paper-guard-review`)

The five independent reviewers (Scientific, Adversarial/Red-Team, Evidence,
References, Figures) share one structured finding schema. Each runs as an
independent future; a failed reviewer is recorded as a failed agent status, not
silently ignored. Reviewers run concurrently via the tokio runtime.

### 2.6 Judge (in `paper-guard-review::judge`)

The judge consolidates findings: it merges duplicates (keeping the higher
severity), detects reviewer conflicts on the same claim, assigns severities,
and maps each actionable finding to a strictly-scoped revision instruction. The
judge decides only *review and revision actions*; it never alters a scientific
claim.

### 2.7 Revision workflow (in `paper-guard-agents`)

The revision engine only applies changes inside an explicit `RevisionInstruction`
scope (`allowed_changes` / `forbidden_changes`). The integrity-forbidden
baseline (adding results, experiments, references, altering measurements,
inventing data, etc.) can never be disabled. Every change records `before`,
`after`, `reason`, `finding_id`, `revision_id`, `agent`, `timestamp`, and a
`provenance` tag so a machine-produced edit can never be mistaken for author
content.

The engine **fails closed**. If an instruction asks for an automatic edit but no
deterministically-safe transformation can be found (e.g. the target text or a
numeric overstatement is absent), the engine does **not** claim success. It
returns `applied: false` with a `rejected` reason that surfaces as an
author-facing action rather than an applied revision. The `resulting_hash` is
computed over the *changed* source, so the recorded hash always reflects what a
revision actually produced.

Every message is classified into a conservative [`RevisionCategory`]: safe
presentation changes, evidence-preserving changes, scientist-content changes
(requiring human review), and `NEW_SCIENTIFIC_CONTENT` (always rejected).

The pipeline then re-renders and re-validates the document, and the new state
is recorded as a new run in the ledger.

### 2.8 Provenance (in `paper-guard-core::provenance`)

Every piece of content the system touches carries an unambiguous origin. The
`Provenance` type distinguishes `AUTHOR_CONTENT`, `PARSER_OUTPUT`,
`REVIEWER_OUTPUT`, `JUDGE_OUTPUT`, `REVISION_INSTRUCTION`, `REVISION_OUTPUT`,
and `VALIDATION_OUTPUT`. Applied revision edits are tagged
`REVISION_OUTPUT` (never author content), so it is impossible for an LLM- or
machine-produced statement to be represented as author-supplied content.

### 2.9 Ledger & reproducibility (in `paper-guard-ledger`)

Every run stores input hash, configuration hash, parser/version, model
configuration, reviewer + judge + revision + validation results, and a
timestamp. Findings are tracked through a lifecycle (`OPEN`, `ACKNOWLEDGED`,
`APPROVED`, `REVISED`, `RESOLVED`, `REJECTED`, `REGRESSED`), and the system can
detect a previously resolved problem reintroduced by a later revision. For a
real (non-deterministic) provider, the run is recorded as
`AUDITABLE_NONDETERMINISTIC`: it captures everything needed to audit the run but
never claims bit-for-bit reproducibility.

### 2.10 Strict structured reviewer output (`REVIEWER_OUTPUT_INVALID`)

A reviewer's reply is only accepted if it is a well-formed findings
array/object in the shared schema. `resolve_findings` rejects, as
`REVIEWER_OUTPUT_INVALID`, any reply that is empty, unparseable JSON, a
non-array/object value, or a finding that fails schema/domain validation. This
is fail-closed: malformed (or partially-fabricated) LLM output becomes a
**failed-agent record** in the ledger, never a silent best-effort parse that
might invent or misinterpret findings. An explicit empty array `[]` remains a
legitimate "no findings" reply, and its structured output is preserved.

## 3. Pipeline

```
source (pdf/latex/typst/docx)
   │  parse
   ▼
canonical Document
   │  review (scientific, adversarial, evidence, references, figures — parallel)
   ▼
findings (JSON schema)
   │  judge
   ▼
revision instructions (+ human approval for major)
   │  revision engine (strictly scoped)
   ▼
new source → re-render → validation
   ▼
ledger run (reproducible artifact)
```

## 4. Security model

Paper content (text, captions, references, tables, metadata, OCR) is treated as
**untrusted input**. No hidden prompt instruction from the paper can override
system rules. Every reviewer's system prompt carries an explicit integrity
preamble *in the delivered prompt* (not merely in documentation) that
instructs the model to treat paper content as data, to never fabricate facts,
and to report `NOT_VERIFIED` / `INSUFFICIENT_EVIDENCE` rather than invent
support. The preamble is inserted before the task-specific instructions so it
remains authoritative even if paper content attempts to override it.

Reviewer outputs are independent inputs to a single Judge; reviewers never see
one another's output, share no mutable review state, and are not
order-dependent. A failed reviewer is recorded under its own agent identity (a
panic is never mis-attributed to another agent), so reviewer independence and
the per-agent audit trail are preserved.

## 5. Integrity test coverage

The `tests/` and `crates/*/tests/` suites include adversarial tests that
verify: invented references stay `NOT_VERIFIED`; fabricated support is
rejected; contradictory reviewers are surfaced as conflicts; prompt injection
is treated as untrusted input (in a paragraph, figure caption, table,
reference, and metadata); damaged PDFs fail cleanly; out-of-scope revisions
are forbidden; reviewer disagreement never collapses into `SUPPORTED`;
malicious reviewer recommendations cannot drive the engine to add
results/experiments/references; revision escalation is rejected (the engine
fails closed); a revision's textual change is actually reflected in the source
and tagged as machine-produced; and the ledger detects `REGRESSED` findings
when a previously resolved problem is reintroduced. The provider layer is
covered by offline contract tests (`crates/paper-guard-llm/tests/openai_compatible.rs`)
that verify request construction, auth/config errors, malformed-response
rejection (`REVIEWER_OUTPUT_INVALID`), transient-vs-permanent retry behaviour,
capability gating, and the absence of secrets in logs/config; an opt-in live
harness (`crates/paper-guard-review/tests/live_provider.rs`) exercises one real
reviewer against a real endpoint without ever running in CI.
