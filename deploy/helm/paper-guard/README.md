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

## LAN discovery (mDNS/DNS-SD) — optional and off by default

Paper Guard supports provider-independent LAN discovery via mDNS/DNS-SD so that
clients on the same network can find the service without knowing the node IP,
Service/NodePort, or Ingress address.

By default discovery is **disabled** — the chart neither probes the network nor
advertises anything. The Paper Guard application container **never** runs an
mDNS/Avahi daemon and never requires `hostNetwork`, `serviceAccount`, or
privileged networking. The app stays unprivileged and independent of mDNS.

### Enable client-side discovery in the appConfig

```bash
helm install paper-guard deploy/helm/paper-guard \
  --set discovery.enabled=true \
  --set discovery.mode=manual
```

This writes a `[discovery]` section into the service's `paper-guard.toml`
(`manual` lists/verifies services; `auto` may select one, but only with explicit
user confirmation before any manuscript is transmitted — discovery never
authorises an upload).

### Optional publisher pod (separate infrastructure component)

If you also want the service *advertised* on the LAN (so clients can reach it by
hostname), deploy a **separate** mDNS publisher pod. It is a distinct Deployment
with its own security context, never merged into the Paper Guard container:

```bash
helm install paper-guard deploy/helm/paper-guard \
  --set discovery.publisher.enabled=true \
  --set discovery.publisher.repository=your/mdns-bridge
```

The publisher advertises `paper-guard.local` (see `discovery.hostname`, default
`paper-guard.local`) using the DNS-SD service type `_paper-guard._tcp.local.`
and a TXT `version` equal to the chart `appVersion`. Because mDNS requires
multicast, the publisher pod may need `hostNetwork`/`NET_ADMIN`+`NET_RAW`; those
capabilities are scoped to that pod only and are **never** requested by the
Paper Guard app.

### Discovery ≠ authorization

Discovery only tells a client "a Paper Guard service exists". It does **not**
grant permission to submit a manuscript. Paper Guard never sends a manuscript to
a discovered service unless remote execution has been explicitly selected.

### Network assumptions

mDNS operates within the local multicast domain. It may not work across routed
networks, VPNs, VLAN boundaries, Wi-Fi client isolation, firewalls blocking
multicast, or Kubernetes CNI boundaries. Paper Guard fails gracefully when mDNS
is unavailable (an empty result is not an error).
