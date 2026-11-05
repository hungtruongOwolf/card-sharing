---
title: Region / jurisdiction policy
type: concept
status: proposed
updated: 2026-11-05
sources: [raw/discussions/2026-11-04-idea-round-5.md, raw/discussions/2026-11-04-idea-round-6.md]
related: [[prd-mvp]], [[change-management]], [[security-baseline]], [[genai-effects]], [[monetization]], [[vision]]
---
# Region / jurisdiction policy
**TL;DR:** Default global rules (min age 13+), with per-jurisdiction overrides in **versioned config**,
resolved server-side by a `policy` module. Adding/changing a country = config change + tests, no code edits.

## Model
```yaml
# config/policy/regions.yaml  (versioned, reviewed like code)
default:
  min_age: 13
  discovery: { min_age: 18 }          # handle search never returns younger users (prd-mvp US-5a)
  public_links: { min_age: 18 }       # public board links off below this age (prd-mvp US-24)
  ai_effects: { cloud_allowed: true, people_in_photo: on_device_or_non_gemini }
  ads: { rewarded: true, personalized_requires_consent: false }
  ai_label: { visible_badge: true, c2pa: true }
overrides:
  EU:  { min_age: 16, ads: { personalized_requires_consent: true } }   # GDPR Art. 8 (member states 13–16) — legal review
  GB:  { min_age: 13 }                                                   # UK (ISO 3166 code GB) — Age Appropriate Design Code — review
  US:  { min_age: 13 }                                                   # COPPA
  KR:  { min_age: 14 }                                                   # PIPA — verify
```
(Values are placeholders pending legal review — only the mechanism is decided. Implemented in `crates/policy`, config at
`config/policy/regions.yaml`; country keys are ISO 3166-1 alpha-2. Open: should the `EU` group become `EEA` (adds IS, LI, NO)?)

## Rules
- Resolution order: country → region group (EU) → default. Region from store country / IP / user-declared; take the **strictest** when signals conflict.
- Enforced **server-side** (`policy` crate): sign-up age gate, ad config returned to client, AI routing, feature flags.
  Client only renders what the server allows.
- Each override has a source/legal note and a test (`policy_resolves_eu_min_age_16`).
- Consumers depend on `PolicyService::resolve(ctx) -> Policy`, never on raw country codes.

## Implementation (T-28)
- Age gate: `IdentityService::submit_age` resolves `min_age` with `PolicyService::resolve_strictest` over the request's store /
  declared / IP country and the signals stored with the account; client hints can only tighten.
- IP country comes only from a trusted edge header (`API_IP_COUNTRY_HEADER`, e.g. `cf-ipcountry`); without one there is no IP
  signal. Which edge sets it in production (Cloudflare in front of the API, or a GeoIP database) is still open.

## Implementation (T-29)
- `config/policy/regions.yaml` now carries `adult_age` (default 18; minor status) and `discovery.min_age` (default 18; handle
  search), both resolved strictest (max) like `min_age`. KR placeholder: `adult_age: 19`, `discovery.min_age: 19` (civil
  majority 19 — verify in legal review). Fields missing from a config default to 18.
- `identity::AgeService::status(account, current_ctx) -> {is_minor, discoverable}` decrypts the DOB, computes the age for the
  current UTC date and applies `resolve_strictest(current, stored sign-up signals)` — a later request from a laxer region never
  lowers `adult_age` or `discovery.min_age` (SR-24). Accounts without a DOB get every protection.
- OS age signals (`POST /v1/me/age-signal`, flag `API_OS_AGE_SIGNALS_ENABLED`, default off pending legal L-3): a `minor` signal
  caps the effective age below `adult_age`; an `adult` signal never loosens the DOB; latest signal wins; ignored while the flag
  is off.

## Open questions
- Legal review of per-country values before launch.
