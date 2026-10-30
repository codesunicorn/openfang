# Changelog

All notable changes to OpenFang will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`code_execute` tool — persistent Python kernel per agent.** Scripts call `call_tool(...)` to invoke file/memory/web tools inline, collapsing a multi-step tool loop (N LLM round-trips, each re-sending the whole conversation) into a single turn. State persists between calls; exceptions return `[error]` without killing the interpreter; nested calls use the same workspace sandbox, tool allowlist and `deny_shell` floor. `shell_exec` is deliberately not available inside the kernel. Config: `OPENFANG_PYTHON`, `OPENFANG_CODE_EXECUTE_TIMEOUT_SECS` (default 120).
- **Prompt-cache metering.** `TokenUsage` now carries `cache_read_tokens` / `cache_creation_tokens`, parsed from OpenAI-compatible (`prompt_tokens_details.cached_tokens`) and Anthropic (`cache_read_input_tokens` / `cache_creation_input_tokens`) responses, persisted in a new `usage_events` column pair (schema v9) and priced with provider-specific discounts (Anthropic 10% reads / 1.25x writes, OpenAI 50% reads). The Anthropic driver now emits `cache_control: ephemeral` breakpoints on the system prompt and final tool definition. Agent-loop completion logs include both counters, so cache-hit rate is finally observable.
- **Session search (`session_search` + `GET /api/sessions/search`).** FTS5 index over every stored message (schema v9, with insert/update/delete triggers), bm25-ranked results with session labels and dates. Lets agents answer "what did we decide about X?" without asking again.
- **Mid-turn steering.** `POST /api/agents/{id}/message` with `"steer": true` queues the text into a running turn; the agent loop appends it as a user turn at its next iteration boundary instead of refusing or starting a competing turn.
- **Channel directory.** Learns a name→id map of reachable chat channels from inbound traffic plus operator aliases, persisted to `channel_directory.json`. New `channel_directory` tool lets agents resolve "#taau" before calling `channel_send`.
- **`[approval] deny_shell` hard floor.** Matching commands are blocked before approvals, before `auto_approve_shell`, and before any exec-policy bypass — nothing overrides it. Both exemption and deny matching now run on a deobfuscated command (`r\m`, `st""us`, `'doc'ker`), so quoting tricks cannot slip past either list.
- **Channel adapter feature gating.** Discord/Telegram/Slack/Matrix always build; the other 36 adapters are behind per-adapter Cargo features (`full-channels` for all, which `openfang-api` enables by default).
- **Curated bundled agents.** `openfang` installs four templates by default (`assistant`, `coder`, `ops`, `researcher`) instead of 30; `OPENFANG_INSTALL_ALL_AGENTS=1` restores the full set. Each installed agent is heartbeated on boot, so the full set cost scheduler/compaction churn for unused specialities.
- **Editable model catalog.** The compiled-in catalog is written to `{data_dir}/models.json` on first boot and preferred over the built-in tables, so model/price updates become data edits instead of rebuilds (malformed files fall back safely).
- **Progressive skill disclosure.** The per-turn skill index is now one truncated line per skill; full instructions and tool lists are fetched on demand via `skill_describe`, instead of inlining everything on every turn.

### Fixed

- **Busy conversations no longer hang silently.** A message arriving mid-turn used to block on an uncontended-looking mutex with no timeout and no feedback — measured at 32s of dead air on a second `POST /api/agents/{id}/message`, and unbounded if the running turn was wedged. The wait is now bounded by `queue_wait_secs` (default 90): past that the call returns `OpenFangError::AgentBusy` immediately, the API answers `503` with `Retry-After` and `retryable: true`, and chat surfaces reply "Still working on your previous message… Send /stop to cancel it." Callers that don't want to wait can send `"steer": true`.
- **Turns have a wall-clock budget.** `turn_timeout_secs` (default 900, `0` disables) cancels a turn that overruns on both the streaming and non-streaming paths and releases the conversation lock, so one hung provider call or runaway tool loop can no longer pin a conversation until restart. API: `504`; channels: "That task ran past its time limit and was stopped."


