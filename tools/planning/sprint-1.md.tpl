---
title: Sprint 1 — identity slice + risk spikes
type: status
status: proposed
updated: 2026-11-04
sources: [raw/discussions/2026-11-04-prd-design-qa.md, raw/discussions/2026-11-04-security-qa.md, raw/discussions/2026-11-04-infra-qa.md]
related: [[mvp-plan]], [[prd-mvp]], [[threat-model-mvp]], [[api-mvp]], [[ADR-0004-auth-sessions]], [[region-policy]], [[subject-cutout]], [[effects-catalog]], [[design-system]], [[testing-strategy]], [[status]]
---
# Sprint 1 — identity slice + risk spikes
**TL;DR:** This is the first 2-week sprint after the walking skeleton (M0). It has {{S1_COUNT}} tickets, {{S1_SPIKES}} of them time-boxed spikes. It delivers one
vertical slice, **sign in → region age gate → onboarding**, on a staging build, behind a deny-by-default route gate. At the same
time it answers the riskiest unknowns of the whole MVP: cutout, holo, iOS widget push, Skia on the web, the nudity classifier and
the CSAM vendor. The full plan and traceability are in [[mvp-plan]]. The tickets also appear in `context/wiki/progress/backlog.json` (label `sprint-1`).

## Sprint goal
> A tester installs the staging build, signs in with Apple or Google, is refused or admitted by the region age gate (EU 16 / default 13
> from config), and lands on the camera-permission step. Every API route is deny-by-default and covered by the authz harness.
> The seven spikes each produce a go/no-go with measured numbers.

- **Dates:** planned 2026-11-16 → 2026-11-27. The sprint starts once the M0 exit criteria are met ([[mvp-plan]] section 2).
- **Entry criteria:** {{skeleton}}–{{platform}} done or close. Staging deploy works. EAS staging build installs on test devices.
- **Lanes and planned load (ideal days; a ~0.6 focus factor gives the lanes slack):**

{{S1_LANES}}

Allocation note: the Mobile engineer also takes {{sp_holo}} (holo spike, 2 d), which keeps Native at ~8 d. Backend B owns
{{authz_harness}}, {{rate_limiter}}, {{me_config}}, {{age_api}} and the two safety spikes. Backend A owns {{policy_fw}}, {{id_sessions}}, {{apple}} and {{google}}.

## Order of work
| Days | Backend A | Backend B | Mobile | Native | Web |
|---|---|---|---|---|---|
| 1–2 | {{policy_fw}} route gate | {{rate_limiter}} limiter; start {{sp_csam}} | {{ui_kit}} UI kit | {{sp_cut_ios}} | {{sp_skia_web}} |
| 3–5 | {{id_sessions}} sessions | {{authz_harness}} harness; {{sp_nudity}} | {{sp_holo}} | {{sp_cut_ios}} → {{sp_cut_android}} | — |
| 6–8 | {{apple}} Apple, {{google}} Google | {{me_config}} `/me` + config; {{age_api}} age gate | {{mobile_auth}} auth client | {{sp_cut_android}} → {{sp_widget_ios}} | — |
| 9–10 | fixes, review, staging demo | spike write-ups for EM | {{onboarding_shell}} onboarding | {{sp_widget_ios}} write-up | — |

## Tickets
{{S1_BLOCKS}}
## Vertical slice and parallelism
- **Slice:** {{policy_fw}} → {{id_sessions}} → {{apple}}/{{google}} → {{mobile_auth}} → {{age_api}} + {{me_config}} → {{onboarding_shell}}. It covers DB, API, generated
  client, UI and tests end to end, and it is deployable on its own.
- **Runs in parallel with no shared code:** all seven spikes. {{ui_kit}} only depends on the skeleton. {{authz_harness}} and {{rate_limiter}} only need the platform crate.
- **Contract first:** Backend A publishes the auth endpoints in OpenAPI on day 3, so {{mobile_auth}} can be built against MSW mocks before the API is deployed.

## Risks
| Risk | Mitigation |
|---|---|
| Spikes overrun and eat feature time | Hard time boxes. Stop at the box and report what is known. |
| Apple/Google console setup (bundle ids, services ids, App Attest/widget entitlements) blocks testing | Web/infra lane prepares the developer accounts in the first 2 days. The EM may need to grant access. |
| EM decisions on the CSAM vendor and classifier are not made by sprint end | The spikes hand over a recommendation with a decision date. A fake `HashMatcher` keeps M4 moving. |
| Refresh-token races look like token theft | Single-flight refresh on the client plus the 30 s grace window from [[ADR-0004-auth-sessions]]. |
| Region config values not legally reviewed | Use placeholder values from [[region-policy]]. Legal values land in M8. |

## Demo and exit checklist
- [ ] Staging build: Apple and Google sign-in, EU-16 refusal (neutral copy, 24 h cooldown), adult admission, camera explainer.
- [ ] CI: route without a policy fails; authz harness runs; refresh-reuse revokes the session family.
- [ ] Seven spike write-ups with go/no-go and numbers, ingested into the wiki (memory skill). Follow-up tickets re-estimated.
- [ ] EM has recommendations and decision dates for the CSAM vendor and the nudity classifier.
- Carry-over rule: an unfinished slice ticket moves to sprint 2 first. Unfinished spike questions become new spikes; the box is not extended.
