# Paper Guard on Windows — Team Client Quickstart

This guide shows a Windows researcher how to use Paper Guard as a **self-contained
command-line tool** without installing Rust, Cargo, Python, Docker, WSL,
Kubernetes, or a development environment.

> Paper Guard reviews an **already-written** paper. It never writes the paper.

---

## 1. Installation

1. Download the latest release archive for Windows x86_64:

   ```
   paper-guard-1.0.0-x86_64-pc-windows-msvc.zip
   ```

2. Extract it anywhere (e.g. `C:\Users\<You>\PaperGuard\`).

   The archive contains:

   ```
   paper-guard.exe        the Paper Guard binary
   QUICKSTART.md          this quick reference
   LICENSE                the Apache-2.0 license
   ```

3. Verify the binary runs:

   ```powershell
   .\paper-guard.exe --version
   ```

   You can now use Paper Guard from a PowerShell or `cmd.exe` terminal. There is
   no installer and nothing to configure before first use.

---

## 2. What Paper Guard can and cannot do

Paper Guard is a **review and revision-quality system**. It can:

- `review` an existing paper (local, offline)
- `discover` the team service on your local network
- connect to a configured remote Paper Guard service
- submit a manuscript to that service (only on explicit remote review)
- retrieve findings
- submit human feedback
- manage **approved** review memory

It must **never**:

- invent results, references, experiments, or scientific evidence
- generate a paper from scratch
- silently modify or upload a manuscript

The **only** command that transmits manuscript content is an explicit remote
review. Discovery and health checks never upload your paper.

---

## 3. Local review (no network)

You can review a paper entirely on your machine using the deterministic mock
provider (no server, no Ollama, no internet):

```powershell
.\paper-guard.exe review paper.tex
```

This parses the paper, runs the review agents, judges, and records findings in
the local ledger under a `data_dir` (default `.paper-guard`). Results are
printed to the terminal and persisted as structured JSON.

> Paths with spaces and Unicode filenames work. Paper Guard uses Rust's native
> filesystem APIs, never shell escaping.

To check what the tool remembered:

```powershell
.\paper-guard.exe findings
```

---

## 4. Configure an OpenAI-compatible provider (optional)

If you want real review agents (rather than the deterministic mock), create a
configuration file and point it at any **OpenAI-compatible** endpoint —
OpenAI, Ollama, Mammoth.ai, or a local server.

```powershell
.\paper-guard.exe init paper-guard.toml
```

Then edit `paper-guard.toml` and select the provider:

```toml
[llm]
provider = "openai-compatible"

[providers.openai-compatible]
base_url = "http://localhost:11434/v1"   # Ollama, or any OpenAI-compatible endpoint
api_key_env = "OPENAI_API_KEY"           # optional; Ollama needs no key
model = "llama3.1"
# Structured output only constrains the JSON transport shape; it does NOT make
# an LLM scientifically trustworthy (scientific validity is still enforced by
# domain validation, evidence checks, provenance, Judge, and integrity guards).
#   true / "json_object"  -> {"type":"json_object"} (default)
#   "json_schema"         -> strict JSON Schema (recommended for Ollama / LM Studio)
#   false / "off"         -> free-form; reviewer-side validation still enforces JSON
structured_output = "json_schema"        # true | false | "json_object" | "json_schema"
```

*Secrets are never stored in the config.* Only the **name** of an environment
variable is stored; the value is read from the environment at request time:

```powershell
$env:OPENAI_API_KEY = "your-key-here"
.\paper-guard.exe --config paper-guard.toml review paper.tex
```

> No token is ever written to the Windows Registry, to logs, to CLI arguments,
> or to any plaintext file.

---

## 5. Discover the team service

If your team runs a Paper Guard service on the local network, let the client
find it automatically via mDNS/DNS-SD:

```powershell
.\paper-guard.exe discover --force
```

This broadcasts a one-shot browse for `_paper-guard._tcp` services, lists the
candidates it finds, and verifies each by calling its `/health` endpoint.

> **Discovery does not upload the paper.** It only announces "is any Paper
> Guard service here?" and checks health. Use `--force` for a one-off browse;
> otherwise enable `[discovery]` in the config.

If nothing is found you will see:

```
No Paper Guard services found on the local network.
```

This is normal when:

- the team service is off the network,
- your network blocks multicast / mDNS,
- the Windows Firewall blocks discovery, or
- Wi-Fi client isolation is enabled.

The tool fails **gracefully** — it does not crash and does not fall back to an
arbitrary server. Use an explicit `--server` URL when you know the address.

### Windows Firewall

Paper Guard never modifies the Windows Firewall and never requires
administrator privileges. For *discovery* to work, your network should permit
outbound multicast (UDP port 5353); on most networks this is already allowed.
If you only review locally or connect via an explicit `--server` address, you
do not need any firewall change.

---

## 6. Remote review against the team service

Once you know the service is reachable, run an explicit remote review:

```powershell
.\paper-guard.exe review C:\Users\Researcher\Documents\Paper\paper.tex --server http://paper-guard.local:8080
```

or, with the server stored in your config:

```toml
[server]
url = "http://paper-guard.local:8080"
auth_token_env = "PAPER_GUARD_TOKEN"   # optional, name only
timeout_seconds = 120
```

```powershell
.\paper-guard.exe --config paper-guard.toml review paper.pdf
```

Only this explicit remote review sends manuscript content to the service. The
server's authoritative ledger records the run; the client never invents a local
ledger entry for a remote review.

---

## 7. Local web GUI

Paper Guard v1.0 includes a small local web GUI. Start it with:

```powershell
.\paper-guard.exe --gui
```

Paper Guard binds the GUI to `127.0.0.1` by default (never the LAN), prints
the local URL (e.g. `http://127.0.0.1:8080`), and opens your default browser.

From the GUI you can:

- see the Paper Guard version, configured LLM provider, and current config
- select a `.tex` or `.pdf` paper and start a review
- watch the five reviewers (Scientific, Adversarial, Evidence, References,
  Figures) and the Judge complete
- view findings (severity, confidence, evidence, affected claim, source
  location)
- filter by reviewer, severity, finding category, or status
- switch the presentation style (Neutral / Funny / Insulting) — this is purely
  presentational and never triggers another LLM request
- export the canonical JSON `RunRecord`

No API keys are stored in the browser, no paper content is uploaded to any
third-party service, and the GUI cannot modify findings or bypass integrity
rules. Any mutation you perform goes through the same domain/API layer as the
CLI.

---

## 8. Retrieve findings and provide feedback

After a review, list the findings:

```powershell
.\paper-guard.exe findings
```

Record your human decision on a finding (accepted / rejected / modified):

```powershell
.\paper-guard.exe feedback <RUN_ID> <FINDING_ID> --decision accept --feedback "Looks correct."
```

Feedback is stored as a **private** Review Memory candidate (approved for
retrieval only by a human).

---

## 8. Diagnostics

To see which version, platform, and build you are running (never secrets):

```powershell
.\paper-guard.exe info
.\paper-guard.exe diagnostics --paths
```

---

## 9. Where files live on Windows

Paper Guard keeps concerns separated so your manuscripts are never written to
arbitrary locations. Defaults (overridable via your config):

| Concern       | Windows location                                    |
|---------------|-----------------------------------------------------|
| Config        | `%APPDATA%\paper-guard\`                            |
| Data          | `%APPDATA%\paper-guard\` (or your `data_dir`)       |
| Cache         | `%LOCALAPPDATA%\paper-guard\cache\`                 |
| Logs          | `%LOCALAPPDATA%\paper-guard\logs\`                  |

Manuscripts are only ever read from where you point `review` at them; they are
never copied into logs, cache, or telemetry automatically.

---

## 10. Verify the release checksum

Verify the downloaded executable is exactly the CI-produced artifact:

```powershell
Get-FileHash .\paper-guard.exe -Algorithm SHA256
```

Compare the result against the `SHA256SUMS` file included in the release. The
release archive and checksums are produced reproducibly by CI from the tagged
Git commit — the binary is never hand-copied from a developer machine.

---

## 11. Uninstall

Paper Guard is a single executable. To remove it, delete the folder you
extracted it into. If you created a config or data directory under your
`%APPDATA%`, you may delete those too.

---

For people who maintain the service or infrastructure, see
[`docs/architecture.md`](architecture.md).
