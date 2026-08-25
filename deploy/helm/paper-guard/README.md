# Paper Guard Helm Chart

Deploys the **Paper Guard** HTTP service into Kubernetes. The chart packages
only the Paper Guard service — it does **not** bundle Qdrant or Ollama. Those
are configured as external endpoints (local, Kubernetes, or managed), so the
topology stays configurable.

## Install

```bash
helm install paper-guard deploy/helm/paper-guard \
  --set llm.endpoint=https://api.openai.com/v1 \
  --set llm.model=gpt-4o-mini \
  --set llm.apiKeySecretName=paper-guard-api-key
```

Provide the API key out-of-band (never in `values.yaml`):

```bash
kubectl create secret generic paper-guard-api-key \
  --from-literal=api-key='sk-...' \
  --dry-run=client -o yaml | kubectl apply -f -
```

### Local Ollama (keyless)

```bash
helm install paper-guard deploy/helm/paper-guard \
  --set llm.provider=openai-compatible \
  --set llm.endpoint=http://ollama:11434/v1 \
  --set llm.model=llama3.2
```

`apiKeySecretName` is left empty, so requests are sent without an
Authorization header (local Ollama needs no key). The example cluster topology:

```
Kubernetes
├── paper-guard (this chart)
├── ollama      (deployed separately)
└── qdrant      (deployed separately or managed)
```

### Mammoth.ai or another OpenAI-compatible endpoint

Changing backends is purely a configuration change:

```bash
--set llm.endpoint=https://mammoth.ai/v1 \
--set llm.model=<your-model>
```

## Secrets

API keys are **never** placed in `values.yaml` or the ConfigMap. They live in a
Kubernetes Secret referenced by name. Either:
- create the Secret out-of-band (`kubectl create secret …`), or
- set `llm.apiKeySecretName` (the chart creates an empty placeholder Secret
  that you populate, or that an external tool like sealed-secrets injects).

## Configuration

| Parameter | Description | Default |
|-----------|-------------|---------|
| `replicas` | Service replicas | `1` |
| `image.repository` / `image.tag` | Container image | `paper-guard/paper-guard` / chart version |
| `service.type` / `service.port` / `service.targetPort` | Service exposure | `ClusterIP` / `80` / `8080` |
| `llm.provider` | `mock` or `openai-compatible` | `openai-compatible` |
| `llm.endpoint` | OpenAI-compatible base URL | `https://api.openai.com/v1` |
| `llm.model` | Model name (config-driven) | `gpt-4o-mini` |
| `llm.structuredOutput` / `llm.vision` | Capability flags | `true` / `false` |
| `llm.apiKeySecretName` / `llm.apiKeySecretKey` | Secret + key for the API key | `""` / `api-key` |
| `qdrant.endpoint` / `qdrant.collection` | Review Memory vector backend | `http://qdrant:6333` / `review_memory` |
| `persistence.enabled` / `persistence.size` | Persistent storage for artifacts | `true` / `1Gi` |
| `logging.level` | Log level | `info` |

## Security notes

- The service refuses to bind to a non-loopback address unless
  `allow_external_bind = true` is explicitly set; the chart sets this because
  the container runs behind the Kubernetes Service and probes, but you should
  still restrict access via NetworkPolicies / the Service type.
- Uploaded manuscripts are untrusted data; they are never logged.
- Paper Guard does **not** generate scientific papers and does **not**
  automatically use papers for training. Memory is retrieval-based and
  requires explicit human approval (`PRIVATE` by default).

## Uninstall

```bash
helm uninstall paper-guard
```
