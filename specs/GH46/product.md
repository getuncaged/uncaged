# GH46: Per-connection reasoning effort for Uncaged models

## Summary

Make reasoning effort a real setting for Uncaged connections. Each connection stores an effort choice. The levels offered are the ones the connection's current model actually supports, discovered from the provider where possible and configured by the user otherwise. The engine sends the choice in the form each backend expects. "Default", which sends nothing and lets the provider decide, is always available and is the default.

## Problem

Choosing an effort level has no effect on the Uncaged engine today. The engine sends only the model, messages, tools and token limit. No effort or thinking parameter reaches any provider, and the Uncaged model menu has no effort control at all.

A single fixed list such as Low/Medium/High cannot work across backends:

- **Different parameters.**
  - OpenAI-compatible servers use `reasoning_effort`.
  - OpenRouter uses a `reasoning` object.
  - Anthropic uses `output_config.effort` together with `thinking`.
  - CLI agents (ACP) use a session config option.
- **Different values, per model.**
  - The vocabulary spans `none`, `minimal`, `low`, `medium`, `high`, `xhigh` and `max`.
  - Even within one provider, some models reject levels that others accept.
- **Different failure modes.** Some providers reject an unsupported value with HTTP 400, for example OpenAI, Anthropic and xAI. Others silently map it to a nearby level.

Model lists have a related gap. The picker shows each preset's hard-coded model list. Models available on the user's own endpoint (Ollama, LM Studio, LiteLLM, OpenRouter) aren't listed unless typed in by hand.

## Goals

1. A per-connection effort choice that is actually sent to the provider.
2. Offer only levels the active model supports, using the provider's own level names.
3. Discover models and their effort levels from the provider where the provider exposes them.
4. Let users configure levels by hand for endpoints that expose nothing.
5. Never break a working connection. With the setting left at Default, requests are byte-for-byte what they are today.

## Non-goals

1. Displaying or replaying model reasoning output (thinking blocks, `reasoning_content`, `reasoning_details`). This is tracked separately; see Open questions.
2. Exposing raw token budgets (`budget_tokens`, `reasoning.max_tokens`) as a user setting. Where a backend only takes budgets, levels are mapped to budgets internally.
3. Per-conversation or per-message effort. The setting belongs to the connection.
4. Changing effort behaviour for Warp-server models when the Uncaged engine is inactive.

## Behavior

### The setting

1. Every connection has an effort setting whose value is either **Default** or one level name.
   - New connections start at Default.
   - Connections saved before this feature load as Default.
2. **Default means the request contains no effort or thinking parameter.** The request is identical to what this version sends today, and the provider applies its own default.
3. Levels are shown with the provider's own names, capitalized for display. `none` and `off` display as **Off**.
4. Levels are listed in a fixed order: Off < Minimal < Low < Medium < High < XHigh < Max. Only the levels the current model supports are shown.

### Where it appears

5. In the model picker, the active connection's current model row has an effort submenu: Default followed by the supported levels, with the active choice checked. Choosing an item saves it on the connection and applies from the next request. No restart is needed.
6. The connection editor has an effort control showing the same choices.
7. The connection editor has an optional **Effort levels** field for a manual list, e.g. `low, medium, high`. When filled, it replaces discovered levels for that connection. Unknown names are allowed and are sent verbatim.
8. If the current model has no known levels, from discovery, a built-in table or the manual field:
   - the picker shows no effort submenu;
   - the editor shows Default only, with a hint that levels can be added by hand.

### Model changes and unsupported values

9. **Saved choice not supported by the newly selected model.**
   - The saved choice is kept, so switching back restores it.
   - The effective level is the highest supported level below it in the ordering. If no such level exists, nothing is sent (Default).
   - The picker and editor show the effective level and mark it as adjusted.
10. **Provider rejects the effort parameter.**
    - A rejection is an HTTP 400 whose error names the effort or thinking parameter.
    - The engine retries the same request once without the parameter, and the turn proceeds.
    - The model is recorded as not supporting effort until the next refresh of its models. Its picker submenu and editor control then show that.
    - Errors that don't name the parameter are not retried and surface as today.
11. Effort is applied per request, so changing it mid-conversation affects the next request only.
    - Earlier turns are unaffected.
    - On providers with prompt caching, such as Anthropic, a change can make the next request's cache miss. The editor notes this.

### Discovery

12. Discovery runs:
    - in the background when a connection is saved or becomes active;
    - from a **Refresh models** button in the connection editor.

    It never delays an agent request. A request uses whatever is known at that moment.
13. Discovery fetches the endpoint's model list and, where the backend exposes it, each model's effort levels and default level.
    - The picker's model list for the connection becomes the union of the preset's list and the discovered models, without duplicates.
    - The connection's current model is always present.
14. Discovery talks only to the connection's own host. It sends the connection's API key only to that host, exactly as normal requests do.
15. **Discovery fails** (server not running, offline, 401, unexpected response).
    - Previously discovered results stay in use.
    - The editor shows the failure and when it last succeeded.
    - Nothing else in the app reports it.
16. **Backend exposes model ids but no effort information** (e.g. OpenAI, Groq, xAI, Gemini, DeepSeek, Mistral, Together).
    - Levels come from a built-in table for that preset, if one exists.
    - Otherwise the model has no known levels (invariant 8).
    - The manual field (invariant 7) always takes precedence.

### CLI agents

17. **For CLI agent connections, effort levels come from the agent.** If the agent advertises a thought-level session option (Claude Code's `effort`, Codex's `reasoning_effort`), those values are the levels.
    - Default means the option is not set.
    - An agent value that itself means default is merged into Default, not shown twice.
    - If the agent advertises no such option, the connection has no levels.

### Robustness

18. **Invalid effort value in a hand-edited `connections.json`.** The connection loads with Default, logs a warning, and keeps working.

## Success criteria

1. With every connection at Default, request bodies match the current version for all three wires.
2. With a level chosen, the outgoing request carries the parameter in the backend's expected shape, verified per backend style in tests and manually against at least: Anthropic, OpenAI, OpenRouter, Ollama, and one CLI agent.
3. For Anthropic, OpenRouter, Ollama and LM Studio, the picker shows the model's real levels after a refresh, without manual configuration.
4. A model that rejects the parameter still completes the turn, and the UI shows that it doesn't support effort.

## Open questions

1. **Reasoning replay.** The engine discards reasoning output today. Anthropic (with tools), DeepSeek, Kimi and OpenRouter (`reasoning_details`) expect it to be sent back on tool turns. Raising effort makes reasoning more likely, so this may surface as 400s in tool loops.
   - Should this ship before phase 1?
   - Or should levels be disabled for those backends until it does?
   - It overlaps with #43, which concerns tool results missing from history.
2. **Label for Default.** Keep "Default", or use "Auto" or "Provider default"?
3. **Unsupported levels after a model switch.** Show them greyed out, or hide them as specified in invariant 4?
4. **Anthropic models that take only `budget_tokens`, not effort.** Offer Low/Medium/High mapped to budgets, or hide effort for them?
