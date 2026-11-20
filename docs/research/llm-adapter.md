# LLM adapter layer: what to build on

_2026-11-19. Question (Flemming): which platform abstraction/adapter should the LLM integration use,
ideally one that also supports Claude Code subscriptions? Status: **research; recommendation at the
end** (own trait + two adapter families, not one framework). Related: STATUS next steps "LLM integration",
[`../STATUS.md`](../STATUS.md)._

Prices, quotas and ToS below were read on 2026-11-18/19; free tiers move monthly, so re-check before
hardcoding any of it.

## 1. "Adapter" means two different things here

Two layers get called an adapter, and they have different shapes:

| | Completion / API layer | Agent / subscription layer |
|---|---|---|
| Unit | one request → one response (streamed) | a run: prompt → tool calls → files → final message |
| Auth | API key (or OAuth token) | an installed CLI's own login, i.e. a plan |
| Good for | "write this patch", "read this analysis JSON" | "use my Claude/ChatGPT/Google/Copilot plan" |
| Examples | genai, rig, async-openai, OpenAI-compatible HTTP | `claude -p`, `codex exec`, ACP, agent-harness |

ScriptFX needs the **first** shape: a single, validated answer (a Luau patch or a chat reply), not an
agent that edits files. The **second** shape is only interesting as a quota source — the user's plan
pays instead of their API key. Any design that conflates them ends up dragging an agent loop and file
permissions into the plugin for no reason.

## 2. Rust crates

