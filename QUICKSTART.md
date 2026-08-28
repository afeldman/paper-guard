# Paper Guard — Quick Start

Paper Guard is a reproducible, multi-agent scientific **review** and
**revision-quality** system for papers you have already written. It reviews a
paper; it does not write the paper.

## Published binaries

Paper Guard ships **self-contained binaries** for the supported platforms. No
Rust, Cargo, Python, Docker, WSL, or Kubernetes is needed — just download the
archive for your machine, extract it, and run it.

| Platform       | Architecture | Archive suffix                     |
| -------------- | ------------ | ---------------------------------- |
| macOS          | ARM64        | `aarch64-apple-darwin.zip`         |
| Linux          | ARM64        | `aarch64-unknown-linux-gnu.tar.gz` |
| Linux          | x86_64       | `x86_64-unknown-linux-gnu.tar.gz`  |
| Windows        | x86_64       | `x86_64-pc-windows-msvc.zip`       |

Every archive contains the binary, this `QUICKSTART.md`, and the `LICENSE`.
Release artifacts are produced by GitHub Actions from the tagged Git commit —
never hand-copied from a developer machine.

## Verify your binary

Every release ships a **`SHA256SUMS`** file covering every archive. Download it
alongside the archive and verify before extracting.

- **Linux**

  ```bash
  sha256sum -c SHA256SUMS
  ```

- **macOS**

  ```bash
  shasum -a 256 paper-guard-vX.Y.Z-aarch64-apple-darwin.zip
  # compare the output against the SHA256SUMS file
  ```

- **Windows**

  ```powershell
  Get-FileHash .\paper-guard-vX.Y.Z-x86_64-pc-windows-msvc.zip -Algorithm SHA256
  # compare the output against the SHA256SUMS file
  ```

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

That's it. A human stays in control of what is accepted. A normal `review` does
not modify your manuscript.

## Choosing a review provider

By default Paper Guard runs the deterministic (offline, reproducible) **mock**
provider. To use a real OpenAI-compatible endpoint — Ollama, LM Studio, OpenAI,
Mammoth.ai, or any local server — create a `paper-guard.toml`:

```
paper-guard init paper-guard.toml
```

Set `provider = "openai-compatible"` and the endpoint under
`[providers.openai-compatible]`, then review:

```
paper-guard --config paper-guard.toml review paper.tex
```

**Ollama example** (Windows, macOS, or Linux — the same provider works on all):

```toml
[llm]
provider = "openai-compatible"

[providers.openai-compatible]
base_url = "http://localhost:11434/v1"
model = "llama3.2"
structured_output = "json_schema"
```

Secrets (API keys) are always read from the environment, never stored in
config, logs, or the ledger. Local providers such as Ollama and LM Studio
normally need no API key at all.

---

- Windows users: see [`docs/windows.md`](docs/windows.md)
- Architecture & infrastructure: see [`docs/architecture.md`](docs/architecture.md)
