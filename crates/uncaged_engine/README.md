# uncaged_engine

The local, bring-your-own-model agent harness for the Uncaged fork. It
replaces Warp's server-side inference so native Agent Mode runs on the user's own
backend. See [`../../UNCAGED.md`](../../UNCAGED.md) for the product-level
overview; this file is the developer map.

## Where it plugs in

One call site: [`warp_multi_agent_client::generate_multi_agent_output`](../warp_multi_agent_client/src/lib.rs).
At the top of that function:

```rust
#[cfg(not(target_family = "wasm"))]
if let Some(local_stream) = uncaged_engine::run_if_active(request) {
    return Ok(local_stream.map(|e| e.map_err(Error::LocalEngine)).boxed());
}
```

`run_if_active` returns `None` when no engine is configured, so Warp's normal
server path runs untouched.

## Module map

| File | Responsibility |
|---|---|
| `config.rs` | Load/save `~/.uncaged/engine.json` + env overrides; `active()` decides if the engine is on. |
| `proto.rs` | `api` alias for `warp_multi_agent_api`; JSON ↔ `prost_types::Struct`. |
| `model.rs` | Provider-neutral `Conversation` / `NeutralMsg` / `ToolUse` / `ToolResult`. |
| `request_parse.rs` | Lower a `Request` into neutral messages (history + new input), deduped. |
| `tools.rs` | JSON Schemas for built-in tools; encode model tool-use → Warp `ToolCall`; decode `ToolCallResult` → text; MCP passthrough. |
| `system_prompt.rs` | Author the system prompt + fold in `InputContext` (pwd, OS, shell, git, rules). |
| `providers/` | `Provider` trait + `anthropic`, `openai` (covers local models), `acp`; `sse.rs` line decoder. |
| `wire.rs` | Build `ResponseEvent`s: `StreamInit`, `ClientActions` (create task / add message / append text / tool call), `StreamFinished`. |
| `engine.rs` | Per-turn orchestration: parse → run provider → sequence events. |

## Design invariants

- **Stateless per turn.** Warp's client owns the cross-turn tool loop; each
  `run_turn` is one assistant turn (text + tool calls) then finish. Tool results
  arrive in the *next* request's `input`.
- **The client executes tools, not the engine.** The engine only emits `ToolCall`
  messages; Warp runs them in the user's PTY / filesystem / MCP and feeds results
  back. The engine never does I/O on the user's machine beyond the model HTTP call
  (and, for ACP, spawning the configured CLI).
- **Two things Warp's server keeps private** are reconstructed here: the system
  prompt and the built-in tools' JSON schemas. These are the main fidelity levers.

## Assumptions worth verifying when iterating

- `proto.rs` assumes `warp_multi_agent_api` maps `google.protobuf.*` well-known
  types to the `prost_types` crate (`Struct`, `Value`, `FieldMask`). If the proto
  crate vendors its own well-known types, adjust `proto.rs` and `wire.rs`
  accordingly.
- The streamed-text append uses a `FieldMask` path of `agent_output.text`
  (`wire.rs`). If appends don't render, that's the first thing to check.
