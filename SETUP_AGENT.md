# Agent Setup Prompt

Don't configure Agencia by hand. Copy-paste the prompt below into **your own coding agent** (Claude Code, Codex, OpenCode, Cursor, Gemini CLI, Antigravity, etc.) inside the cloned `agencia` repo so it adapts everything to the CLIs, models, and local/cloud inference you actually use.

---

```text
Set up Agencia in this repository for MY local machine, MY installed AI CLIs, and MY preferred models/inference.

1. Inspect my environment first (read-only):
   - Check which agent CLIs and local inference runtimes are installed (`claude`, `codex`, `opencode`, `agy`, `gemini`, `aider`, `ollama`, `lmstudio`, etc.) and which accounts/models are active.
   - Do NOT assume I use the default models or providers in the repo.

2. Build and install Agencia:
   - Run `./install.sh` (or `cargo build --release` + `agencia init`).
   - Ask me before modifying any global config outside this repo (`~/.claude.json`, `~/.codex/config.toml`, `~/.config/opencode/opencode.json`, LaunchAgent).

3. Adapt Agencia to my models & providers with minimal changes (no over-engineering):
   - Write `~/.config/agencia/config.json` with the right defaults (`bec_model`, `cla_model`, `codex_name`) for the models I actually have available.
   - If I use an extra CLI or local inference provider (e.g., Ollama, OpenRouter, Gemini CLI, Cursor, Aider) that Agencia doesn't read natively yet, wire a minimal adapter or connect it via the `sesiones` MCP server (`target/release/agencia mcp`).
   - Wire the `sesiones` MCP server into ONLY the agent clients I actually use.

4. Verify end-to-end:
   - Start Agencia (`agencia up`), confirm `http://127.0.0.1:8800/api/state` returns HTTP 200 with my live sessions, and show me the exact command (`agencia`) to open it.
```
