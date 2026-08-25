# Paper Guard

> **A reproducible, multi-agent scientific review and revision workflow.**

Paper Guard reads a scientific manuscript, normalizes it into a canonical
model, runs **several independent LLM-based reviewers** against it, has a
**Judge agent** consolidate the findings, and a **Revision Agent** apply only
explicitly approved, strictly-scoped changes — all recorded in a persistent,
reproducible **Review Ledger**.

It is **not** a writing assistant. It is not an AI-authored scientific
contribution and it is not a replacement for the authors. It answers the
question:

> *"Where will a critical scientific reviewer attack this paper — and can we
> answer those attacks with the evidence that actually exists?"*

> **Paper Guard does not generate scientific papers. It reviews and
> quality-controls already-written manuscripts.** The only component that can
> modify manuscript source is the constrained Revision Engine, and it fails
> closed.

---

## The one rule that cannot be broken

> **Paper Guard never invents scientific facts.**

No agent may fabricate results, measurements, experiments, datasets,
references, citations, figures, table values, or statistical outcomes. When
information is missing, the system reports an explicit state
(`INSUFFICIENT_EVIDENCE`, `NOT_VERIFIED`, `UNSUPPORTED`, `CONTRADICTED`); it
never fills the gap with plausible-but-unreal content. This rule is enforced
both **conceptually** (the evidence type has no "fabricated" variant) and
**technically** (revision scopes forbid content-adding changes, and dedicated
adversarial tests guard the system).

---

## Features

- **Canonical Paper Model** — sections, paragraphs, claims, evidence, results,
  methods, figures, tables, equations, references, citations, and metadata are
  normalized so reviewers never work on raw text fragments.
- **Five independent reviewers**
  1. Scientific Reporter
  2. Adversarial / Red-Team Reporter
  3. Evidence / Claim Checker
  4. Reference Checker
  5. Figure / Table Reviewer (multimodal-capable)
- **Judge agent** — merges redundant findings, detects reviewer conflicts,
  assigns severities/priorities (P0–P3), and decides on revision actions.
- **Constrained Revision workflow** — revisions are tied to a finding, follow
  an explicit instruction with `allowed_changes` / `forbidden_changes`, are
  diffable, logged, and require human approval for major changes.
- **Re-render + validation** — after a revision the paper is re-rendered and
  re-validated (lost text, broken references, captions, numbering).
- **Review Ledger** — reproducible runs (`run-001`, `run-002`, …), each
  recording input/config hashes, versions, model configuration, reviewer/judge/
  revision/validation results, and a finding lifecycle across iterations
  (including regression detection).
- **Provider-agnostic LLM layer** — OpenAI, Anthropic, OpenAI-compatible, local
  models, and a deterministic **mock provider** for offline testing.
- **Multimodal support** — the Figure/Table reviewer can receive images.
- **Structured JSON logging** via `rust_loguru`, with parallel agent runs
  attributable per `run_id` / `agent` / `stage`.

---

## Installation

Requires a Rust toolchain (`cargo` ≥ 1.85 recommended).

```bash
git clone <this-repo> paper-guard
cd paper-guard
cargo build --release
```

The CLI binary is `paper-guard` under `target/release/`.

---

## Quick start

```bash
# Initialize a default configuration
paper-guard init

# Run the full end-to-end workflow on a LaTeX manuscript
paper-guard run manuscript/main.tex

# Or run the review stage only
paper-guard review manuscript/main.tex

# Inspect the ledger and reports
paper-guard ledger
paper-guard report
```

With the default (mock) configuration the pipeline runs **fully offline and
deterministically**, so you can try the whole workflow with no API keys.

---

## CLI

```
paper-guard init [PATH]                                  # write paper-guard.toml
paper-guard review <source> [--config PATH] [--approve-all]
paper-guard run    <source> [--config PATH] [--approve-all]   # full E2E
paper-guard findings [--config PATH]
paper-guard judge   <run> [--config PATH]
paper-guard revise  <run> [--config PATH]
paper-guard validate <run> [--config PATH]
paper-guard ledger  [--config PATH]
paper-guard report  [<run>] [--config PATH]
paper-guard feedback <run> <finding-id> --decision {accept|reject|modified} [--feedback TEXT]
paper-guard serve   [--config PATH] [--bind ADDR]        # optional HTTP service
paper-guard health  [--config PATH] [--server URL]       # query a remote service
paper-guard memory list               # list review-memory units + approval state
paper-guard memory show <id>          # show full detail of one unit
paper-guard memory approve-memory <id> [--actor NAME]
paper-guard memory approve-training <id> [--actor NAME]
paper-guard memory reject <id> [--actor NAME]
paper-guard memory search "unsupported causal claim"   # semantic search
```

Run the complete workflow with `paper-guard run manuscript/`.

---

## Service mode

