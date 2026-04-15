# OAuth Bindings

## Purpose

`oauth_bindings` stores the durable OAuth authorization result bound to a Nyro provider.

It represents the state after an OAuth flow has completed successfully and the provider can use OAuth-managed credentials instead of a static API key.

This table is intentionally separate from `providers`:

- `providers` remains the canonical provider configuration record
- `oauth_bindings` stores OAuth identity and credential lifecycle state
- transient authorization flow state should live in a separate `oauth_sessions` table

## Recommended Scope

This table should store:

- which provider is bound to OAuth
- which external subject/account is authorized
- current binding status
- token metadata needed for runtime decisions
- timestamps needed for refresh, expiry, and audit

Sensitive credential values such as `access_token`, `refresh_token`, and `id_token` should be encrypted at rest. If token rotation history is needed later, split token material into a separate `oauth_tokens` table.

## Proposed Schema

```sql
CREATE TABLE oauth_bindings (
    id                TEXT PRIMARY KEY,
    provider_id       TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    subject           TEXT NOT NULL,
    account_id        TEXT,
    account_name      TEXT,
    account_email     TEXT,
    issuer            TEXT,
    token_type        TEXT NOT NULL DEFAULT 'Bearer',
    scope             TEXT,
    access_token      TEXT NOT NULL,
    refresh_token     TEXT,
    id_token          TEXT,
    expires_at        TEXT,
    last_authorized_at TEXT NOT NULL,
    last_refresh_at   TEXT,
    status            TEXT NOT NULL DEFAULT 'active',
    created_at        TEXT DEFAULT (datetime('now')),
    updated_at        TEXT DEFAULT (datetime('now')),
    UNIQUE(provider_id)
);

CREATE INDEX idx_oauth_bindings_subject ON oauth_bindings(subject);
CREATE INDEX idx_oauth_bindings_status ON oauth_bindings(status);
CREATE INDEX idx_oauth_bindings_expires_at ON oauth_bindings(expires_at);
```

## Field Notes

- `id`: internal binding identifier
- `provider_id`: owning Nyro provider; one active binding per provider in the current design
- `subject`: stable external OAuth subject, typically the provider-side user identifier
- `account_id`: optional provider account id if different from `subject`
- `account_name`: display name for UI
- `account_email`: optional email for operator visibility
- `issuer`: token issuer or platform identifier
- `token_type`: usually `Bearer`
- `scope`: granted scope string as returned by the provider
- `access_token`: encrypted access token
- `refresh_token`: encrypted refresh token if refresh is supported
- `id_token`: encrypted ID token if returned by the provider
- `expires_at`: access token expiry time; nullable for non-expiring tokens
- `last_authorized_at`: when the binding was first established or fully re-authorized
- `last_refresh_at`: last successful refresh timestamp
- `status`: recommended values: `active`, `expired`, `revoked`, `error`
- `created_at` / `updated_at`: audit timestamps

## Why Keep Tokens Here Initially

For an MVP, keeping binding state and token material in one table reduces join complexity and speeds up implementation.

This is acceptable if:

- only the latest valid token set needs to be retained
- token rotation history is not yet required
- encryption at rest is enforced

If Nyro later needs token history, multiple token versions, or richer audit trails, move token material into a dedicated `oauth_tokens` table and keep `oauth_bindings` as the long-lived relationship record.

## Suggested Provider Integration

To integrate this table with the current schema, `providers` should gain:

```sql
ALTER TABLE providers ADD COLUMN auth_mode TEXT NOT NULL DEFAULT 'api_key';
ALTER TABLE providers ADD COLUMN oauth_binding_id TEXT;
```

Expected runtime behavior:

- `auth_mode = 'api_key'`: use `providers.api_key`
- `auth_mode = 'oauth'`: resolve `oauth_bindings` by `oauth_binding_id` or `provider_id`

## Validation Rules

Recommended invariants:

- `provider_id` must refer to an existing provider
- `subject` must not be empty
- `access_token` must not be empty when `status = 'active'`
- `refresh_token` may be null
- `expires_at` may be null
- only one active binding per provider should exist in the MVP design

## Open Questions

- whether to allow multiple bindings per provider for account switching
- whether refresh failures should change `status` immediately or only after access token expiry
- whether `oauth_binding_id` should be stored on `providers`, or bindings should be resolved only by `provider_id`
- when to split token material into a dedicated `oauth_tokens` table
