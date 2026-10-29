# Changelog

All notable changes to OpenFang will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `[approval] auto_approve_shell` — per-command approval exemptions for read-only shell commands (`"uptime"`, `"df *"`, `"free *"`, `"systemctl status *"`...). Matching commands run without a human prompt; chained/piped/redirected commands (`;`, `&&`, `||`, `|`, `>`, `<`, `` ` ``, `$(`) never auto-approve and always prompt. Intended for scheduled monitoring agents that otherwise spam approval buttons for non-destructive health checks. Empty by default.
- `[memory] embedding_provider = "none"` (or `"disabled"`) explicitly turns off vector embeddings — recall falls back to text search instead of retrying a doomed embedding API (e.g. no embedding credentials on the machine) on every call. `[memory]` embedding settings documented in configuration.md along with a new `[approval]` section.
- Discord: threads the bot is engaged in no longer require an @mention on every message. Messages inside a known thread (created by the bot, replied into by the bot, or discovered on the send path) are flagged `in_bot_thread` and pass the `mention_only` group policy automatically. Each channel the bot posts to is classified once via `GET /channels/{id}` (cached), so human-created threads — and threads created before a daemon restart — become auto-engaged once the bot replies there. Threads the bot has never been part of stay mention-gated.
- Discord: replies now carry reply context. The replied-to message is prepended to the prompt (`[Replying to <name>: <text>]`, Telegram parity), and `reply_to_message_id` is recorded in message metadata — including for parents with no text (sticker/attachment-only). Commands keep their argument structure.

### Fixed

- Discord approval buttons now acknowledge instantly with interaction type 7 (UPDATE_MESSAGE): the prompt is replaced in place and the buttons removed on tap — no more "tap twice" or re-clicking the same prompt. Verdict lines keep the original prompt text (component interactions now carry `message_content`), verdict latency dropped 5s → 2s, and clicking an expired prompt replies "That approval expired (window: Ns) or was already decided." instead of implying the id was wrong.
- Kernel boot no longer rewrites `default_model` when auto-detecting a provider. When the primary driver init failed, the salvage path built the driver chain AND silently overwrote the operator's declared `provider`/`model` with the detected provider's defaults (e.g. `provider="default"`, `model="opus"` became `openai`/`gpt-4o` whenever `OPENAI_API_KEY` was set) — destroying catalog-alias references (#1188) and making `referenced_providers()` report the salvage instead of the configuration. The salvage driver still enters the chain (runtime behavior unchanged); the declared config stays intact.
- `PUT /api/budget/agents/{id}` now takes effect at runtime immediately. Previously it updated the registry manifest (and persisted it) but never propagated to the scheduler's quota map that `check_quota` reads, so budget changes were silently a no-op until the agent restarted. Usage already consumed this window is preserved (no reset).
- Bundled agent manifests no longer ship `max_llm_tokens_per_hour` (values ranged 50K–500K). One multi-tool turn can consume 140K+ input tokens, so the previous limits silenced agents after a handful of messages for up to an hour. Absent field = unlimited; `[budget] default_max_llm_tokens_per_hour` in config.toml still overrides when set.
- Default tool-execution timeout raised from 120s to 300s (`OPENFANG_TOOL_TIMEOUT_SECS` still overrides; `0` disables). The old default was shorter than the maximum approval window (`[approval] timeout_secs`, capped at 300s), so every approval click made between 2 and 5 minutes guaranteed a tool timeout.

### Changed

- OpenAI-compatible driver now enforces an LLM request deadline (default 300s, env `OPENFANG_LLM_REQUEST_TIMEOUT_SECS`, `0` disables): a 30s connect timeout, an idle-read timeout on streams, and a per-request total timeout on non-streaming completions. A wedged upstream now fails the attempt and retries instead of pinning the agent turn (and the user's "thinking" preview) indefinitely.

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