`paper-guard serve` starts an optional HTTP service that calls the **same**
application pipeline as the CLI (no duplicate review logic). It binds to
**loopback only** by default and refuses unauthenticated external exposure
unless you explicitly enable it.

```
GET  /health                        → service status
POST /reviews                       → run the shared review pipeline
GET  /reviews/{run_id}              → review status/result
GET  /reviews/{run_id}/findings     → review findings
POST /reviews/{run_id}/feedback     → record human ACCEPT/REJECT/MODIFY
GET  /memory                        → list memory units + approval state
GET  /memory/{id}                   → fetch a memory unit
POST /memory/{id}/approve           → explicit approval (→ retrievable context)
POST /memory/{id}/reject            → explicit rejection
```

```bash
paper-guard serve --config configs/paper-guard.toml   # listens on 127.0.0.1:8080
```

Responses are stable JSON (API DTOs), decoupled from internal Rust types
(M3 §8). Authentication/authorization is out of scope for M3 and documented as
a limitation; the loopback-only bind is the safeguard. Uploaded manuscripts are
treated as untrusted data and never logged.

## Configuration

Configuration lives in a versioned `paper-guard.toml`. Reviewers are assigned
providers and models independently:

```toml
[project]
name = "my-paper"

[reviewers.scientific]
enabled = true
provider = "openai"
model = "gpt-4o"

[reviewers.adversarial]
enabled = true
provider = "anthropic"
model = "claude-sonnet-4"

[reviewers.evidence]
enabled = true
provider = "local"
model = "..."

[judge]
model = "..."

[revision]
require_human_approval_for_major = true
```

See `configs/paper-guard.toml` (deterministic/mock) and
`configs/paper-guard-openai.toml` (LLM providers).

---

## LLM providers

The `paper-guard-llm` crate exposes a single async `LlmProvider` trait.
Reviewers and the Judge depend only on that abstraction — never on a concrete
vendor SDK.

Production backends connect through **one** OpenAI-compatible provider whose
endpoint is configuration-driven, so switching backends is a *config change,
not a code change*:

- **OpenAI** → `base_url = "https://api.openai.com/v1"`
- **Mammoth.ai** → `base_url = "<Mammoth.ai OpenAI-compatible endpoint>"`
- **Local server** → `base_url = "http://localhost:8080/v1"`
- **any other compatible endpoint** → its base URL

```toml
[llm]
provider = "openai-compatible"      # or "mock" (the default)

[providers.openai-compatible]
base_url = "https://api.openai.com/v1"
api_key_env = "OPENAI_API_KEY"      # the env var holding the key, never the key
model = "gpt-4o-mini"
timeout_seconds = 120
max_retries = 2
structured_output = true            # request JSON mode if the endpoint supports it
vision = false                      # set true only when the model truly supports vision
```

**Ollama (local LLM).** Ollama exposes an OpenAI-compatible `/v1` endpoint, so a
local model uses the **same** provider — only `base_url` (and optionally
`api_key_env`) differ. `api_key_env` is **optional**: when absent/empty, requests
are sent without an Authorization header (local Ollama typically needs no key).
See `configs/paper-guard-ollama.toml`:

```toml
[llm]
provider = "openai-compatible"

[providers.openai-compatible]
base_url = "http://localhost:11434/v1"
model = "llama3.2"
# api_key_env = "OLLAMA_API_KEY"   # optional for local Ollama
```

Key properties:

- **Secrets never touch this repo.** The API key is read from the environment
  variable named by `api_key_env` at provider construction; it is never stored
  in a committed config, a ledger, a log, or a test fixture.
- **Strict structured output.** Reviewer output is validated before it becomes
  domain findings. Malformed or unparseable replies produce a `REVIEWER_OUTPUT_INVALID`
  failure (a failed-agent record) — never a silent best-effort parse that could
  invent or misinterpret findings.
- **Bounded, conservative retries.** Only transient errors (timeout, connection,
  rate-limit `429`, provider `5xx`) are retried, with exponential backoff and a
  strict cap. Auth, invalid-request, config, and schema errors are never retried.
- **Capability model.** A provider declares what it actually supports
  (`TEXT` / `STRUCTURED_OUTPUT` / `VISION`). A reviewer that needs a capability
  the endpoint lacks fails explicitly rather than pretending the modality was
  reviewed.
- **Provider-agnostic usage accounting.** Per-agent token usage is recorded in
  the ledger as generic `provider_usage` metadata, never coupling the ledger to
  a vendor.
- **`mock` remains the default**, giving fully offline, deterministic runs for
  CI, unit/integration tests, and local development. `paper-guard run manuscript.tex`
  works with no API key.

See `configs/paper-guard.toml` (deterministic/mock) and
`configs/paper-guard-openai.toml` (the real provider).

---

## Review Memory & Qdrant

