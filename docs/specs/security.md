# Security

## Defaults

- Bind services to `127.0.0.1`.
- Require API keys for ingestion.
- Treat logs as sensitive by default.
- Keep retention short.
- Do not publish raw logs to the Markdown vault.

## Redaction

The collector should redact known secret shapes before persistence:

- bearer tokens
- API keys
- connection string passwords
- cookies
- private keys

Redaction must preserve enough context to diagnose the event.

## Network Exposure

If logs must be accepted from another device:

- use a private network or tunnel;
- configure an API key per producer or producer group;
- record source identity separately from caller IP;
- avoid public internet exposure for the first version.

## Vault Safety

Agent-written Markdown summaries should avoid secrets, raw stack traces, personal data, and long log dumps. Link to source windows through event IDs or time ranges instead.
