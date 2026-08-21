# Security Policy

Uncaged is a free, account-free, bring-your-own-model fork of the open-source Warp terminal. We appreciate the efforts of security researchers who help keep users of this fork safe.

## Reporting a Vulnerability

If you believe you've found a security vulnerability in Uncaged, please follow responsible disclosure practices and **do not** open a public GitHub issue or pull request, as this could expose the vulnerability before a fix is available.

Instead, please report it privately through GitHub:

- **GitHub Security Advisory (preferred):** [Open a private advisory](https://github.com/getuncaged/uncaged/security/advisories/new) on the Uncaged repository.

We will acknowledge your report and work with you to understand and resolve the issue as quickly as possible.

## Privacy & data handling

Uncaged is designed so that your prompts and terminal data never leave your machine except to reach the model endpoint you configure yourself. There are no accounts, no login, no telemetry, no analytics, and no cloud sync.

Outbound traffic is limited to two things: the model provider or local runtime you connect in **Settings → AI Models**, and — only if you turn it on — update checks against GitHub, described in the next section. Nothing identifying you is sent in either case. Configuration is stored locally in `~/.uncaged/`.

## How updates are trusted (and what that does not cover)

Uncaged can check GitHub for a new release and install it. That is **off until
you turn it on** — the app makes no update request of any kind before you answer
the one-time prompt — and it can be turned off again in Settings, or disabled
outright with `UNCAGED_DISABLE_SELF_UPDATE=1`.

When it is on, this is the chain:

1. The release API is read over TLS from `api.github.com`. No identifier is
   sent; the only header that describes the client is a `User-Agent` of
   `Uncaged/<version>`, which does reveal the version you are running.
2. Each release asset comes with a SHA-256 **digest published in that API
   response**, separately from the file it describes.
3. The download is fetched over HTTPS from a GitHub host — the URL and every
   redirect hop are checked, because the URL arrives in the API response and is
   therefore untrusted input.
4. The downloaded file is hashed **from disk** and compared to the digest before
   the disk image is mounted or anything is installed. A mismatch deletes the
   file and aborts.

**What this does not prove.** The digest is computed by GitHub over what GitHub
stores. It shows the bytes you received are the bytes GitHub has — nothing more.
It does not establish who published them. Uncaged is ad-hoc signed: it has no
Apple Developer ID and no notarization, so there is no independent signature to
check, and macOS performs no Gatekeeper assessment on a bundle the app installs
itself.

So **publish access to this repository is the security perimeter of the
updater.** Anyone who could push a release could publish a malicious build and a
matching digest, over valid TLS, and consenting clients would install it. We
would rather say that plainly than let a checksum imply a guarantee it does not
give.

Mitigations we consider on the roadmap, in order: an offline signature over a
release manifest, using a key that never touches CI; GitHub artifact
attestations; and a Developer ID with notarization, which would also let macOS
form its own opinion.

If you would rather not rely on that chain, install and update through Homebrew
or winget — both verify their own checksums, and the app deliberately refuses to
self-update an install a package manager owns.
