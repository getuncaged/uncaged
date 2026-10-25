# Security Policy

Uncaged is a free, account-free, bring-your-own-model fork of the open-source Warp terminal. We appreciate the efforts of security researchers who help keep users of this fork safe.

## Reporting a Vulnerability

If you believe you've found a security vulnerability in Uncaged, please follow responsible disclosure practices and **do not** open a public GitHub issue or pull request, as this could expose the vulnerability before a fix is available.

Instead, please report it privately through GitHub:

- **GitHub Security Advisory (preferred):** [Open a private advisory](https://github.com/getuncaged/uncaged/security/advisories/new) on the Uncaged repository.

We will acknowledge your report and work with you to understand and resolve the issue as quickly as possible.

## Privacy & data handling

Uncaged is designed so that your prompts and terminal data never leave your machine except to reach the model endpoint you configure yourself. There are no accounts, no login, no telemetry, no analytics, no cloud sync, and no autoupdate or phone-home. The only outbound network traffic is to the model provider or local runtime you connect in **Settings → AI Models**. Configuration is stored locally in `~/.uncaged/`.

## Update checks

Uncaged can check GitHub for a new release and tell you one exists. That is
**off until you turn it on** — the app makes no update request of any kind
before you answer the one-time question — and it can be turned off again in
Settings.

It only looks. Uncaged does not download or install updates and never replaces
its own bundle; you update the way you installed, with `brew upgrade`, `winget
upgrade`, or from the release page. That is deliberate: the app is ad-hoc signed,
with no Apple Developer ID and no notarization, so it has no way to prove a
downloaded build is genuinely ours. Homebrew and winget verify their own
checksums, which is a better guarantee than anything we could offer in-app.

The check itself reads `api.github.com` over TLS. No identifier is sent; the
only header describing the client is a `User-Agent` of `Uncaged/<version>`,
which does reveal the version you are running.
