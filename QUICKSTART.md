# Paper Guard — Quick Start

Paper Guard is a reproducible, multi-agent scientific **review** and
**revision-quality** system for papers you have already written. It reviews a
paper; it does not write the paper.

## What you need

A single executable: `paper-guard` (macOS/Linux) or `paper-guard.exe` (Windows).
No Rust, no Docker, no Kubernetes, no Python — none of that is required to use
it.

## The core workflow

```
 paper → review → findings → feedback
```

**1. Review a paper (locally, offline, deterministic):**

```
paper-guard review paper.tex
```

**2. See what was found:**

```
paper-guard findings
```

**3. Give your decision on a finding:**

```
paper-guard feedback <RUN> <FINDING> --decision accept --feedback "Looks correct."
```

That's it. A human stays in control of what is accepted.

## Connect to your team service

If your team runs a shared Paper Guard service on the network:

```
paper-guard discover --force
```

lists the services it finds (this never uploads your paper). To actually send a
manuscript, you must explicitly choose the service:

```
paper-guard review paper.pdf --server http://paper-guard.local:8080
```

> **Discovery ≠ upload.** Finding a service never sends your manuscript. Only
> an explicit remote review transmits the paper.

## Choosing a review provider

By default Paper Guard runs the deterministic (offline, reproducible) mock
provider. To use a real OpenAI-compatible endpoint — Ollama, OpenAI,
Mammoth.ai, or a local server — create a `paper-guard.toml`:

```
paper-guard init paper-guard.toml
```

set `provider = "openai-compatible"` and the endpoint under
`[providers.openai-compatible]`, then:

```
paper-guard --config paper-guard.toml review paper.tex
```

Secrets (API keys) are always read from the environment, never stored in
config, logs, or the ledger.

## Verify your binary

Every release ships `SHA256SUMS`.

- Windows: `Get-FileHash .\paper-guard.exe -Algorithm SHA256`
- macOS/Linux: `shasum -a 256 paper-guard`

Compare against the checksum file in the release.

---

- Windows users: see [`docs/windows.md`](docs/windows.md)
- Architecture & infrastructure: see [`docs/architecture.md`](docs/architecture.md)
