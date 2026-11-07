# uncaged_agent_api

The agent protocol: the `warp.multi_agent.v1` protobuf messages that Agent Mode
streams from `uncaged_engine` and stores in SQLite (`agent_tasks.task` holds
encoded `Task` messages).

## Where it came from

Vendored from Warp's `warp-proto-apis` repository
(<https://github.com/warpdotdev/warp-proto-apis>), directory
`apis/multi_agent/v1/`, at rev `ac1af7303d2931b0fb485be650a1fbc8b80d5667`, where
it was the `warp_multi_agent_api` crate. The library keeps that crate name
(`[lib] name`), so callers still `use warp_multi_agent_api as api;`.

The protobuf package name (`warp.multi_agent.v1`) and every field number are
unchanged, so conversations saved by earlier builds still decode. Change a
`.proto` file only in wire-compatible ways: add fields with new numbers, and
mark removed ones `reserved`.

## Layout

- `proto/` — the `.proto` sources, as upstream (protobuf edition 2023).
- `src/generated/` — prost output, checked in. Regenerate it with
  `./script/regen_agent_api` after editing `proto/` (needs `protoc` on PATH; the
  crate itself builds without protoc and without a build script).

## License

Copyright (C) 2020-2026 Denver Technologies, Inc. Licensed under the GNU Affero
General Public License, version 3 (AGPL-3.0-only), like the rest of this fork;
see `LICENSE-AGPL` at the repository root. The upstream `.proto` files carry no
per-file license headers.