| Crate | Shape | What it covers | Subscription auth | Maturity (crates.io, 2026-11-19) |
|---|---|---|---|---|
| [`genai`](https://crates.io/crates/genai) 0.7.5 | completions | ~40 providers: OpenAI, Anthropic, Gemini, Groq, DeepSeek, OpenRouter, Ollama, custom OpenAI-compatible (`genai_{n}`), plus `github_copilot` and `opencode_go` adapters; tools + structured output + streaming | Copilot, OpenCode Go | 313k recent downloads, MIT, one maintainer, releases monthly |
| [`rig-core`](https://crates.io/crates/rig-core) 0.46 | framework | 20+ providers, agents, tools, RAG/vector stores, embeddings, OpenTelemetry | none | 2.2M recent, MIT, large surface, API still moving (`rig` is now also a CLI) |
| [`async-openai`](https://crates.io/crates/async-openai) 0.43 | client | OpenAI + OpenAI-compatible base URLs (Groq, OpenRouter, Ollama, llama.cpp, Gemini's OpenAI endpoint) | none | 3.6M recent, MIT, typed and stable |
| [`llm`](https://crates.io/crates/llm) 1.3.8 | completions | several backends | none | 21k recent, last release Apr 2026 |
| [`agent-harness`](https://crates.io/crates/agent-harness) 0.6.0-alpha.2 | both | Claude Code, Codex, ACP agents (OpenCode, Gemini), OpenAI-compatible/Ollama, all behind one `RunEvent` stream and a `Harness` trait | Claude, Codex, via CLIs | **656 downloads, alpha**, MIT/Apache, API changes |
| [`agent-client-protocol`](https://crates.io/crates/agent-client-protocol) 5.4.0 | agent protocol | Rust SDK for ACP: ~50 agents incl. Claude (via adapter), Codex, Gemini CLI, OpenCode, Copilot (preview), Cursor, Qwen Code, Goose | whatever the agent uses | 2.6M recent, Apache/MIT, releases weekly |

Notes:

- `genai` is the closest thing to "one API for every API-key provider" without owning your loop. Its
  `github_copilot` and `opencode_go` adapters show it is willing to model plan-backed providers.
- `rig` is a good framework but it is a framework: it wants to own the agent, the prompt, the tool
  loop and the transport. For two calls from a plugin that is a lot of dependency and churn.
- `agent-harness` is precisely the shape asked for ("coding agents + local models behind one trait",
  normalized events, discovery of what is installed). It is also 0.6-alpha with under a thousand
  downloads: worth reading, not worth depending on yet.
- ACP is a *protocol*, not a client: the agent brings its own tools and asks permission to run them.
  That is the right protocol if you later want "any installed coding agent", and the wrong one for
  "give me one JSON object".

## 3. Subscription routes (the actual question)

| Plan | How a third-party program can use it | Structured output | Notes |
|---|---|---|---|
| **Claude Pro/Max** | spawn the installed `claude` CLI: `claude -p --output-format json --json-schema '<schema>'`; or the Agent SDK (Python/TS); or ACP via `claude-agent-acp` | yes, `--json-schema` is validated by the CLI | The docs say: *"Unless previously approved, Anthropic does not allow third party developers to offer claude.ai login or rate limits for their products, including agents built on the Claude Agent SDK."* |
| **ChatGPT Plus/Pro** | `codex exec --json --output-schema schema.json` reuses the CLI login; also documented for CI | yes, `--output-schema` | OpenAI documents ChatGPT-managed auth for automation; no equivalent prohibition found |
| **Google account (Gemini CLI)** | was 1,000 req/day; **replaced by Antigravity CLI on 2026-06-18** for free/Google One users; Gemini CLI remains for API keys and Workspace/Code Assist | via the agent | Gemini CLI still speaks ACP; quota docs and Google's pages disagree — verify before relying |
| **GitHub Copilot** | `genai` has a `github_copilot` adapter; Copilot CLI speaks ACP (public preview) | via the adapter | check Copilot's terms for embedding |
| **OpenCode Go / Zen free** | `opencode_go` adapter in `genai` for Go; the free Zen model is app-only, so only reachable by driving the `opencode` CLI/ACP | via the agent | free tier is promotional and explicitly "inside OpenCode" |
| **Cursor / others** | ACP, if the vendor ships it | via the agent | per-vendor terms |

The Claude situation is the one to be explicit about:

- **Technically it works**, and Anthropic has repeatedly said plan-backed programmatic use is
  allowed: Help Center, Oct 7 2026 — "when you're signed in with your Claude plan, the Claude Agent
  SDK and `claude -p` still draw from your subscription limits." Third-party apps built on the Agent
  SDK were explicitly reinstated in May 2026 (drawing from a monthly Agent SDK credit, later paused).
- **For a distributed product** the login/rate-limit clause still stands: shipping "sign in with
  Claude" or "use your Claude plan" needs Anthropic's approval. The safe pattern is the one Zed's
  `claude-agent-acp` uses: require the user's own installed and already-authenticated Claude Code,
  never handle their credentials, never show a claude.ai login. Even then, treat it as gray until
  Anthropic says otherwise — and it is fine without qualification for Flemming's own machine.
- `--bare` disables OAuth/keychain auth (API key only). Subscription runs must **not** pass `--bare`.

## 4. Recommendation

Do **not** adopt a single framework that owns the integration. Define a small trait in a new
`crates/llm` and implement two adapter families behind it:

```rust
/// One request, optional streaming, optional schema.
trait Backend {
    fn id(&self) -> &str;
    fn capabilities(&self) -> Capabilities;     // tools, json_schema, streaming, vision
    fn complete(&self, req: Request, sink: &mut dyn EventSink) -> Result<Response>;
}

// Family A: HTTP, API keys
struct OpenAiCompat { base_url, key, model }   // Gemini/Groq/OpenRouter/DeepSeek/Ollama/Zen
struct Anthropic   { key, model }              // optional, native tools/caching

// Family B: installed CLIs, subscriptions
struct ClaudeCli { bin: PathBuf }   // claude -p --output-format json --json-schema
struct CodexCli  { bin: PathBuf }   // codex exec --json --output-schema
// later: struct Acp { agent: AcpAgent }  // agent-client-protocol, any installed agent
```

Why this shape:

1. **The domain is the schema, not the agent.** `--json-schema` / `--output-schema` return exactly
   the object the loader wants, so the generate → strict-load → feed-error-back loop stays simple and
   identical across providers.
2. **The tools are yours, whoever runs them.** Patch work needs file access, so the run is an agent
   loop either way. The plugin still owns the tool surface (`read_patch`, `write_patch`,
   `validate_patch`, …); an HTTP backend executes them in-process, a CLI backend may use its own
   equivalents or the same ones over MCP. See §5.
3. **HTTP layer is a commodity.** Take `genai` if the dependency is acceptable (it brings reqwest +
   TLS and native adapters, and already models Copilot and OpenCode Go); otherwise a ~300-line
   OpenAI-compatible client on `reqwest`/`ureq` covers every key-based provider. `rig` buys agents
   and RAG that this project does not need.
4. **ACP is the growth path, not the first step.** If the product later wants "pick any installed
   agent" with streaming tool UI, `agent-client-protocol` 5.x is the mature, well-adopted way to add
   ~50 agents at once. Its model (sessions, permissions, file edits) is heavier than a completion, so
   it should sit beside the trait, not under it.
5. **`agent-harness` is the reference implementation to copy, not a dependency** until it is past
   alpha. Its `RunEvent` vocabulary (text delta, reasoning, tool call, plan, usage, exited) is a good
   target for `EventSink`.

Integration rules for the plugin:

- Store keys/tokens in the OS keychain (`keyring`), **never** in plugin state — state travels inside
  the DAW project and would leak keys to anyone who opens it.
- Run everything on a worker thread with a channel into the shell (same pattern as Listen); the audio
  thread never allocates or touches the network, and the UI never blocks.
- Detect CLIs with `which` + `<cli> auth status` (Claude has JSON output for this) and show them as
  "use your plan" options only when authenticated. Never prompt for a plan login in the plugin.
- One capability table per backend drives the UI (tools? schema? streaming? context size?), and a
  fallback chain handles 429s by kind (per-minute → retry after; per-day → switch provider).
- Budget: two repair attempts max (error back to the model), then show the card with the loader error.

## 5. File access: own tools, or the agent's?

Flemming's constraint (2026-11-19): two machines. The work PC has Claude Code and OpenCode installed,
logged in, loaded with skills and MCP servers; the studio Mac mini has **no AI software and should not**
(shared machine, no saved credentials, ChatGPT/Claude are used in the browser). So both routes have to
exist — but they should not be two implementations of the same feature.

**The file layer is the small part.** Editing a patch means listing a folder, reading a handful of
`.luau` files, writing them, and checking the result. That is five tools and the plugin already has
the loader, the type checker and hot reload. The hard part is the model knowing *what* to change — and
that is the same problem in every route.

**Recommendation: domain tools in the plugin, three runners behind one interface.**

```
AgentTask ──► Runner ──┬─ LocalLoop  : HTTP backend (genai / OpenAI-compatible) + our tools
                       ├─ CliRunner  : claude -p / codex exec, cwd = patch, own tools
                       └─ AcpRunner  : agent-client-protocol            (later)
                                     │
                              ToolSet (ours)  ── read_patch · write_patch · validate_patch · …
```

Tools are **patch-domain, not raw filesystem**:

| Tool | Why not raw `read_file`/`rm` |
|---|---|
| `list_patches()` | the model sees patches and their UI files, not the whole disk |
| `read_patch(name, files?)` | returns `init.luau`, `ui/*`, the type defs, and the 1–2 closest examples; token-cheap |
| `write_patch(name, files)` | atomic write + snapshot, then **runs the loader and returns the errors** |
| `validate_patch(name)` | the existing strict load + type check, as a tool, no writes |
| `delete_patch(name)` | move to `<data dir>/trash/`, never `rm`; requires confirmation in the shell |
| `render_panel(name)` | headless render (already exists) → PNG in the chat, so the model can see its UI |
| `run_analysis(path)` | the analysis crate, for "why does it sound like this" |

All paths resolve against the patches root, canonicalised, with `..`/symlink escapes rejected, and no
shell. Everything is snapshot-before / diff-after, so every run is inspectable and undoable even when
the model wrote the files itself.

Why not make "let the CLI use its built-in Read/Edit/Bash" the primary route:

- **It does not exist on the studio Mac**, and adding it there means installing an agent whose login
  you explicitly do not want on that machine.
- **It is not reproducible.** "Clapped out with skills and MCPs" on one machine is the point on that
  machine, and a liability for support: the same prompt behaves differently per install, per CLI
  version, per `CLAUDE.md`.
- **The capability set diverges.** Bash gives you everything, but only if the model knows the commands
  (`cargo run -p ui-components --example render …`, `luau-lsp analyze …`). Encoding them as tools makes
  them reliable, cheap to call and identical across backends.
- **Trust.** An agent with Bash in the patches folder can do anything the user can. Domain tools plus
  snapshot/diff keep the blast radius to "a patch file changed".

Why the CLI route is still worth having (and why it is not much code): on the work PC it delivers the
user's skills, MCP servers and tuned `CLAUDE.md` for free, and the agent explores the repo better than
a 5-tool loop will. Point it at the same root, one shot, and keep your own `validate_patch` + diff as
the gate. Options worth exposing: `--setting-sources` / `--strict-mcp-config` (a plugin should not
silently load the user's whole config), `--max-turns`, and a "use my agent's full setup" toggle.

**Machine-specific UX that falls out of this:**

- Studio Mac: HTTP key or local Ollama, key in the user config (a dedicated free-tier Gemini key for
  the studio account, no billing on the project; DECIDED 2026-11-19, no session-only mode needed).
- Work PC: "use my installed agent" appears when `claude auth status` / `codex login status` says
  logged in; the key path stays available.
- Both: the shared verification step is the loader; a run that produces an invalid patch shows the
  error card and a diff, never a silently broken patch.

Add a `patches/AGENTS.md` (and `CLAUDE.md` pointing at it) describing the DSL, the type-def file, one
good example and the validate command. That single file improves every CLI agent and is also the best
system-prompt material for the HTTP backend.

## Sources

- Agent SDK overview and the third-party login clause: <https://platform.claude.com/docs/en/agent-sdk/overview>
- Claude Code headless / `-p` / `--json-schema` / `--bare`: <https://code.claude.com/docs/en/headless>, <https://code.claude.com/docs/en/cli-reference>
- Claude plan-backed SDK usage: <https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan>
- Agent SDK credit history (May–June 2026): <https://arstechnica.com/ai/2026/06/anthropic-pauses-token-based-billing-for-its-claude-agent-sdk/>
- Codex non-interactive mode and `--output-schema`: <https://learn.chatgpt.com/docs/non-interactive-mode>
- Codex with a ChatGPT plan: <https://help.openai.com/en/articles/11369540-using-codex-with-your-chatgpt-plan>
- ACP agents list: <https://agentclientprotocol.com/get-started/agents>
- Claude ACP adapter: <https://github.com/agentclientprotocol/claude-agent-acp>
- Gemini CLI quotas (and the Antigravity replacement): <https://geminicli.com/docs/resources/quota-and-pricing/>, <https://mizanstack.com/guides/gemini-free-tier>
- genai: <https://github.com/jeremychone/rust-genai> · rig: <https://github.com/0xPlaygrounds/rig> · agent-harness: <https://github.com/getlatentic/agent-harness>