- Discord approval buttons now acknowledge instantly with interaction type 7 (UPDATE_MESSAGE): the prompt is replaced in place and the buttons removed on tap — no more "tap twice" or re-clicking the same prompt. Verdict lines keep the original prompt text (component interactions now carry `message_content`), verdict latency dropped 5s → 2s, and clicking an expired prompt replies "That approval expired (window: Ns) or was already decided." instead of implying the id was wrong.
- Kernel boot no longer rewrites `default_model` when auto-detecting a provider. When the primary driver init failed, the salvage path built the driver chain AND silently overwrote the operator's declared `provider`/`model` with the detected provider's defaults (e.g. `provider="default"`, `model="opus"` became `openai`/`gpt-4o` whenever `OPENAI_API_KEY` was set) — destroying catalog-alias references (#1188) and making `referenced_providers()` report the salvage instead of the configuration. The salvage driver still enters the chain (runtime behavior unchanged); the declared config stays intact.
- Tool-execution timeout is now 300s (was 120s) for non-agent tools, so it covers the approval window instead of guaranteeing a timeout for late approvals. `OPENFANG_TOOL_TIMEOUT_SECS` still overrides.
- OpenAI-compatible driver gained real deadlines: 30s connect, idle-read watchdog on streams, 300s total per request (`OPENFANG_LLM_REQUEST_TIMEOUT_SECS`, 0 = off). An Anthropic/Gemini wedged upstream now fails the attempt and retries instead of pinning the turn forever.
- `[memory] embedding_provider = "none"` (or `"disabled"`) opts out of vector embeddings; recall falls back to text search instead of retrying a doomed embedding API on every call.
- Bundled agents no longer ship a `max_llm_tokens_per_hour` (300K default, exhausted after a few tool-loop turns); the `ops` and `orchestrator` idle schedules were removed (approval-prompt churn).
- Discord threads the bot is engaged in no longer require an `@mention` per message; replies now carry the replied-to message as context.
- `tts` test is env-hermetic (an `OPENAI_API_KEY` on the host broke it).

## [0.5.10] - 2026-04-17

### Fixed

- Non-loopback requests with no `api_key` configured now return 401 by default. Opt out with `OPENFANG_ALLOW_NO_AUTH=1`. Fixes the B1/B2 authentication bypass from #1034.
- Agent `context.md` is re-read on every turn so external updates take effect mid-session. Opt out per agent with `cache_context = true` on the manifest. Fixes #843.
- `openfang config get default_model.base_url` now prints the configured URL instead of an empty string. Missing keys return a clear "not found" error. Fixes #905.
- `schedule_create`, `schedule_list`, and `schedule_delete` tools plus the `/api/schedules` routes now use the kernel cron scheduler, so scheduled jobs actually fire. One-shot idempotent migration imports legacy shared-memory entries at startup. Fixes #1069.
- Multimodal user messages now combine text and image blocks into a single message so the LLM sees both. Fixes #1043.

### Added

- `openfang hand config <id>` subcommand: get, set, unset, and list settings on an active hand instance. Fixes #809.
- Optional per-channel `prefix_agent_name` setting (`off` / `bracket` / `bold_bracket`). Wraps outbound agent responses so users in multi-agent channels can see which agent replied. Default is off, byte-identical to prior behavior. Fixes #980.

### Closed as invalid

- #818 and #819. Both reference a knowledge-domain API that does not exist on `main`. Filed against an unmerged feature branch (`plan/013-audit-remediation`). Close with a note to build the proposed validation and stale-timestamp surfacing into that feature when it lands.

## [0.5.9] - 2026-04-10

### Changed

- **BREAKING:** Dashboard password hashing switched from SHA256 to Argon2id. Existing `password_hash` values in `config.toml` must be regenerated with `openfang auth hash-password`. Only affects users with `[auth] enabled = true`.

### Fixed

- Dashboard passwords were hashed with plain SHA256 (no salt), making them vulnerable to rainbow table and GPU-accelerated brute force attacks. Now uses Argon2id with random salts.

## [0.1.0] - 2026-02-24

### Added

#### Core Platform
- 15-crate Rust workspace: types, memory, runtime, kernel, api, channels, wire, cli, migrate, skills, hands, extensions, desktop, xtask
- Agent lifecycle management: spawn, list, kill, clone, mode switching (Full/Assist/Observe)
- SQLite-backed memory substrate with structured KV, semantic recall, vector embeddings
- 41 built-in tools (filesystem, web, shell, browser, scheduling, collaboration, image analysis, inter-agent, TTS, media)
- WASM sandbox with dual metering (fuel + epoch interruption with watchdog thread)
- Workflow engine with pipelines, fan-out parallelism, conditional steps, loops, and variable expansion
- Visual workflow builder with drag-and-drop node graph, 7 node types, and TOML export
- Trigger system with event pattern matching, content filters, and fire limits
- Event bus with publish/subscribe and correlation IDs
- 7 Hands packages for autonomous agent actions

#### LLM Support
- 3 native LLM drivers: Anthropic, Google Gemini, OpenAI-compatible
- 27 providers: Anthropic, Gemini, OpenAI, Groq, OpenRouter, DeepSeek, Together, Mistral, Fireworks, Cohere, Perplexity, xAI, AI21, Cerebras, SambaNova, Hugging Face, Replicate, Ollama, vLLM, LM Studio, and more
- Model catalog with 130+ built-in models, 23 aliases, tier classification
- Intelligent model routing with task complexity scoring
- Fallback driver for automatic failover between providers
- Cost estimation and metering engine with per-model pricing
- Streaming support (SSE) across all drivers

#### Token Management & Context
- Token-aware session compaction (chars/4 heuristic, triggers at 70% context capacity)
- In-loop emergency trimming at 70%/90% thresholds with summary injection
- Tool profile filtering (cuts default 41 tools to 4-10 for chat agents, saving 15-20K tokens)
- Context budget allocation for system prompt, tools, history, and response
- MAX_TOOL_RESULT_CHARS reduced from 50K to 15K to prevent tool result bloat
- Default token quota raised from 100K to 1M per hour

#### Security
- Capability-based access control with privilege escalation prevention
- Path traversal protection in all file tools
- SSRF protection blocking private IPs and cloud metadata endpoints
- Ed25519 signed agent manifests
- Merkle hash chain audit trail with tamper detection
- Information flow taint tracking
- HMAC-SHA256 mutual authentication for peer wire protocol
- API key authentication with Bearer token
- GCRA rate limiter with cost-aware token buckets
- Security headers middleware (CSP, X-Frame-Options, HSTS)
- Secret zeroization on all API key fields
- Subprocess environment isolation
- Health endpoint redaction (public minimal, auth full)
- Loop guard with SHA256-based detection and circuit breaker thresholds
- Session repair (validates and fixes orphaned tool results, empty messages)

#### Channels
- 40 channel adapters: Telegram, Discord, Slack, WhatsApp, Signal, Matrix, Email, Teams, Mattermost, Google Chat, Webex, Feishu/Lark, LINE, Viber, Facebook Messenger, Mastodon, Bluesky, Reddit, LinkedIn, Twitch, IRC, XMPP, and 18 more
- Unified bridge with agent routing, command handling, message splitting
- Per-channel user filtering and RBAC enforcement
- Graceful shutdown, exponential backoff, secret zeroization on all adapters

#### API
- 100+ REST/WS/SSE API endpoints (axum 0.8)
- WebSocket real-time streaming with per-agent connections
- OpenAI-compatible `/v1/chat/completions` API (streaming SSE + non-streaming)
- OpenAI-compatible `/v1/models` endpoint
- WebChat embedded UI with Alpine.js
- Google A2A protocol support (agent card, task send/get/cancel)
- Prometheus text-format `/api/metrics` endpoint for monitoring
- Multi-session management: list, create, switch, label sessions per agent
- Usage analytics: summary, by-model, daily breakdown
- Config hot-reload via polling (30-second interval, no restart required)

#### Web UI
- Chat message search with Ctrl+F, real-time filtering, text highlighting
- Voice input with hold-to-record mic button (WebM/Opus codec)
- TTS audio playback inline in tool cards
- Browser screenshot rendering in chat (inline images)
- Canvas rendering with iframe sandbox and CSP support
- Session switcher dropdown in chat header
- 6-step first-run setup wizard with provider API key help (12 providers)
- Skill marketplace with 4 tabs (Installed, ClawHub, MCP Servers, Quick Start)
- Copy-to-clipboard on messages, message timestamps
- Visual workflow builder with drag-and-drop canvas

#### Client SDKs
- JavaScript SDK (`@openfang/sdk`): full REST API client with streaming, TypeScript declarations
- Python client SDK (`openfang_client`): zero-dependency stdlib client with SSE streaming
- Python agent SDK (`openfang_sdk`): decorator-based framework for writing Python agents
- Usage examples for both languages (basic + streaming)

#### CLI
- 14+ subcommands: init, start, agent, workflow, trigger, migrate, skill, channel, config, chat, status, doctor, dashboard, mcp
- Daemon auto-detection via PID file
- Shell completion generation (bash, zsh, fish, PowerShell)
- MCP server mode for IDE integration

#### Skills Ecosystem
- 60 bundled skills across 14 categories
- Skill registry with TOML manifests
- 4 runtimes: Python, Node.js, WASM, PromptOnly
- FangHub marketplace with search/install
- ClawHub client for OpenClaw skill compatibility
- SKILL.md parser with auto-conversion
- SHA256 checksum verification
- Prompt injection scanning on skill content

#### Desktop App
- Tauri 2.0 native desktop app
- System tray with status and quick actions
- Single-instance enforcement
- Hide-to-tray on close
- Updated CSP for media, frame, and blob sources

#### Session Management
- LLM-based session compaction with token-aware triggers
- Multi-session per agent with named labels
- Session switching via API and UI
- Cross-channel canonical sessions
- Extended chat commands: `/new`, `/compact`, `/model`, `/stop`, `/usage`, `/think`

#### Image Support
- `ContentBlock::Image` with base64 inline data
- Media type validation (png, jpeg, gif, webp only)
- 5MB size limit enforcement
- Mapped to all 3 native LLM drivers

#### Usage Tracking
- Per-response cost estimation with model-aware pricing
- Usage footer in WebSocket responses and WebChat UI
- Usage events persisted to SQLite
- Quota enforcement with hourly windows

#### Interoperability
- OpenClaw migration engine (YAML/JSON5 to TOML)
- MCP client (JSON-RPC 2.0 over stdio/SSE, tool namespacing)
- MCP server (exposes OpenFang tools via MCP protocol)
- A2A protocol client and server
- Tool name compatibility mappings (21 OpenClaw tool names)

#### Infrastructure
- Multi-stage Dockerfile (debian:bookworm-slim runtime)
- docker-compose.yml with volume persistence
- GitHub Actions CI (check, test, clippy, format)
- GitHub Actions release (multi-platform, GHCR push, SHA256 checksums)
- Cross-platform install script (curl/irm one-liner)
- systemd service file for Linux deployment

#### Multi-User
- RBAC with Owner/Admin/User/Viewer roles
- Channel identity resolution
- Per-user authorization checks
- Device pairing and approval system

#### Production Readiness
- 1731+ tests across 15 crates, 0 failures
- Cross-platform support (Linux, macOS, Windows)
- Graceful shutdown with signal handling (SIGINT/SIGTERM on Unix, Ctrl+C on Windows)
- Daemon PID file with stale process detection
- Release profile with LTO, single codegen unit, symbol stripping
- Prometheus metrics for monitoring
- Config hot-reload without restart

[0.1.0]: https://github.com/RightNow-AI/openfang/releases/tag/v0.1.0
