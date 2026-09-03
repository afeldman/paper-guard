# Paper Guard — Quick Start

Paper Guard is a reproducible, multi-agent scientific **review** and
**revision-quality** system for papers you have already written. It reviews a
paper; it does not write the paper.

## Published binaries

Paper Guard ships **self-contained binaries** for the supported platforms. No
Rust, Cargo, Python, Docker, WSL, or Kubernetes is needed — just download the
archive for your machine, extract it, and run it.

| Platform       | Architecture | Rust target                | Archive                                              |
| -------------- | ------------ | -------------------------- | ---------------------------------------------------- |
| macOS          | ARM64        | `aarch64-apple-darwin`      | `paper-guard-v1.1.2-aarch64-apple-darwin.zip`        |
| macOS          | x86_64       | `x86_64-apple-darwin`       | `paper-guard-v1.1.2-x86_64-apple-darwin.zip`         |
| Linux          | ARM64        | `aarch64-unknown-linux-gnu` | `paper-guard-v1.1.2-aarch64-unknown-linux-gnu.zip`   |
| Linux          | x86_64       | `x86_64-unknown-linux-gnu`  | `paper-guard-v1.1.2-x86_64-unknown-linux-gnu.zip`    |
| Windows        | x86_64       | `x86_64-pc-windows-msvc`    | `paper-guard-v1.1.2-x86_64-pc-windows-msvc.zip`      |

Every archive contains the binary (`paper-guard`, or `paper-guard.exe` on
Windows), this `QUICKSTART.md`, and the `LICENSE`. Release artifacts are
produced by GitHub Actions from the tagged Git commit — never hand-copied from
a developer machine.

## Verify your binary

Every release ships a **`SHA256SUMS`** file covering every archive. Download it
alongside the archive and verify before extracting.

- **Linux**

  ```bash
  sha256sum -c SHA256SUMS
  ```

- **macOS**

  ```bash
  shasum -a 256 paper-guard-v1.0.0-aarch64-apple-darwin.zip
  # (use the archive filename for your architecture)
  # compare the output against the SHA256SUMS file
  ```

- **Windows**

  ```powershell
  Get-FileHash .\paper-guard-v1.0.0-x86_64-pc-windows-msvc.zip -Algorithm SHA256
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

## Reviewing different inputs

Paper Guard v1.0 supports three source formats through the same `review`
command:

```bash
# Single LaTeX file
paper-guard review paper.tex

# LaTeX project (\input / \include)
paper-guard review main.tex

# PDF manuscript
paper-guard review paper.pdf
```

For LaTeX projects, `\input`/`\include` references are resolved
deterministically in document order (nested includes supported). Path
traversal and symlink escapes are blocked. Missing files and cycles are
reported — reviewers never silently see an incomplete manuscript. The root of
a LaTeX review is the directory containing the supplied root `.tex` file.

**Inspect a source before reviewing it:**

```bash
paper-guard inspect main.tex
paper-guard inspect paper.pdf
```

## Local web GUI

Start the local GUI (binds to `127.0.0.1` only):

```bash
paper-guard --gui
```

Paper Guard prints the local URL, e.g. `http://127.0.0.1:8080`, and opens
your default browser. From the GUI you can select a manuscript, start a
review, watch the five reviewers + Judge complete, view findings, switch
presentation style (Neutral / Funny / Insulting), and export the canonical
JSON.

## Human-readable report styles

```bash
paper-guard review paper.tex                      # neutral (default)
paper-guard review paper.tex --style funny
paper-guard review paper.tex --style insulting
paper-guard review paper.tex --output summary     # concise summary
```

Styles are purely presentational — they never alter finding IDs, severity,
confidence, evidence, claims, reviewer identity, Judge decisions, or revision
scope. The canonical RunRecord remains the single source of truth.

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

## User configuration, setup and editable prompts (optional)

Paper Guard works with zero configuration. When you want a per-user setup
(configuration, editable reviewer prompts, rolling logs), run:

```text
paper-guard setup
```

This creates `~/.paper-guard/` (`config/config.toml`, `config/prompts/`,
`logs/`, `data/`) — idempotently and without overwriting existing files.
Every command then automatically prefers the user configuration when present
(`--config` always wins):

```text
paper-guard config show          # effective configuration (secrets redacted)
paper-guard config edit          # open the user config in $VISUAL/$EDITOR
paper-guard prompts list         # which prompt source each reviewer uses
```

Reviewer prompts live in `~/.paper-guard/config/prompts/` as plain Markdown
files (`scientific.md`, `adversarial.md`, …). Edit them and run the next
review — **prompt changes take effect without recompiling Paper Guard**. The
binary always contains the built-in defaults as fallback; missing prompt
files are not an error. Technical logs rotate automatically in
`~/.paper-guard/logs/paper-guard.log` (10 MiB × 5 files, oldest deleted).

---

- Windows users: see [`docs/windows.md`](docs/windows.md)
- Architecture & infrastructure: see [`docs/architecture.md`](docs/architecture.md)
