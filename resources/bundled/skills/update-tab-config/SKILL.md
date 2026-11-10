---
name: update-tab-config
description: Update existing Uncaged tab config TOML files from natural-language edit requests. Use when the user wants to modify a tab config that already exists or when editing a tab config file already open in Uncaged.
---

# update-tab-config

Update an existing Uncaged tab config in place.

## Required context

- Use the `tab-configs` skill as the canonical source of truth for:
  - schema details
  - validation rules
  - examples
  - common layout patterns

## Workflow

1. Read the existing tab config file before making changes.
2. Understand the requested edit.
3. If important details are missing or ambiguous, use the `ask_user_question` tool before editing. Do not guess about layout changes, command changes, or parameters.
4. Make sure you are editing a tab config in Uncaged's tab config directory, `{{tab_configs_dir}}`, then update it so it remains valid according to the `tab-configs` schema (there is no `on_close` field; a config containing unknown fields fails to load).
5. Preserve the user's existing structure and naming where possible unless the requested change requires restructuring.
6. Briefly explain what changed.
