# Security Policy

Uncaged is a free, account-free, bring-your-own-model fork of the open-source Warp terminal. We appreciate the efforts of security researchers who help keep users of this fork safe.

## Reporting a Vulnerability

If you believe you've found a security vulnerability in Uncaged, please follow responsible disclosure practices and **do not** open a public GitHub issue or pull request, as this could expose the vulnerability before a fix is available.

Instead, please report it privately through GitHub:

- **GitHub Security Advisory (preferred):** [Open a private advisory](https://github.com/getuncaged/uncaged/security/advisories/new) on the Uncaged repository.

We will acknowledge your report and work with you to understand and resolve the issue as quickly as possible.

## Privacy & data handling

Uncaged is designed so that your prompts and terminal data never leave your machine except to reach the model endpoint you configure yourself. There are no accounts, no login, no telemetry, no analytics, no cloud sync, and no autoupdate or phone-home. The only outbound network traffic is to the model provider or local runtime you connect in **Settings → AI Models**. Configuration is stored locally in `~/.uncaged/`.
