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
| `paper-guard-llm`    | `LlmProvider` trait + deterministic `MockProvider` + the real `OpenAICompatibleProvider`. |
| `paper-guard-parser` | `Parser` trait + a LaTeX parser producing the canonical model. |
| `paper-guard-review` | The five reviewers + the runner (parallel) + the judge.    |
| `paper-guard-agents` | The revision engine (strictly scoped, auditable edits).    |
| `paper-guard-renderer` | Emits a source representation from the canonical model.   |
| `paper-guard-validation` | Text/structural validation after re-rendering.          |
| `paper-guard-ledger` | Persistent review ledger and run tracking.                 |
| `paper-guard-app`    | **Shared application layer**: config, pipeline orchestration, review memory. Used by both the CLI and the HTTP service. |
| `paper-guard-service`| **Optional HTTP service mode**: minimal REST API over the shared application layer. |
| `paper-guard-client` | **HTTP client for a remote service** (M3.5). Transport only; maps wire DTOs onto the same domain representations. Never instantiates the pipeline. |
| `paper-guard-cli`    | Command-line interface (thin shell over `paper-guard-app`). |

### 2.1b Standalone vs Service

Both entry points drive the **same** application layer (`paper-guard-app`):
the CLI is a thin shell, and the HTTP service is a thin API — neither
re-implements review logic.

```text
  CLI ────────────────┐
                      ▼
                 paper-guard-app   (config, pipeline, review, judge, ledger, memory)
                      ▲
  HTTP API ───────────┘
```

**Standalone** (`paper-guard run paper.tex`): minimal dependencies; works fully
offline with the `MockProvider`, and can use any OpenAI-compatible endpoint
(OpenAI, Mammoth.ai, a local Ollama `/v1`, ...) purely via configuration.

**Service** (`paper-guard serve`): a minimal HTTP API (`GET /health`,
`POST /reviews`, `GET /reviews/{id}`, `GET /reviews/{id}/findings`,
`POST /reviews/{id}/feedback`) that calls the same pipeline. It binds to
loopback by default and refuses unauthenticated external exposure unless
explicitly enabled (M3 §9); authentication/authorization is documented as
Persistence for artifacts/ledger is filesystem-backed and
survives restarts via the Helm PVC.

### 2.1c Local vs remote (client) mode

The `paper-guard` binary is both a standalone application and a client for a
remotely-deployed service. The same binary supports either mode without
changing the scientific review architecture:

```text
                    paper-guard binary
                           │
                 ┌─────────┴─────────┐
                 │                   │
             Local Mode         Remote Mode
                 │                   │
                 ▼                   ▼
          local app layer      paper-guard-client (HTTP only)
                 │                   │
                 │                   ▼
                 │            Paper Guard Service
                 │                   │
                 └─────────┬─────────┘
                           ▼
                      Review Result
```

**Mode resolution** (M3.5 §6): an explicit `--server` flag always wins; a
configured `[server].url` is next; otherwise execution is local. The CLI never
switches to remote implicitly from an environment variable. The selected mode
is always reported (e.g. `Mode: remote / Server: http://…`).

**Same domain results** (M3.5 §7): local and remote execution map onto the same
application-level representation (`RunOutput`, `FindingPayload`, `RunStatus`).
`paper-guard-client` exposes typed methods (`health`, `submit_review`,
`get_review`, `get_findings`, `submit_feedback`) and a typed error taxonomy
(connection, timeout, HTTP, auth, invalid response, server-side review failure,
serialization) so the CLI never receives generic string errors (M3.5 §8).

**Remote ledger** (M3.5 §13): the server is authoritative for a remote run. The
client uploads the manuscript content (base64 `content_base64`) and never
writes a second local ledger entry pretending it executed the review.

**Security** (M3.5 §15–16): production deployments should use HTTPS (the
reverse proxy/ingress terminates TLS). The client resolves a bearer token from
the configured `auth_token_env` at request time and never stores or logs it;
manuscript content is never cached or logged.

### 2.1d LAN discovery (M5.1)

