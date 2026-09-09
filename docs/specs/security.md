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

## Dashboard authentication

The Daily service refuses to start without an owner secret of at least 20 bytes. Login stores an Argon2 verifier and issues independently generated session and CSRF credentials. Only credential digests are stored. Sessions carry explicit scopes, have 30-minute idle and eight-hour absolute expiry, and are revoked when the owner secret changes.

Allowed Host and Origin values are exact configuration, not suffix matches. Login and logout require both; logout additionally requires the session CSRF token. Secret replacement requires the explicit rotation setting.

## Vault Safety

Agent-written Markdown summaries should avoid secrets, raw stack traces, personal data, and long log dumps. Link to source windows through event IDs or time ranges instead.
