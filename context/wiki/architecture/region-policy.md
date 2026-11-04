---
title: Region / jurisdiction policy
type: concept
status: proposed
updated: 2026-11-04
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

## Open questions
- Legal review of per-country values before launch.