Paper Guard has provider-independent, **optional** LAN service discovery so a
service in a local cluster can be found by machines on the same LAN without
manually entering a node IP, Service/NodePort, or Ingress address. Discovery is
a third, explicitly-opted-in path in addition to local and explicit-remote:

```text
no explicit server
        │
        ▼
  optional discovery
        │
   ┌────┴────┐
   │         │
 found     not found
   │         │
   ▼         ▼
 remote     local
```

**Abstraction.** The client depends only on the `ServiceDiscovery` trait and the
[`ServiceEndpoint`] model in `paper-guard-discovery`. mDNS/DNS-SD (via the
`mdns-sd` crate) is one pluggable backend; a deterministic
`MockServiceDiscovery` covers tests. Future mechanisms (DNS-SD, static config,
Kubernetes discovery) remain replaceable. No Avahi/mDNS logic lives in the
Paper Guard application itself. The DNS-SD service type `_paper-guard._tcp`
uniquely identifies Paper Guard and does not collide with any registered IANA
service.

**Opt-in, never implicit.** Discovery is disabled by default (`[discovery]
enabled = false`). Unknown modes fail closed to `off`. The client never probes
the network implicitly. `paper-guard discover` performs a manual browse, lists
all candidates, verifies each through `GET /health`, and exits without
uploading anything.

**Security contract.**

> **Discovery ≠ authorization.** Finding a Paper Guard service never authorises
> an upload. Paper Guard never sends a manuscript to a discovered service unless
> remote execution has been explicitly selected.

Discovered records are **untrusted input**: they are parsed defensively and
sanitised so a hostile record cannot inject a scheme, port, path, credentials,
or cause command execution / filesystem access / secret disclosure. Candidates
are cross-checked via `/health`: a service that does not self-identify as Paper
Guard is rejected, and obvious API-version incompatibilities are surfaced as
`INCOMPATIBLE_SERVICE_VERSION` (API compatibility is preferred over binary
version equality). Multiple services are always listed; selection is never
"first response wins" and requires an explicit `preferred_service` in Auto mode.

**Deployment.** The Kubernetes/k3s chart keeps discovery config in the app's
`paper-guard.toml`, but mDNS *publishing* stays a separate, opt-in
infrastructure pod (`discovery.publisher`, default off) with its own security
context. The Paper Guard app container remains unprivileged and never requires
`hostNetwork`, `NET_ADMIN`, or `NET_RAW`; if a publisher needs those, they are
scoped to the publisher pod only.

**Networking limits.** mDNS operates within the local multicast domain and may
not cross routed networks, VPNs, VLANs, Wi-Fi client isolation, multicast-blocking
firewalls, or CNI boundaries. Paper Guard fails gracefully (empty result, not an
error) when mDNS is unavailable.

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


### 2.11 Local LLM (Ollama)

Ollama exposes an OpenAI-compatible `/v1` endpoint, so Paper Guard reaches a
local model through the **same** `OpenAICompatibleProvider` as OpenAI and
Mammoth.ai — there is no separate Ollama provider. Switching to a local model
is a configuration change only ([`configs/paper-guard-ollama.toml`](../../configs/paper-guard-ollama.toml)):

```toml
[llm]
provider = "openai-compatible"

[providers.openai-compatible]
base_url = "http://localhost:11434/v1"
model = "llama3.2"
# api_key_env is OPTIONAL for local Ollama; when absent/empty requests are
# sent without an Authorization header.
```

Using a local model does **not** mean papers are used for training. A local
Ollama run is a regular review; it changes nothing about privacy or training
(see §2.13).

### 2.12 Review Memory (retrieval-based learning, in `paper-guard-app::memory`)

Review Memory is the foundation for the learning architecture. M4 turns it into
a **team review-memory system**: human-approved review experience can be
embedded, stored, retrieved by semantic similarity, and injected into a future
review as *untrusted guidance*. It is:

- **Separate from the LLM provider.**
- **Retrieval-based, not model-weight training.** No fine-tuning/LoRA/QLoRA in
  M4 (explicitly out of scope).