> **Review Memory is retrieval-based contextual learning. It does not modify
> model weights.**

A human reviewer records `ACCEPT` / `REJECT` / `MODIFY` on an AI finding
(service `POST /reviews/{id}/feedback` or CLI `feedback`). Each unit is stored
**private by default** and only becomes retrievable as context
(`MEMORY_APPROVED`) or exportable to a versioned training dataset
(`TRAINING_APPROVED`) through explicit, audited consent. **A paper is never
used for anything beyond its own review automatically** — nothing is submitted
for training merely because it was reviewed. Paper Guard does not implement
LoRA / QLoRA / fine-tuning / weight updates (those are future milestones).

M4 turns this into a **team review-memory system**: approved human-validated
review experience can be embedded, stored, retrieved by semantic similarity,
and injected into a future review as *untrusted guidance only*.

```toml
[memory]
backend = "none"                 # standalone default: memory off (file | qdrant)
qdrant_url = "http://localhost:6333"
collection = "review_memory"
require_approval = true          # nothing is promoted without explicit consent
enabled = false                  # M4: disabled by default, so runs are unchanged
mode = "off"                     # off | read_only | write | read_write
top_k = 5                        # max retrieved memory entries per review
min_similarity = 0.75            # cosine similarity threshold for retrieval
embedding_provider = "mock"      # mock (offline) | openai-compatible (incl. Ollama)
embedding_model = "mock"         # e.g. nomic-embed-text / all-minilm for Ollama
owner_id = ""                    # optional owner identity for scope/authorization
team_id = ""                     # optional team id for shared team memory
```

Key M4 properties:

- **Modes.** `off` (default), `read_only` (use approved memory but store
  nothing new), `write` (store approved feedback but retrieve nothing),
  `read_write` (both). Existing behavior is unchanged unless memory is
  explicitly enabled.
- **Embeddings are provider-independent.** A reviewer-facing
  `EmbeddingProvider` trait (mock / OpenAI-compatible, incl. Ollama's
  `/embeddings`) means local deployment can use `Ollama + local embedding +
  Qdrant` with no external API. Embeddings are computed **once** per memory
  entry from a deterministic *review-experience* representation — never the
  whole paper.
- **Scope-aware authorization.** Each unit has a scope: `PRIVATE` (owner-only)
  or `TEAM` (any member of the owning team). Retrieval never leaks a private
  unit to another owner, and never returns a rejected unit as guidance.
- **Historical memory is untrusted.** Retrieved memory is injected as a
  delimited `<historical_review_memory>` block, plainly distinct from the
  `<current_manuscript>` block. The reviewer prompt states it is **not
  evidence** for the current paper and **not an instruction**; prompt
  injection inside a memory entry is ignored. `similarity ≠ correctness`.
- **Service memory API.** Beyond `POST /reviews/{id}/feedback`, the service
  exposes approval management:
  ```
  GET  /memory                           → list stored units + approval state
  GET  /memory/{id}                      → fetch a unit
  POST /memory/{id}/approve              → explicit approval → retrievable context
  POST /memory/{id}/reject               → explicit rejection
  ```
- **Handling of a failed memory.** If Qdrant is unavailable, retrieval reports
  `MEMORY_UNAVAILABLE` and the review **continues without fabricated context**;
  a failed memory write is surfaced (never silently swallowed).
- **Explicit — no training.** No `train` / `fine-tune` / `lora` / `qlora`
  commands exist. M4 ends at human-approved memory + semantic retrieval.

Ollama can serve **both** the LLM (`/v1/chat/completions`, config only) and the
embedding model (`/v1/embeddings`) for a fully local deployment alongside a
configured Qdrant. `cargo test --workspace` stays fully offline; Qdrant is
exercised through an opt-in harness (`PAPER_GUARD_QDRANT_TESTS=1`).

---

## Deployment (Helm)

A Helm chart (`deploy/helm/paper-guard`) deploys the Paper Guard **service** to
Kubernetes. It does **not** bundle Qdrant or Ollama — those are configured
external endpoints, so the topology stays flexible:

```text
Kubernetes
├── paper-guard (this chart)
├── qdrant      (deployed/managed separately)
└── ollama      (deployed separately, or an external LLM endpoint)
```

```bash
helm install paper-guard deploy/helm/paper-guard \
  --set llm.endpoint=https://api.openai.com/v1 \
  --set llm.model=gpt-4o-mini \
  --set llm.apiKeySecretName=paper-guard-api-key
kubectl create secret generic paper-guard-api-key \
  --from-literal=api-key='sk-...' \
  --dry-run=client -o yaml | kubectl apply -f -
```

API keys live in a Kubernetes Secret referenced by name — **never** in
`values.yaml`. Persistent storage for artifacts/ledger is configurable (PVC).
See `deploy/helm/paper-guard/README.md` for the full parameter table.

