# axon-ai — LLM backends for Axon's `ai_complete` / `ai_extract_*`

Axon's AI builtins call a live model when built with `--features asi-runtime`
(otherwise they mock under `AXON_AI_MOCK=1` or require an `@[ai(policy(fallback))]`).
This crate is the live backend. It speaks **two wire formats**, so you can point
Axon at Anthropic, an Anthropic-compatible gateway, or any OpenAI-compatible
endpoint (NVIDIA NIM, OpenRouter, vLLM, LiteLLM, Together, Groq, …).

## Quick start

### Anthropic (default)
```bash
export ANTHROPIC_API_KEY=sk-ant-...
axon-run run prog.ax          # built with --features asi-runtime
```

### An Anthropic-compatible gateway / proxy (e.g. a trainloop gateway, Helicone)
No code change — just repoint the base URL. The gateway must speak Anthropic's
`/v1/messages` shape.
```bash
export ANTHROPIC_BASE_URL=https://your-gateway.internal
export ANTHROPIC_API_KEY=...
```

### OpenAI-compatible backends (NVIDIA NIM, OpenRouter, vLLM, …)
```bash
export AXON_AI_PROVIDER=openai
export AXON_AI_BASE_URL=https://integrate.api.nvidia.com   # NIM example
export AXON_AI_API_KEY=nvapi-...
export AXON_AI_MODEL_BALANCED="meta/llama-3.1-70b-instruct" # backend's model id
```
OpenRouter:
```bash
export AXON_AI_PROVIDER=openai
export AXON_AI_BASE_URL=https://openrouter.ai/api
export AXON_AI_API_KEY=sk-or-...
export AXON_AI_MODEL_BALANCED="anthropic/claude-sonnet-4-6"
```

## Configuration reference

| Env var | Effect |
|---|---|
| `AXON_AI_PROVIDER` | `anthropic` (default) or `openai`. Selects the wire format. Unknown → Anthropic. |
| `AXON_AI_BASE_URL` | Base URL for the active provider. Falls back to `ANTHROPIC_BASE_URL`, then the provider default (`api.anthropic.com` / `api.openai.com`). Trailing slash tolerated; the provider's path (`/v1/messages` or `/v1/chat/completions`) is appended. |
| `AXON_AI_API_KEY` | API key. Falls back to `ANTHROPIC_API_KEY`. Sent as `x-api-key` (Anthropic) or `Authorization: Bearer` (OpenAI). |
| `AXON_AI_MODEL_{CHEAP,BALANCED,STRONG}` | Per-tier model id. Set these to the backend's model strings when not on Anthropic. |
| `ANTHROPIC_BASE_URL` / `ANTHROPIC_API_KEY` | Legacy names, still honored (back-compat). The neutral `AXON_AI_*` names win when both are set. |
| `AXON_AI_MOCK=1` | Deterministic offline stub — no key, no network. Used by demos/CI. |

### `.env` files

Secrets can live in a `.env` file instead of your shell. On the first AI-config
read, axon-ai loads `.env` from `$AXON_DOTENV` (if set), else the nearest `.env`
walking up from the current directory to the filesystem root. Format:

```dotenv
# .env
AXON_AI_PROVIDER=openai
AXON_AI_BASE_URL=https://integrate.api.nvidia.com
AXON_AI_API_KEY="nvapi-..."
AXON_AI_MODEL_BALANCED=meta/llama-3.1-70b-instruct
```

`#` comments, blank lines, an optional `export ` prefix, and surrounding quotes
are handled. **A variable already set in the real environment is never
overwritten** — the shell wins over the file. Keep `.env` out of version control.

## Wire-format differences (handled for you)

| | Anthropic | OpenAI-compatible |
|---|---|---|
| Path | `/v1/messages` | `/v1/chat/completions` |
| Auth header | `x-api-key` + `anthropic-version` | `Authorization: Bearer` |
| Reply text | `content[0].text` | `choices[0].message.content` |
| Token usage | `usage.{input,output}_tokens` | `usage.{prompt,completion,total}_tokens` |
| Structured output (`ai_extract`) | `tools[].input_schema` + `tool_use` block | `tools[].function.parameters` + `tool_calls[].function.arguments` (JSON string) |

The same Axon program runs unchanged across providers — only the environment
differs.