- **Stored in meaningful units** (claim/method/figure/reference) with
  provenance, scope, and a `schema_version`, never whole papers, in a dedicated
  repository (`ReviewMemoryRepository`). Backends: `FileReviewMemory` (offline
  JSON, default) and `QdrantReviewMemory` (vector backend; see §2.14).

**Memory model.** A `ReviewMemoryEntry` carries `memory_id`, `source_run_id`,
`source_finding_id`, `reviewer_kind`, `category`, claim/evidence context,
`finding`, `resolution` (ACCEPT/REJECT/MODIFY), `human_feedback`, `provenance`,
`privacy_state`, `scope` (`PRIVATE`/`TEAM`), `owner_id`/`team_id`, an
`embedding`, `created_at`, and `schema_version`. The strongest learning signal
is human feedback; the original LLM finding and the human feedback are kept as
separate provenance-bearing objects (never merged into "truth").

**Memory modes** (`[memory] mode`): `off` (default), `read_only` (use approved
memory, store nothing new), `write` (store approved feedback, retrieve
nothing), `read_write` (both). Memory is **disabled by default** (`enabled =
false`), so existing behaviour is unchanged unless explicitly enabled.

**Embeddings are provider-independent** (§14–§16): a reviewer-facing
`EmbeddingProvider` trait and a deterministic `MockEmbeddingProvider` (offline)
plus an `OpenAICompatibleEmbeddingProvider` covering Ollama's `/embeddings`.
Each memory entry is embedded **once** from a deterministic
*review-experience* representation (category, claim/evidence context, finding,
decision, feedback) — never the whole paper and never raw manuscript text.

### 2.13 Privacy and training consent

Papers may contain unpublished, confidential, proprietary, or personal research.
Therefore **nothing is ever used for training automatically**:

- Every memory candidate starts in `PRIVATE` (the default).
- `PRIVATE` units cannot be retrieved as context or exported.
- `MEMORY_APPROVED` units may be retrieved as context.
- `TRAINING_APPROVED` units may be exported to a versioned, human-approved
  dataset. A paper is never used for anything beyond its own review merely
  because it was reviewed.
- `REJECTED` units are removed from retrieval/export eligibility (audited).

**Scope-aware authorization** (M4 §11–12, 18): each unit has a scope.
`PRIVATE` memory is visible only to its owner; `TEAM` memory is visible to any
member of the owning team. Retrieval always enforces the intersection of
(approval state) AND (scope grants access), so a private unit belonging to one
user is never leaked to another, and a rejected unit is never presented as
positive review guidance.

Retrieved memory is **historical review experience**, never current-paper
evidence. A memory entry saying "this claim is supported" does **not** prove the
current claim is supported. Retrieved context is injected as a delimited
`<historical_review_memory>` block that is plainly distinct from the current
document and is **untrusted**: it is not an instruction and not evidence for
the current manuscript (M4 §21–22). Prompt injection inside a memory entry
cannot override the reviewer system prompt.

### 2.14 Qdrant + deployment

Qdrant is the vector store for Review Memory and is **optional**:
- **Standalone** never requires Qdrant (default `[memory] backend = "none"`).
- **Service** can use a configured Qdrant for vector retrieval; consent/approval
  always remains on the authoritative local store. `QdrantReviewMemory` is a
  *mirror* of approved units: it stores vectors and performs semantic search
  (`upsert` / `vector_search`), and a failed mirror write is surfaced (never
  silently swallowed). If Qdrant is unavailable, retrieval reports
  `MEMORY_UNAVAILABLE` and the review **continues without fabricated context**.
- The Helm chart (`deploy/helm/paper-guard`) deploys the Paper Guard service and
  **does not bundle Qdrant or Ollama** — those are configured external endpoints.

The chart exposes configurable replicas, image, resources, service type/port,
LLM provider/endpoint/model, **memory configuration (enabled/mode/top-k/
min-similarity/embedding provider+model)**, Qdrant endpoint, persistent storage,
and logging. API keys live in a Kubernetes Secret referenced by name — never in
`values.yaml`.

