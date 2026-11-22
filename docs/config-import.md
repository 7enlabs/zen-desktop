# External Config Import

One command to switch from another AI coding tool: Zen CLI detects what's
installed, shows a dry-run preview with per-item checkboxes, and imports
your configuration. API keys are **never** copied - they're listed by name
and source, and you paste them into Zen CLI's own secure store yourself.

## Usage

```
/import
```

The `/import` slash command (wiring into `slash.rs` is three lines -
see below) runs `config_import::plan()` for the dry-run, renders each
`ImportItem` as a checkbox row plus the `SecretFinding` list, then calls
`config_import::apply()` on the selection.

## What gets imported

| Source | Models | Skills | MCP servers | Commands | Rules |
|---|---|---|---|---|---|
| `~/.claude.json`, `~/.claude/` | `settings.json` → `model` | `~/.claude/skills/*/SKILL.md` | `~/.claude.json` / `settings.json` → `mcpServers` | `~/.claude/commands/**/*.md` | `~/.claude/CLAUDE.md` |
| `~/.codex/` | `config.toml` → `model` (+ `model_provider` base URL) | - | - | - | - |
| `~/.cursor/` | - | - | `mcp.json` | - | project `.muse/rules/` (per-project, see note) |
| `~/.continue/` | `config.yaml` / `config.json` → `models[]` | - | - | - | - |
| VS Code globalStorage `saoudrizwan.claude-dev` | - | - | `cline_mcp_settings.json` | - | - |
| VS Code globalStorage `rooveterinaryinc.roo-cline` | - | - | `mcp_settings.json` | - | - |
| `~/.aider.conf.yml` | `model`, `weak-model`, `editor-model` | - | - | - | - |
| `~/.config/opencode/opencode.json` | `"model": "provider/model"` | - | `"mcp"` | - | - |

macOS VS Code storage lives under
`~/Library/Application Support/Code/...`; both locations are probed.

### Destinations

| Import kind | Destination | Trust |
|---|---|---|
| Skill | `~/.zen/skills/<name>/` | **UNTRUSTED** - approve before first use (Project-equivalent, PR #73) |
| MCP server | merged into `~/.zen/mcp.json` | **Untrusted** - every tool call asks until approved (PR #77) |
| Command | `~/.zen/commands/<name>.md` | n/a |
| Rule / memory | `~/.zen/imported/<source>-<file>` | n/a |
| Model | `~/.zen/imported-providers.json` | suggestions only - activate in Settings → Providers |

Server names are namespaced (`<source>-gh`, `<source>-sqlite`, …) so two
tools can define a server with the same name. Existing Zen files are
never overwritten: the item is deselected in the preview and skipped at
apply time with a note.

## Model mapping

Imported model strings are translated to a model name + base URL
(seam for PR #66 `ProviderKind`). The suggested name keeps only the bare
model name; the endpoint is carried in the mapping itself. The provider
prefixes in the left column are the input vocabulary read from the user's
own tool configs, not a list of providers Zen offers:

| Input | Maps to |
|---|---|
| `anthropic/...`, `claude-*` | Messages API |
| `openai/...`, `gpt-*`, `o1*`, `o3*` | `https://api.openai.com/v1` |
| `ollama/...`, `llama*` | Ollama `http://localhost:11434/v1` |
| `openrouter/...` | `https://openrouter.ai/api/v1` |
| `deepseek/...` | `https://api.deepseek.com/v1` |
| `gemini/...`, `google/...`, `gemma*` | OpenAI-compatible endpoint |
| `xai/...`, `grok*` | `https://api.x.ai/v1` |
| `groq/...` | `https://api.groq.com/openai/v1` |
| `mistral/...`, `mixtral*` | `https://api.mistral.ai/v1` |
| `moonshot/...`, `kimi*` | `https://api.moonshot.ai/v1` |
| `zhipu/...`, `glm*` | `https://open.bigmodel.cn/api/paas/v4` |
| `qwen/...`, `dashscope/...` | `https://dashscope.aliyuncs.com/compatible-mode/v1` |
| TOML `[model_providers.*]` (`~/.codex/config.toml`) | base URL matched against the table above |
| anything else | kept verbatim for manual mapping |

## Secrets: detected, listed, never copied

`is_secret_pair()` flags key names (`apiKey`, `*_token`, `password`, …)
and value shapes (`sk-…`, `ghp_…`, `AKIA…`, long opaque strings).
Findings are reported as `SecretFinding { name, source, hint }`; there is
no code path that writes a detected secret anywhere - including a
defense-in-depth strip of secret `env` vars when merging MCP JSON, and an
end-to-end test asserting no secret-looking string lands under `~/.zen`.

## `/import` wiring (for `slash.rs`, PR #76)

```rust
// In the slash-command dispatcher:
"import" => {
    let plan = zen_engine::config_import::plan(&home_dir(), &zhome_dir())?;
    // render plan.items as checkboxes + plan.secrets as a list, then:
    let report = zen_engine::config_import::apply(&plan, &zhome_dir())?;
    // render report
}
```

## Merge notes

- Only touches `engine/src/lib.rs` (one `pub mod` line - merge-queue #51).
- No new dependencies (`serde`/`serde_json`/`anyhow` only); TOML and YAML
  are read with minimal hand-rolled parsers scoped to the keys imported -
  complex configs degrade to warnings, never to wrong imports.
- When PR #66 merges: replace `MappedProvider` with `ProviderKind`.
- When PR #73 merges: register `~/.zen/skills/` imports with
  Project-equivalent trust (never `SkillTrust::User`).
- When PR #77 merges: hand the merged `mcp.json` map to its config loader
  (trust metadata is already under `_zen.trust`).
