<p align="center">
  <img src="docs/logo.png" alt="Paper Guard" width="150">
</p>

# Paper Guard

> Local AI-assisted pre-review for scientific papers.

Paper Guard is a reproducible, multi-agent scientific **review** tool that runs
locally and acts as a **pre-submission quality gate**. It reviews an
already-written manuscript, identifies potential problems across several
independent review perspectives, and presents those findings to the researcher
for human judgment.

> **Paper Guard does not write scientific papers. It reviews an already-written
> manuscript, identifies potential problems, and presents those findings to the
> researcher for human judgment.**

The language model acts as an **advisor / reviewer**, never as an execution
authority. All scientific decisions stay with the human researcher. Because a
local LLM can be used, unpublished manuscripts do not need to leave the local
environment; the same tool can also target OpenAI-compatible remote endpoints
when that is appropriate. The architecture is designed so it can later be
shared by a team over a LAN.

```
Write paper → Paper Guard review → researcher fixes issues → human/peer review → submission
```

Paper Guard is **not** a replacement for real peer review. It is an early,
structured pass intended to help researchers reach reviewers who can then
address substantive scientific feedback instead of catchable basics.

---

## Architecture

Paper Guard parses a manuscript into a **canonical paper model** so reviewers
work on normalized structure rather than raw text. The core pipeline:

```text
Manuscript
   │
   ▼
Parser
   │
   ▼
Canonical Paper Model
   │
   ├── Scientific Reviewer
   ├── Adversarial Reviewer
   ├── Evidence / Claim Checker
   ├── Reference Checker
   └── Figure / Table Reviewer
            │
            ▼
          Judge
            │
            ▼
   Prioritized Findings
            │
            ▼
      Human Approval
            │
            ▼
      Optional Revision
```

Each reviewer has its own independent role:

- **Scientific Reviewer** — examines scientific correctness, methodology,
  assumptions, interpretation, logical consistency, and scientific validity.
- **Adversarial Reviewer** — acts as a hostile peer reviewer searching for
  weaknesses, unsupported assumptions, contradictions, ambiguities, missing
  controls, and likely reviewer attacks.
- **Evidence / Claim Checker** — checks the relationship between
  Claim → Evidence → Result and identifies claims that are unsupported,
  insufficiently supported, or not adequately connected to the presented
  evidence.
- **Reference Checker** — checks citations, references, citation placement,
  citation-to-claim relationships, and whether the cited literature appears
  appropriate for the claim.
- **Figure / Table Reviewer** — checks figures, tables, captions, labels, units,
  consistency with the text, readability, and whether visual material
  adequately supports the scientific argument.

Reviewers run **independently** and in parallel on the same canonical model.
Their individual findings are then consolidated by the **Judge**, which merges
redundant findings, detects reviewer conflicts, assigns severities and
priorities, and decides on revision actions.

---

## Local-first LLM support

Paper Guard intentionally has **no separate LM Studio provider and no separate
Ollama provider**. Both — like OpenAI, Mammoth.ai, or any local server — are
reached through a single generic provider:

```toml
[llm]
provider = "openai-compatible"
```

This works because LM Studio and Ollama both expose **OpenAI-compatible APIs**.
Switching backends is therefore a configuration change, never a code change.

### Structured output modes

The provider supports several structured-output modes (configured per
endpoint under `[providers.openai-compatible]`):

```toml
structured_output = false
structured_output = true
structured_output = "json_object"
structured_output = "json_schema"
```

| Value | Meaning |
| ----- | ------- |
| `false` | Free-form provider response. Reviewer-side validation remains authoritative and still enforces the JSON/finding shape. |
| `true` / `"json_object"` | Historical "JSON object" hint mode. |
| `"json_schema"` | Strict JSON Schema response format. **Recommended** for local models such as LM Studio/Qwen and compatible Ollama setups. |

**Regardless of the provider output mode, reviewer-side schema/domain
validation remains mandatory.** Structured output constrains the JSON
*transport* shape; it does **not** make an LLM scientifically trustworthy.
Scientific validity is enforced separately by domain validation, evidence
checks, provenance, the Judge, and integrity guards.

---

## LM Studio local example

A practical configuration for a local LM Studio server:

```toml
[llm]
provider = "openai-compatible"

[providers.openai-compatible]
base_url = "http://localhost:1234/v1"
api_key_env = ""
model = "qwen/qwen3.5-9b"
structured_output = "json_schema"
vision = false
```

- `api_key_env = ""` — local LM Studio normally needs **no API key**; when
  empty, requests are sent without an `Authorization` header.
- `structured_output = "json_schema"` — strict JSON Schema output, recommended
  for local models that support it.
- `vision = false` — set to `true` only if the local model is genuinely
  multimodal.

Confirm LM Studio is serving before reviewing:

```bash
curl http://localhost:1234/v1/models
```

Run the review against that configuration (source build):

```bash
paper-guard review ./phobos.tex \
  --config ./paper-guard-lmstudio.toml
```