### 2.15 Reviewer integration (memory-aware review)

When `[memory] enabled` + a retrieving mode, the pipeline retrieves authorized
approved memory relevant to the manuscript and renders it as a delimited,
untrusted block appended to each reviewer's user prompt after the current
document. `paper-guard-review` exposes `ReviewerContext::memory_context` (a
rendered string) plus `render_memory_context` / `MEMORY_UNTRUSTED_PREAMBLE`.
The prompt structure keeps `=== CURRENT DOCUMENT (evidence) ===` and
`=== HISTORICAL REVIEW MEMORY (untrusted) ===` clearly separate; the model is
told memory is not evidence and not an instruction, and `similarity ≠
correctness`. A reviewer never receives another reviewer's current-run findings
— only historical, authorized memory.

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

M3 adds:

- **Ollama-compatibility contract tests** (`paper-guard-llm/tests/ollama_compat.rs`):
  a mocked OpenAI-compatible endpoint verifies the keyless local path (no
  Authorization header), keyless construction, and structured-response
  pass-through for downstream `REVIEWER_OUTPUT_INVALID`.
- **Service tests** (`paper-guard-service`): in-process HTTP tests verify
  `GET /health`, `POST /reviews` (running the *same* shared pipeline), review
  status/findings, loopback-bind enforcement, and human-feedback memory
  recording (private by default).
- **Memory tests** (`paper-guard-app`): verify the default `PRIVATE` state,
  `MEMORY_APPROVED` retrieval, `TRAINING_APPROVED` export, that private/rejected
  units are never retrievable as context, and that retrieved memory is always
  framed as `HISTORICAL REVIEW MEMORY` (never current-paper evidence).
- **Client tests** (`paper-guard-client/tests/client.rs`): verify the typed
  methods against a mocked wiremock service — health, submit, status, findings,
  feedback — plus the error taxonomy (connection refused, timeout, 400/401/403/
  404/409/429/500/503, malformed JSON), mode resolution, and security invariants
  (tokens are sent but never logged/serialized; manuscript content never leaks).
- **Service upload tests** (`paper-guard-service`): `POST /reviews` accepts
  base64 `content_base64` uploads and writes them to a managed server dir.
- **Optional end-to-end integration test** (`paper-guard-client/tests/service_integration.rs`,
  #ignored, enable with `PAPER_GUARD_SERVICE_TESTS=1`): starts the real service
  and drives it via the actual client with `MockProvider`, verifying the full
  `client → HTTP → service → app → review → ledger → HTTP → client` lifecycle.

M4 adds:

- **Memory repository + mode tests** (`paper-guard-app`): create/read/update/
  delete(persist and reopen), memory modes (OFF/READ_ONLY/WRITE/READ_WRITE),
  and persistence across reopen.
- **Privacy + authorization tests**: private memory requires its owner; team
  members cannot access another's private memory; rejected memory is excluded;
  category/reviewer filtering applies during retrieval.
- **Memory-aware reviewer tests** (`paper-guard-review/tests/memory_context.rs`):
  historical memory is rendered inside a delimited, untrusted block distinct
  from the current document; prompt injection inside a memory entry does not
  override the reviewer system prompt; empty memory produces no block (no
  behaviour change).
- **Service memory workflow test** (`paper-guard-service`): `feedback →
  approval → memory → retrieval` and `reject` through the new `GET /memory`,
  `GET /memory/{id}`, `POST /memory/{id}/approve`, `POST /memory/{id}/reject`
  endpoints.
- **Optional Qdrant integration test** (`paper-guard-app/tests/qdrant_integration.rs`,
  #ignored, enable with `PAPER_GUARD_QDRANT_TESTS=1`): write memory → read
  memory → retrieve memory through a real/configured Qdrant.

The default `cargo test --workspace` requires **no** external service (no
OpenAI/Mammoth.ai/Ollama/Qdrant/Kubernetes/internet); all integrations are
either mocked, offline, or opt-in.