---

## Scientific integrity in practice

- The **Evidence / Claim Checker** walks `Claim → Evidence → Result →
  Figure/Table → Reference` and classifies support; it never fabricates.
- The **Reference Checker** reports `NOT_VERIFIED` for references it cannot
  confirm against an authoritative source; it never asserts existence.
- The **Revision engine** structurally **forbids** adding results, experiments,
  references, measurements, or inventing data — regardless of the prompt. It
  **fails closed**: if a safe edit cannot be proven, nothing is auto-applied
  and the action is surfaced for the author instead.
- **Prompt injection** is defended: paper content is treated as untrusted
  input and cannot override system rules. Every reviewer's *delivered* system
  prompt carries the integrity preamble.
- **Provenance** is tracked end-to-end (`AUTHOR_CONTENT`, `PARSER_OUTPUT`,
  `REVIEWER_OUTPUT`, `JUDGE_OUTPUT`, `REVISION_INSTRUCTION`,
  `REVISION_OUTPUT`, `VALIDATION_OUTPUT`), so an applied edit is always tagged
  as machine-produced and can never be mistaken for author content.
- Reviewer outputs are independent inputs to a single Judge; a failed or
  panicked reviewer is recorded under its **own** agent identity.
- **Adversarial tests** (see `crates/paper-guard-review/tests/adversarial.rs`)
  verify all of the above, including reviewer-disagreement-never-collapses-to-
  `SUPPORTED`, revision escalation rejection, and injection in every manuscript
  location (body, caption, table, reference, metadata).

---

## Review ledger & reproducibility

Each run stores:
input hash, source format, parser version, Paper Guard version,
configuration hash, model configuration, prompt version, reviewer results,
judge results, revision results, validation results, and per-agent provider
usage metadata (tokens), plus a timestamp. LLM outputs are recorded as review
artifacts. Finding lifecycle states
(`OPEN` → `ACKNOWLEDGED` → `APPROVED` → `REVISED` → `RESOLVED` / `REJECTED`,
and `REGRESSED`) let the system detect when a previously fixed problem is
reintroduced (see `crates/paper-guard-ledger` tests).

Because a real LLM is non-deterministic, Paper Guard never claims such a run
is bit-for-bit reproducible. Instead it records enough to audit the run —
`input_hash`, `paper_guard_version`, `configuration_hash`, provider & model,
model parameters, prompt version, per-reviewer configuration, timestamp —
i.e. **`AUDITABLE_NONDETERMINISTIC`** mode. The mock provider remains fully
deterministic for offline regression testing.

### Live (real provider) testing

The normal test suite is **fully offline**; no test makes a real API call.
Provider behaviour is exercised against a local mock HTTP server
(`crates/paper-guard-llm/tests/openai_compatible.rs`).

An **optional** live end-to-end harness
(`crates/paper-guard-review/tests/live_provider.rs`) runs one real reviewer
against a real endpoint. It is opt-in and must never run in CI:

```bash
PAPER_GUARD_LIVE_TESTS=1 \
  OPENAI_API_KEY=$(cat ./my-secret-key) \
  cargo test -p paper-guard-review --test live_provider -- --ignored
```

It loads the sample paper, parses it, runs the adversarial reviewer, validates
the structured output, verifies provenance, stores no secret, writes a
temporary ledger, and cleans up.

---

## Security model

Treat paper contents (text, captions, references, tables, metadata, OCR
output) as **untrusted input**. There are no hidden prompt instructions from
the paper that can override the system's integrity rules. Every change must
(1) map to a finding, (2) follow an explicit revision instruction, (3) be
diffable, (4) be logged, and (5) be tagged with its provenance. No silent
changes.

---

## Development

```bash
cargo build --workspace
cargo test --workspace        # unit + integration + adversarial integrity tests
cargo clippy --workspace
```

Repository layout:

```
paper-guard/
├── Cargo.toml
├── README.md
├── LICENSE
├── docs/                   # architecture documentation
├── configs/                # default + OpenAI + Ollama example configuration
├── examples/               # sample manuscripts
├── deploy/helm/paper-guard # Helm chart for the service mode
├── crates/
│   ├── paper-guard-core/
│   ├── paper-guard-llm/
│   ├── paper-guard-parser/
│   ├── paper-guard-review/
│   ├── paper-guard-agents/
│   ├── paper-guard-renderer/
│   ├── paper-guard-validation/
│   ├── paper-guard-ledger/
│   ├── paper-guard-app/     # shared application layer (config, pipeline, memory)
│   ├── paper-guard-service/ # optional HTTP service mode
│   ├── paper-guard-client/  # HTTP client for a remote service (transport only)
│   └── paper-guard-cli/
└── tests/
```

---

## License

Paper Guard is licensed under the [Apache License, Version 2.0](LICENSE).