If you downloaded the **release binary** instead (see
[Installation](#installation--binaries)), invoke it the same way:

```bash
./paper-guard review ./phobos.tex --config ./paper-guard-lmstudio.toml
```

A **real local end-to-end run** has been performed successfully with:

- Qwen 3.5 9B
- LM Studio
- the OpenAI-compatible API
- JSON Schema structured output
- all five reviewers
- the Judge
- findings persisted to the ledger
- the manuscript left **unchanged**

That run is a demonstration of the workflow. The resulting findings are
**model-generated review findings that require your scientific judgment** — they
are not automatically correct and are not a substitute for human peer review.

---

## Ollama / Windows

Because many researchers use Windows, the same generic `openai-compatible`
provider works with **Ollama on Windows** — no provider-specific code is
introduced for Windows.

```text
Windows
  │
  ├── Paper Guard
  │
  └── Ollama
       └── OpenAI-compatible API
```

Ollama exposes an OpenAI-compatible endpoint locally:

```toml
[providers.openai-compatible]
base_url = "http://localhost:11434/v1"
model = "llama3.2"
structured_output = "json_schema"
```

Local Ollama typically requires no API key; when a setup does, you reference
the key by environment-variable **name** only (`api_key_env = "OLLAMA_API_KEY"`),
never the key itself. Paper Guard itself does not require any Ollama-specific
integration — it sees an ordinary OpenAI-compatible server.

The project also publishes a **native Windows executable** (`paper-guard.exe`)
as part of its cross-platform release, so no Rust or other toolchain is needed
on a Windows researcher machine. No administrator privileges are required for
normal operation and Paper Guard does not modify the firewall automatically.
See [`docs/windows.md`](docs/windows.md) for the detailed Windows guide.

---

## Installation / binaries

Paper Guard ships **self-contained release artifacts** for the supported
platforms:

| Platform       | Architecture | Rust target                | Release archive                                       |
| -------------- | ------------ | -------------------------- | ----------------------------------------------------- |
| macOS          | ARM64        | `aarch64-apple-darwin`      | `paper-guard-vX.Y.Z-aarch64-apple-darwin.zip`         |
| macOS          | x86_64       | `x86_64-apple-darwin`       | `paper-guard-vX.Y.Z-x86_64-apple-darwin.zip`          |
| Linux          | ARM64        | `aarch64-unknown-linux-gnu` | `paper-guard-vX.Y.Z-aarch64-unknown-linux-gnu.zip`    |
| Linux          | x86_64       | `x86_64-unknown-linux-gnu`  | `paper-guard-vX.Y.Z-x86_64-unknown-linux-gnu.zip`     |
| Windows        | x86_64       | `x86_64-pc-windows-msvc`    | `paper-guard-vX.Y.Z-x86_64-pc-windows-msvc.zip`       |

Each archive contains the binary (`paper-guard` / `paper-guard.exe`),
`QUICKSTART.md`, and `LICENSE`. All five platforms are built natively by GitHub
Actions.

The intended workflow is:

```text
download archive
→ extract
→ run paper-guard
```

No Rust, Docker, Kubernetes, or Python is required to use a prebuilt binary.
To build from source (macOS / Linux), a recent Rust toolchain is sufficient:

```bash
git clone https://github.com/afeldman/paper-guard.git
cd paper-guard
cargo build --release
./target/release/paper-guard --help
```

Release archives are produced by **CI from the tagged Git commit** and include
a `SHA256SUMS` file covering every archive. Verify the downloaded archive:

- **Linux**: `sha256sum -c SHA256SUMS`
- **macOS**: `shasum -a 256 paper-guard-vX.Y.Z-aarch64-apple-darwin.zip`
- **Windows**: `Get-FileHash .\paper-guard-vX.Y.Z-x86_64-pc-windows-msvc.zip -Algorithm SHA256`

Builds carry traceable build metadata — `paper-guard info` reports version,
platform triple, Git commit (via `PAPER_GUARD_BUILD_COMMIT` embedded by CI), and
build profile. (Homebrew, winget, scoop, and similar package managers are
**not** currently supported.)

---

## Basic CLI workflow

`paper-guard --help` lists all commands. The core ones for a local researcher:

| Command | What it does | Reads manuscript? | Modifies manuscript? |
| ------- | ------------ | ----------------- | -------------------- |
| `paper-guard review <SOURCE>` | Parse + parallel review + Judge + ledger. Default, read/review only. | Yes | No |
| `paper-guard run <SOURCE>` | Full workflow: review + judge + revision + render + validate. Revisions require approval. | Yes | Only with explicit `--approve-all` |
| `paper-guard info` | Print version, platform, build profile, config/data dirs. | No | No |
| `paper-guard diagnostics --paths` | Print resolved config/data/cache/log directories. Never prints secrets or manuscript content. | No | No |
| `paper-guard discover` | Find Paper Guard services on the LAN (mDNS/DNS-SD). Never uploads. | No | No |
| `paper-guard findings` | List findings from the latest run. | No | No |
| `paper-guard ledger` | Show the review ledger (runs + status). | No | No |
| `paper-guard report [RUN]` | Emit a summary report for a run (defaults to latest). | No | No |
| `paper-guard feedback <RUN> <FINDING> --decision <accept\|reject\|modified>` | Record a human decision on a finding (stored as a private Review Memory candidate). | No | No |
| `paper-guard memory …` | List / show / approve / reject / search approved review memory. | No | No |
| `paper-guard prompts init` | Copy the embedded default reviewer prompts into the prompt directory. Never overwrites existing files. | No | No |
| `paper-guard prompts list` | Show which prompt source each reviewer role resolves to (`external` / `embedded-default`). Never prints prompt contents. | No | No |
| `paper-guard setup` | Create `~/.paper-guard/{config,prompts,logs,data}`, write a default `config.toml`, export default prompts. Idempotent. | No | No |
| `paper-guard config show` | Print the effective configuration (user config + defaults); secrets are redacted. | No | No |
| `paper-guard config edit` | Open the user configuration in `$VISUAL`/`$EDITOR` (or a platform default). | No | No |
| `paper-guard bibliography <paper>` | Verify bibliography metadata against scholarly sources (arXiv; Google Scholar reports `Unavailable`). Off unless `[bibliography] enabled = true`; `--output json` and `--clear-cache` supported. | No | Yes (opt-in) |

Minimal examples:

```bash
paper-guard review paper.tex
paper-guard run manuscript/main.tex
paper-guard info
paper-guard diagnostics --paths
paper-guard discover        # list-only; explains how to enable discovery
paper-guard findings
paper-guard feedback run-001 PG-0001 --decision accept --feedback "Looks correct."
```

`review` reads the manuscript but does not modify it. `run` only applies
revisions when you explicitly approve them (typically interactively, or via
the explicit `--approve-all` flag). See [Review findings and human
approval](#11-review-findings-and-human-approval).

---

## Human-readable output

The default on-screen output is a **human-readable report** that makes the
multi-agent workflow visible:

```text
Paper Guard Review

Reviewer 1: Scientific Reviewer
Purpose: ...

Reviewer 2: Adversarial Reviewer
Purpose: ...

Reviewer 3: Evidence / Claim Checker
Purpose: ...

Reviewer 4: Reference Checker
Purpose: ...

Reviewer 5: Figure / Table Reviewer
Purpose: ...

Judge
...

Consolidated Findings
...
```

This is useful because a researcher immediately sees *which* reviewers ran,
*what* each found, which findings were accepted / rejected / consolidated by
the Judge, and which issues require human approval — without reading raw JSON.
The report is a **presentation layer** generated from the canonical run record;
it introduces no new findings, and it cannot change severity, confidence,
evidence, claims, or Judge decisions.

Three presentation styles adjust the report's wording:

```bash
paper-guard review paper.tex --style neutral
paper-guard review paper.tex --style funny
paper-guard review paper.tex --style insulting
```

- `neutral` is the **default** (and falls back to the `[review] style` config
  value, then `neutral`).
- `funny` and `insulting` are **presentation styles only**. They cannot change
  findings, severity, confidence, evidence, claims, Judge decisions, or
  revision scope.
- The `insulting` style criticizes the **paper / argument**, never the author
  personally.

---

## JSON output / machine-readable workflow

JSON support is deliberate so results can feed automation. Paper Guard keeps
two **deliberately separate presentation layers**: human-readable prose on your
terminal, and canonical machine-readable records on disk.

```text
Paper Guard
    │
    ▼
JSON
    │
    ├── jq
    ├── Python
    ├── CI/CD
    ├── dashboards
    └── other LLM/automation systems
```

For every run, Paper Guard writes **canonical JSON artifacts** to the run's
data directory (`{data_dir}/{run_id}/`), independent of the `--style` and
`--output` you choose on screen:

- `claims.json`, `findings.json`, `judge.json`
- `revisions.json`, `validation.json`, `ledger.json`, `paper.json`
- `schema.json` (schema/run manifest)

> There is intentionally **no `--output json` CLI flag**. JSON is emitted as
> canonical on-disk artifacts, not to stdout. The `--output` flag only controls
> the human-readable terminal report (`human` default, or `summary`).

Use the artifacts from scripts or the shell:

```bash
jq '.[0] | {finding, severity, reviewer}' < .paper-guard/run-001/findings.json
```

or from Python / CI / charts by reading the same JSON files. Because the JSON
is the canonical representation, it is **stable regardless of presentation
style** — the same logical findings produce byte-identical JSON whether viewed
as `neutral`, `funny`, or `insulting`.

---

## Review styles

| Style     | Purpose                                                           |
| --------- | ----------------------------------------------------------------- |
| neutral   | Scientific / professional default                                 |
| funny     | Humorous presentation                                             |
| insulting | Aggressively critical presentation of the paper, **never** the author |

> **Styles change presentation only. They do not change the underlying
> scientific review.**

Select with `--style`; default priority is `--style` > `[review] style` config >
`neutral`.

---

## Review findings and human approval

A real review output may end with entries such as:

```text
needs approval: REV-0001 (finding PG-0003)
```

This is an important safety property:

- Paper Guard **does not silently apply major revisions**.
- Human approval **remains part of the workflow**.
- A **normal `review` does not modify the manuscript** — it is read-only.
- `--approve-all` exists and is an **explicit, user-controlled** option to
  non-interactively approve all required revisions — never the default.

```
Review ≠ automatic rewriting
```

Even with `--approve-all`, every change maps to a finding, follows an explicit
revision instruction, is diffable, logged, and provenance-tagged. There are no
silent changes.

---

## Scientific-integrity guarantees

Paper Guard enforces these properties in the implementation:

- **Reviewer-side schema validation** — reviewer output is validated against
  the domain schema and rejected when invalid.
- **JSON Schema structured output** — optional but recommended at the transport
  layer for compatible local models; it never replaces domain validation.
- **Invalid model responses rejected** — malformed output surfaces as
  `REVIEWER_OUTPUT_INVALID` rather than being coerced into a finding.
- **No unsafe free-form fallback** — if the requested structured-output mode
  cannot be honoured, the provider fails explicitly rather than silently
  downgrading to unconstrained interpretation.
- **Human approval for required revisions** — major revisions require explicit
  approval.
- **Canonical RunRecord** — the canonical finding record is the single source
  of scientific truth for a run.
- **Presentation layer cannot modify canonical findings** — the report reads
  from the run record and never becomes a second source of truth.
- **Styles cannot alter scientific semantics** — `funny`/`insulting` change
  wording only.
- **Manuscript not modified by a normal review** — `review` is read-only.
- **No generated experiments, results, references, or evidence** — no agent may
  fabricate scientific content; missing evidence is reported as an explicit
  state, never invented.
- **Local-first execution** — the default provider is the deterministic
  `mock`; with a local LLM, manuscripts need not leave the machine.
- **Discovery does not imply authorization** — a discovered service is not
  trusted or uploaded to automatically.
- **Memory is explicitly controlled and approval-based** — see
  [Review Memory](#14-review-memory).

These are concrete engineering guarantees, not claims that Paper Guard makes a
manuscript scientifically correct.

---

## Team / LAN functionality

Paper Guard has optional **LAN discovery** for finding a Paper Guard service on
the local network:

- Service type **`_paper-guard._tcp`** via **mDNS / DNS-SD**.
- Candidates are verified through `GET /health` (health verification).
- **Version compatibility** is checked; a major-incompatible service is reported
  as incompatible rather than used.
- **Discovery is not authorization.** A manuscript is never uploaded merely
  because a service was discovered; transmission requires explicit remote-mode
  selection.
- **Discovery is disabled by default** unless explicitly enabled in config
  (`[discovery]`).
- Paper Guard does not require you to run Avahi, and discovery uses standard
  multicast (mDNS/DNS-SD) — no privileged networking is introduced by the
  application.

Because the pipeline lives in a shared application layer, a single running
service can later serve several researchers; the architecture is suitable for a
future shared team service without changing the review logic. See
[`docs/architecture.md`](docs/architecture.md) for details.

---

## Review Memory

Paper Guard has a **retrieval-based review-memory foundation**:

```text
review
  ↓
human feedback
  ↓
approval
  ↓
review memory
  ↓
semantic retrieval
  ↓
future reviewer guidance
```

Important properties:

- **Disabled by default** (`[memory] enabled = false`); when enabled, it runs in
  `off` / `read_only` / `write` / `read_write` modes.
- **Human approval is required** before a unit becomes retrievable guidance.
- **Memory is separate from the manuscript** — it is historical review
  experience, never current-paper evidence, and it is injected as a clearly
  delimited *untrusted* block so it cannot override the reviewer's system
  prompt.
- **Private / team scopes exist** — a private unit is visible only to its
  owner; a team unit to the owning team.
- **Rejected memory is never returned as guidance** and is removed from
  retrieval/export eligibility.
- This is **retrieval-based memory, not model fine-tuning**. Paper Guard does
  **not** train or retrain the LLM; automatic LoRA/QLoRA-style training is
  deliberately out of scope.

---

## Example: reviewing a real paper

This is an end-to-end example that resembles a real successful local run on
`phobos.tex`:

```text
Mode: local
Provider: OpenAI-compatible
Model: qwen/qwen3.5-9b
structured_output = json_schema

5 reviewers
→ findings
→ Judge
→ consolidated findings
→ human approval required
→ revisions applied: 0
→ paper modified: NO
```

Run it locally (source or release binary):

```bash
paper-guard review ./phobos.tex --config ./paper-guard-lmstudio.toml
```

The report shows the five reviewers, the Judge's consolidation, the
consolidated findings, the items needing approval (at least one, e.g.
`needs approval: REV-…`), and the validation block stating the manuscript was
not modified (`revisions applied: 0`, `paper modified: NO`). This demonstrates
the **workflow**; it is **not** a claim that the model's findings are
automatically authoritative. Review each finding and give your own scientific
judgment.

---

## Performance expectations

Local LLM review can take **several minutes**, depending on:

- model size
- hardware (CPU/GPU, memory)
- number of reviewers (default five, configurable)
- context size
- structured-output generation time
- the number of reviewer and Judge LLM calls

This is **expected** when running a real local model. Paper Guard does not make
benchmark claims about end-to-end review times beyond this expectation; the
deterministic `mock` provider is available whenever you want a fast, offline
smoke test of the pipeline.

---

## Configuration precedence

Paper Guard applies a `CLI > config > default` precedence where a CLI flag
overrides configuration, which in turn overrides the built-in default. This is
implemented for:

- **Review style** — `--style` > `[review] style` (config) > `neutral`.
- **Server selection** — an explicit `--server` flag always wins; a configured
  `[server].url` is second; otherwise the run is local. No implicit switching
  via environment variables.
- **Provider configuration** — set in `config` (`[llm] provider` and
  `[providers.openai-compatible]`); the default provider is the offline
  `mock`.
- **Reviewer prompts** — an external prompt file (resolved through
  `[prompts] directory`) overrides the embedded default compiled into the
  binary.

**Configuration file resolution.** When a command does not receive an explicit
`--config`, Paper Guard uses the canonical user configuration when it exists:

```text
CLI arguments                     (highest priority)
explicit --config file
~/.paper-guard/config/config.toml (user configuration)
built-in defaults
```

A missing user configuration is **not an error** — `paper-guard review paper.tex`
works out of the box on a fresh machine. Run `paper-guard setup` to create the
user layout, or `paper-guard config edit` to open the user configuration in
your editor.

Secrets are always referenced by environment-variable **name** in config (e.g.
`PAPER_GUARD_TOKEN`), never stored in the config file or logs.

---

## User configuration, setup & rolling logs

Paper Guard owns one canonical per-user directory, resolved through the
platform home directory (`%USERPROFILE%\.paper-guard` on Windows — never a
hard-coded `/Users/...`, `/home/...` or `C:\...` path):

```text
~/.paper-guard/
├── config/
│   ├── config.toml          # user configuration (optional)
│   └── prompts/             # editable reviewer prompts (optional)
├── logs/                    # rolling technical logs (paper-guard.log, .1 … .5)
└── data/                    # per-user review data (opt-in, see below)
```

### `paper-guard setup`

Creates the layout above, writes a default `config.toml` and exports the
embedded default reviewer prompts into `config/prompts/`. **Idempotent** —
existing user files are never overwritten, so running it repeatedly (or after
editing prompts/config) is safe:

```bash
paper-guard setup
```

```text
Paper Guard Setup
ok /Users/you/.paper-guard
ok /Users/you/.paper-guard/config
ok /Users/you/.paper-guard/config/prompts
ok /Users/you/.paper-guard/logs
ok /Users/you/.paper-guard/data
ok /Users/you/.paper-guard/config/config.toml (created with built-in defaults)
ok 5 reviewer prompt file(s) exported to /Users/you/.paper-guard/config/prompts
Paper Guard is ready.
```

Setup is optional: the binary works with zero configuration. It only exists to
make the built-in defaults visible and editable.

### `paper-guard config show` / `config edit`

```bash
paper-guard config show      # effective configuration (secrets redacted)
paper-guard config edit      # open ~/.paper-guard/config/config.toml in $VISUAL/$EDITOR
```

`config show` prints the effective configuration after applying the resolution
order above. **Secrets are never printed**: values that look like API keys,
tokens, JWTs, bearer credentials, private keys etc. are replaced with
`[redacted]`, and Paper Guard never reads environment variables into config
output. `config edit` uses `$VISUAL`, then `$EDITOR`, then a platform default
editor; if no editor can be found you get a clear error telling you to set
`$EDITOR`.

### `~/.paper-guard/data`

The per-user data directory is created by `setup` for users who prefer a
user-owned location for review artifacts. The default remains the
project-relative `.paper-guard` directory for historical compatibility; set
`[reproducibility] data_dir = "~/.paper-guard/data"` (or
`[service] data_dir`) to use it.

### Rolling technical logs

Technical logs are mirrored to `~/.paper-guard/logs/paper-guard.log` with
size-based rotation:

```text
max file size:    10 MiB per file
max rotated files: 5   (paper-guard.log.1 … paper-guard.log.5)
≈ max local usage: 60 MiB
```

Rotation is automatic and thread-safe: when the current file reaches 10 MiB it
is renamed to `.1` (shifting older files up) and the oldest `.5` is deleted —
no manual cleanup needed. Logs contain technical diagnostics only (provider,
model, reviewer, stage, run id, status, finding count, error category) and
never manuscript contents or secrets; a defense-in-depth scrubber additionally
masks credential-looking tokens in the file. The human-readable CLI output on
stdout is unchanged — the log file is a separate technical channel.

---

## External reviewer prompts

Reviewer prompts are the only LLM prompts Paper Guard uses: the five reviewers
(`scientific`, `adversarial`, `evidence`, `references`, `figures`) each receive
a system prompt whose *role instructions* can be edited as plain Markdown files
on disk.

> **Prompt changes take effect without recompiling Paper Guard.** There is no
> rebuild step: edit a prompt file and run the next review — the new text is
> picked up automatically.

### Where prompts live

External prompts live in the prompt directory, which defaults to a
cross-platform home-directory path:

```text
~/.paper-guard/config/prompts/
```

(`%USERPROFILE%\.paper-guard\config\prompts\` on Windows.) The directory is resolved
through the platform home directory — never hard-coded per-OS paths, and a
literal `~` in a configured path is expanded portably. Override it in config:

```toml
[prompts]
directory = "~/my-custom-prompts"   # optional; tilde is expanded
```

### Resolution order

For each reviewer role the loader uses:

1. the external file `<prompt-directory>/<role>.md`, if present;
2. otherwise the **embedded default** compiled into the binary.

A fresh install works with no configuration at all: no prompt directory exists,
so every role resolves to its embedded default and the binary stays fully
self-contained (no network, no missing-asset failure).

**Fallback rule.** A *missing* prompt file falls back to the embedded default.
A file that *exists* but cannot be read (permission denied, I/O error, not a
regular file, or a symlink that resolves outside the prompt directory) is a
hard error — a deliberately broken external prompt is never silently replaced
by another prompt.

### What a prompt file controls

Each `<role>.md` file replaces the role-instructions paragraph of that
reviewer's system prompt. Paper Guard always composes the fixed wrapper
(`You are …`), the non-negotiable **SCIENTIFIC INTEGRITY RULE** preamble, and
the authoritative arrangement note around your text in code, so a prompt file
cannot silently remove the integrity guardrails. The document under review is
appended by code in the user message — there are no template placeholders to
keep in sync.

The Judge and the revision engine are deterministic code, not LLM agents, so
they have **no** prompt files (`judge.md` / `revision.md` do not exist and are
never loaded).

### Getting started

Copy the embedded defaults into the prompt directory (existing files are never
overwritten):

```bash
paper-guard prompts init
```

```text
Initialized Paper Guard prompts in /Users/you/.paper-guard/config/prompts
  scientific.md
  adversarial.md
  evidence.md
  references.md
  figures.md
Existing files were left unchanged.
```

See which source each role currently resolves to (never prints prompt
contents):

```bash
paper-guard prompts list
```

```text
Prompts:
  directory: /Users/you/.paper-guard/config/prompts
  scientific: embedded-default
  adversarial: external
  evidence: embedded-default
  references: embedded-default
  figures: embedded-default
```

`paper-guard info` prints the same prompt block, and
`paper-guard diagnostics --paths` shows the resolved default prompt directory.

### Changing a prompt

Edit a copied file and run the next review — no rebuild:

```bash
$EDITOR ~/.paper-guard/config/prompts/scientific.md
paper-guard review paper.tex --style neutral
```

For example, an edited `scientific.md` could look like:

```markdown
Focus on statistical rigor above all: check effect sizes, confidence
intervals, multiple-comparison handling, and whether the conclusions
follow from the reported numbers. Flag over-strong wording.
```

A prompt file is **untrusted configuration** with the same trust boundary as
`paper-guard.toml`: it can steer a reviewer's attention, but it cannot create
findings directly, cannot bypass schema/domain validation, the Judge, the
evidence system, or the ledger, and is never executed.

### Reproducibility: prompt provenance in run records

Every review records which prompt each agent actually used. Each agent outcome
in the ledger (`ledger.json` / the run record) carries:

```json
"prompt_usage": {
  "prompt": "scientific",
  "source": "external",
  "sha256": "a3f2…",
  "path": "/Users/you/.paper-guard/config/prompts/scientific.md"
}
```

- `source` is `embedded-default` or `external`;
- `sha256` is a stable SHA-256 of the full composed system prompt text that was
  sent to the model for that role — the same prompt content always produces the
  same hash, and any change produces a different hash;
- prompt **contents are never stored** in the ledger, only provenance.

A run can therefore always be audited later for *which* prompt actually
influenced it, while the canonical RunRecord and its integrity guarantees stay
unchanged.

---

## Bibliography verification (optional)

Paper Guard can **optionally** verify the bibliographic metadata of a paper's
references against scholarly sources. This is the **Bibliography Verification
Layer** — a clearly separated, additive stage of the review run:

```text
Paper
  │
  └── Bibliography ──► BibliographyVerifier
                          ├── arXiv          (real, opt-in network)
                          ├── Google Scholar (Unavailable slot — see below)
                          └── mock           (offline test/demo double)
                          ▼
                      canonical result per reference
                          ▼
                      RunRecord JSON / human report
```

It is **disabled by default** — every run stays offline unless you opt in:

```toml
[bibliography]
enabled = true               # master switch (default: false)
provider = "arxiv"           # "arxiv" (real) or "mock" (offline test double)
timeout_seconds = 15         # per-request timeout

[bibliography.arxiv]
enabled = true               # the real arXiv provider (fixed endpoint only)

[bibliography.google_scholar]
enabled = false              # see the Google Scholar note below

[bibliography.cache]
enabled = true
max_entries = 200            # bounded, oldest entries evicted first
max_bytes = 8388608          # 8 MiB total
```

Run a pure bibliography check without a full review:

```bash
paper-guard bibliography paper.tex
paper-guard bibliography paper.tex --output json
paper-guard bibliography --clear-cache          # delete the local response cache
```

Or include it in a review (`review`/`run` also accept `--bibliography` to
enable it for that run without editing the config). Results land in the
canonical RunRecord JSON under `"bibliography": [...]` and appear as a
"Bibliography Verification" section in the human-readable report.

### What is verified — and what is not

The layer verifies **bibliographic metadata identity**: title, authors, year,
venue, DOI, arXiv id. The statuses are:

| Status                | Meaning                                                        |
|-----------------------|----------------------------------------------------------------|
| `verified`            | Source entry matches the cited metadata; no contradicting field |
| `likely_match`        | Strong candidate; minor wording/venue differences               |
| `partial_match`       | Weak candidate; some metadata overlaps                          |
| `conflicting_metadata`| Candidate found whose metadata conflicts (year/DOI/arXiv/authors) |
| `not_found`           | No usable candidate found                                       |
| `unavailable`         | Source not reachable/not usable (network, timeout, disabled)    |
| `not_checked`         | Deliberately not verified                                       |

Every result keeps the original citation unchanged and lists explicit
`mismatches[]`. Paper Guard **never auto-corrects** a bibliography.

> **Bibliography verification checks bibliographic metadata. It does not prove
> that a cited scientific statement is substantively correct.**

### Google Scholar

Paper Guard deliberately does **not** scrape Google Scholar: there is no
stable official API for this use case, HTML scraping is fragile and violates
the service's terms of use, and CAPTCHA/rate-limit evasion is out of bounds.
The provider abstraction exists and the Scholar slot returns an honest
`Unavailable` result with an explanatory note whenever it is enabled. Stable
scholarly aggregators with public APIs (Crossref, OpenAlex, DataCite) are
technically viable future providers behind the same interface.

### Privacy, network and cache

* Bibliography verification is opt-in and sends **only the bibliographic
  metadata** of each reference (title, authors, year, arXiv id, DOI). Full
  manuscript text is never transmitted.
* The arXiv endpoint is fixed inside the binary and is not configurable
  (no SSRF surface). Requests are bounded in size and time.
* Results are cached locally under `<data_dir>/bibliography-cache/` as plain,
  bounded JSON files (key = SHA-256 of source + query). The cache is
  transparent (`from_cache: true`), deletable (`--clear-cache`), and never
  stores transient failures. It has no scientific semantics — the canonical
  RunRecord JSON remains the single source of truth.
* The `mock` provider is an offline deterministic test/demo double; every
  result is clearly labelled and must never be used as real verification.

---

## Security / privacy

**Local-first privacy model.** For a local configuration:

```text
manuscript
   ↓
Paper Guard
   ↓
local LLM
```

No manuscript needs to leave the machine. Manuscript contents and API keys are
never written to logs.

**Remote OpenAI-compatible providers.** If you configure a remote
OpenAI-compatible endpoint, the manuscript **may be sent to that provider**.
Consider your institutional and legal privacy requirements before doing so. Not
all OpenAI-compatible providers are local — LM Studio and Ollama are local
examples, but OpenAI, Mammoth.ai, and other hosted endpoints are remote by
definition.

Discovered services are treated as untrusted input, and discovery never uploads
a manuscript.

---

## Troubleshooting

### Model not found

If the local server does not know the configured model, inspect what it
actually serves:

```bash
curl http://localhost:1234/v1/models    # LM Studio
curl http://localhost:11434/v1/models   # Ollama
```

Then set `model` to a name that is actually present (e.g.
`qwen/qwen3.5-9b` for LM Studio, any pulled `ollama` model for Ollama).

### Provider asks for `OPENAI_API_KEY` unexpectedly

Ensure the configuration uses the documented `[providers.openai-compatible]`
table key, and that keyless local providers set an empty/no-key configuration
(`api_key_env = ""`). When `api_key_env` is absent or empty, requests are sent
without an `Authorization` header. Keys are read from the named environment
variable only — never stored in config.

### Structured output failures

For compatible local models, use strict JSON Schema:

```toml
structured_output = "json_schema"
```

If a local model cannot reliably honour `response_format`, set it to
`false`/`"off"`; the reviewer-side structured validation
(`REVIEWER_OUTPUT_INVALID`) still protects the pipeline.

### Windows firewall / mDNS

On Windows, multicast can sometimes be blocked. `paper-guard discover` may then
return **no services** because no mDNS replies are received. Paper Guard fails
gracefully in this case: an empty result is **not** an error, and Paper Guard
never silently uploads a manuscript or falls back to a different service. To
reach a known service directly, use the explicit remote-mode flag instead:
`paper-guard review paper.tex --server http://paper-guard-host:8080`.

---

## Development / verification

Paper Guard's normal validation (used by CI and by contributors) is:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo build --release
cargo audit
trivy fs .
```

The test suite is fully offline — no test makes a real API call. As of the
v1.1.2 release the workspace has **389 passing tests** with a clean
`clippy -D warnings` and clean `fmt --check`. An optional live end-to-end
harness runs against a real endpoint only when explicitly enabled and never in
CI (see [`docs/architecture.md`](docs/architecture.md)).

### Security scanning and dependency updates

The repository is guarded continuously:

- **Trivy** runs on every push and pull request (`trivy fs .`) scanning for
  vulnerabilities, secrets, and misconfigurations, and again as a gate before
  any release is published. It fails on detected secrets and on actionable
  HIGH/CRITICAL vulnerabilities.
- **`cargo audit`** checks the committed `Cargo.lock` for known Rust crates
  advisories.
- **Dependabot** monitors **Cargo** dependencies and **GitHub Actions**
  (`.github/dependabot.yml`). It opens weekly, bounded update PRs
  (security updates remain enabled) that must pass the normal CI — including
  fmt, test, clippy, build, and Trivy — before humans review and merge them.
  Nothing is auto-merged.

---

## Version / release status

Current release: **v1.1.2** — the first complete researcher-facing release.

### v1.0 highlights

* **Inputs**: single `.tex`, LaTeX projects (`\input` / `\include`), and `.pdf`.
* **LaTeX project resolution**: deterministic, document-order `\input` /
  `\include` expansion with provenance; path-traversal / symlink-escape
  protection; cycle and missing-include diagnostics.
* **PDF**: reliable in-process text extraction (no OCR, no embedded code
  execution) with per-page provenance; malformed / encrypted / image-only PDFs
  fail explicitly.
* **Local LLMs**: LM Studio and Ollama via the generic OpenAI-compatible
  provider; JSON Schema structured output.
* **GUI**: `paper-guard --gui` starts a local web interface (localhost-only).
* **Human + machine output**: `neutral` / `funny` / `insulting` report styles
  and canonical JSON artifacts.
* **Platforms**: macOS arm64/x86_64, Linux arm64/x86_64, Windows x86_64.

Review memory is retrieval-based; model training / fine-tuning (LoRA, QLoRA) is
deliberately not part of the current workflow. See [Review Memory](#14-review-memory).

### Configuration Wizard — planned for v1.1

Version 1.1 will add an interactive **Configuration Wizard** (`paper-guard
--wizard`) that guides a researcher through:

1. Paper Guard configuration
2. LLM provider (local vs remote)
3. LM Studio / Ollama endpoint
4. Model selection (incl. detecting local OpenAI-compatible models)
5. Structured output mode
6. Reviewer configuration
7. Memory configuration
8. Discovery configuration
9. Output / report preferences
10. Configuration validation + test connection
11. Save configuration

> **v1.1 is planning only.** The wizard is not implemented in v1.0.

---

## Quickstart

### Install

Download the appropriate binary for your platform from the GitHub Release:

| Platform | Archive |
| --- | --- |
| macOS ARM64 | `paper-guard-1.0.0-aarch64-apple-darwin.zip` |
| macOS x86_64 | `paper-guard-1.0.0-x86_64-apple-darwin.zip` |
| Linux ARM64 | `paper-guard-1.0.0-aarch64-unknown-linux-gnu.zip` |
| Linux x86_64 | `paper-guard-1.0.0-x86_64-unknown-linux-gnu.zip` |
| Windows x86_64 | `paper-guard-1.0.0-x86_64-pc-windows-msvc.zip` |

Each archive contains the `paper-guard` binary (`paper-guard.exe` on Windows),
`QUICKSTART.md`, and `LICENSE`. Verify the checksum against `SHA256SUMS` from
the Release before extracting (see the *Verify your binary* section above).

### Review a paper

```bash
# Single LaTeX file
paper-guard review paper.tex

# LaTeX project (with \input / \include)
paper-guard review main.tex

# PDF manuscript
paper-guard review paper.pdf
```

### Local web GUI

```bash
paper-guard --gui
```

This starts a local web interface on `127.0.0.1` (never on the LAN), prints the
URL, and opens the default browser. Use it to:

* see the version, provider, and configuration
* select a `.tex` or `.pdf` paper and start a review
* watch the five reviewers + Judge complete
* view findings (severity, confidence, evidence, location)
* switch presentation style (Neutral / Funny / Insulting) — never triggers an
  LLM request
* export the canonical JSON

### Human-readable output styles

```bash
paper-guard review paper.tex                   # neutral (default)
paper-guard review paper.tex --style funny
paper-guard review paper.tex --style insulting
paper-guard review paper.tex --output summary  # concise terminal summary
```

### JSON output

The canonical machine-readable artifacts (claims, findings, judge, ledger) are
always written to the data directory. They are suitable for Bash pipelines,
CI/CD, downstream LLM processing, and archival:

```bash
paper-guard review paper.tex
cat .paper-guard/run-001/ledger.json
```

### Local LLM configuration (LM Studio / Ollama)

Both LM Studio and Ollama expose an **OpenAI-compatible** endpoint. Configure
them through the same generic provider:

```toml
# paper-guard.toml
[llm]
provider = "openai-compatible"

[providers.openai-compatible]
base_url = "http://localhost:1234/v1"   # LM Studio
# base_url = "http://localhost:11434/v1"  # Ollama
model = "qwen/qwen3.5-9b"
structured_output = "json_schema"
```

---

## Documentation links

- [`docs/architecture.md`](docs/architecture.md) — full architecture, crates,
  security model, integrity coverage.
- [`docs/windows.md`](docs/windows.md) — detailed Windows instructions.
- [`configs/`](configs/) — example configuration (deterministic default, Ollama,
  OpenAI, and a real-provider example).

---

## License

Paper Guard is licensed under the [Apache License, Version 2.0](LICENSE).
