---
source: web research (vendor docs and pricing pages, AWS Marketplace listings, the Arachnid Shield OpenAPI spec, NCMEC API docs, Meta ThreatExchange source code, US/EU/UK law texts and law-firm and tracker summaries) fetched 2026-11-05; URLs inline
date: 2026-11-05
type: research
tickets: [T-136]
related: [SR-26, US-28, PRD Q9, L-4, ADR-0005]
---
# CSAM hash matching: vendors, open alternatives, reporting duties (T-136)

**Bottom line:** Every credible known-CSAM hash service that a small company can use **costs nothing in money**. What it costs is
**vetting time** and the need for a **legal entity**. The one paid option, Thorn Safer, starts at about **$30.7k/year** on AWS Marketplace,
which is far too expensive for us now. Recommended path:
(1) keep the **fake `HashMatcher`** in CI and dev; (2) **apply this sprint** to the free services: **Arachnid Shield** (C3P; free REST API with a
**PDQ-hash-only endpoint** and a built-in `test` classification), **Microsoft PhotoDNA Cloud** (free, vetted), and **NCMEC ESP registration**
(free; gives the CyberTipline reporting API with a test environment, plus separate Hash Sharing API credentials that include **PDQ** image
hashes); (3) at launch, run a **composite matcher**: Arachnid Shield, then PhotoDNA or a local NCMEC PDQ index, with any match counting as a match and any
outage failing closed. Add **Cloudflare's free CSAM Scanning Tool** on the media domain as defence in depth, not as the gate. The reporting duties
(US 18 U.S.C. §2258A, UK OSA s.66 to the NCA since 2026-04-07) apply **from the first external user, closed beta included**. Lead times are
**not published by any vendor**, so apply early.

---

## 1. Context from our docs
- ADR-0005: a `media.process` job re-encodes, makes derivatives, then runs a **hash match via the `HashMatcher` port** on the decoded image *before*
  delivery. It fails closed (retry and hold if the matcher is unavailable). A match ⇒ block the card, suspend the account, copy to `legal-hold`,
  open a `csam_incident`, queue a report. ADR-0005 already rejects CDN-level scanning as the *primary* control because it "runs on serve, not before
  delivery". (`context/wiki/architecture/decisions/ADR-0005-media-pipeline.md`)
- Threat model A-19: cutout, borders and effects defeat perceptual hashes, and "keep original" is off by default, so the source photo is often
  never seen. Hash recall on our renders will be lower than on raw photos. The nudity classifier (T-137) and user reports are the backstop.
- L-4 (threat model): NCMEC reporting, retention under the REPORT Act, EU interim derogation status, evidence access, vendor DPA.
- EM constraint (2026-11-05): no paid accounts now; free first; paid services later.

## 2. Comparison table

| | Microsoft PhotoDNA Cloud | Arachnid Shield (C3P) | NCMEC Hash Sharing + local PDQ (HMA / own Rust index) | Thorn Safer Match | Cloudflare CSAM Scanning Tool | Google CSAI Match / Content Safety API |
|---|---|---|---|---|---|---|
| What | Perceptual hash (PhotoDNA) match vs. known-CSAM lists incl. NCMEC | Exact + near match (perceptual / PhotoDNA) vs. Project Arachnid list | Pull hash lists (MD5, SHA1, PDNA, **PDQ** …) and match locally | Crypto + perceptual hash match, 172.5M+ hashes; optional classifiers | Fuzzy-hash scan of content served through CF cache; blocks URL | CSAI Match: **video** fingerprints vs. YouTube DB. Content Safety API: **classifier** for unseen imagery (not hash) |
| API shape | REST; image (file or URL) → match result (exact schema not public, unverified) | REST: `POST /v1/media` (raw bytes), `POST /v1/pdq` (base64 PDQ hashes), `POST /v1/url`, `GET /v1/version`; result `classification` ∈ {csam, harmful-abusive-material, **test**, no-known-match}, `match_type` exact/near | NCMEC Hash Sharing REST API (separate creds) → local DB; we compute PDQ (pdqhash crate, Apache-2.0) and do a Hamming-distance lookup | Hosted API (Match). Self-hosted only with Enterprise | Dashboard toggle; daily email of matches | Partner APIs (details only after approval) |
| Where it runs / data | Azure; image sent to MS, "never retained" | Canada-run service (data location not stated, unverified). **PDQ endpoint = only a 256-bit hash leaves our infra** | Entirely in our infra; no image leaves | Vendor API or self-hosted | Cloudflare edge | Google |
| Latency | Not published (unverified) | Not published (unverified) | Local: PDQ compute + index lookup, ms-level (estimate, to measure) | Not published | Async (daily email) | Not published |
| Cost | **Free** for qualified orgs | **Free** | **Free** (hashes free to ESPs; HMA BSD, Coop Apache-2.0) | **$30,720/yr** at 1M queries/mo (Match); Enterprise $45,384/yr | **Free** on all plans | **Free** for approved partners |
| Eligibility / onboarding | Online application, case-by-case third-party vetting; **time not stated** | ESP sign-up; approval criteria and time not stated (unverified) | NCMEC ESP registration (email espteam@ncmec.org); hash sharing creds issued separately; criteria and time not stated | AWS Marketplace contract or sales; 12-month term | Verified email only (NCMEC creds no longer needed since 2025-04) | Interest form, approval required |
| Test-image support | Integration environment with **benign images that simulate a hit** | **`test` classification** in the API enum | NCMEC **test** environments for hash sharing and CyberTipline | Not documented publicly | n/a | n/a |
| Reporting help | MS recommends reporting to NCMEC through the PhotoDNA API; MS may send NCMEC aggregate reports | Not documented; `POST /v1/media/submit` lets us send suspected **new** CSAM for analyst review | We report via CyberTipline API ourselves (ROOST Coop has a ready NCMEC integration) | Connects to our NCMEC or RCMP account | CF files third-party reports to NCMEC; **we must still report ourselves** | Partner reports under local law |
| Fit for our gate (before delivery) | Yes | Yes | Yes | Yes | **No** (on serve, after delivery) | No (video only / classifier) |

Sources: PhotoDNA [cloud service](https://www.microsoft.com/en-us/photodna/cloudservice), [FAQ](https://www.microsoft.com/en-us/photodna/faq);
Arachnid Shield [overview](https://projectarachnid.com/shield), [OpenAPI spec](https://shield.projectarachnid.com/openapi.json);
NCMEC hash API client in Meta's [ThreatExchange `hash_api.py`](https://raw.githubusercontent.com/facebook/ThreatExchange/main/python-threatexchange/threatexchange/exchanges/clients/ncmec/hash_api.py)
and [`ncmec_api.py`](https://raw.githubusercontent.com/facebook/ThreatExchange/main/python-threatexchange/threatexchange/exchanges/impl/ncmec_api.py) (image PDQ and PDNA, video MD5),
[ThreatExchange repo (BSD)](https://github.com/facebook/ThreatExchange), [HMA roadmap](https://facebook.github.io/ThreatExchange/hma/roadmap.html),
[ROOST Coop (Apache-2.0)](https://github.com/roostorg/coop), [Coop NCMEC integration](https://docs.roost.tools/coop/1.0/integrations/ncmec.html),
[pdqhash crate](https://crates.io/crates/pdqhash) (Apache-2.0, v0.1.1, last release 2022, so check maintenance),
NCMEC hashes free to ESPs ([Childhood Stella Polaris summary](https://stellapolaris.childhood.se/natural-language-processing-datorlingvistik/verktyg-i-databasen-som-anvnder-ljud-ai/ncmec-hash-sharing-platform), secondary source);
Safer [Match on AWS](https://aws.amazon.com/marketplace/pp/prodview-dfwekn4bx4ake), [Enterprise on AWS](https://aws.amazon.com/marketplace/pp/prodview-unzbqt3w6pqbg), [safer.io](https://safer.io/);
Cloudflare [docs](https://developers.cloudflare.com/cache/reference/csam-scanning/), [launch blog (free for all plans)](https://blog.cloudflare.com/the-csam-scanning-tool/),
[INHOPE 2025-04-15 (NCMEC creds removed, non-US allowed, CF reports to NCMEC)](https://inhope.org/EN/articles/cloudflare-simplifying-the-fight-against-csam);
Google [Child Safety Toolkit](https://protectingchildren.google/tools-for-partners/).

### Notes per option
- **PhotoDNA Cloud.** Free, images only (the FAQ says video is not covered), REST, keys only after vetting. On-prem PhotoDNA is licensed separately, mainly for
  law enforcement and tool vendors. A community client uses base URL `https://api.microsoftmoderator.com/photodna/v1.0/` ([packagist](https://packagist.org/packages/dustindoiron/photodna-client)):
  **unverified, not official**. The DPA and data region are not on the public pages, so ask during onboarding.
- **Arachnid Shield.** Best fit for "free now". The **`/v1/pdq` endpoint** lets us compute PDQ on our worker and send only hashes, which minimises the transfer of personal data
  and the GDPR/DPA surface. The `test` classification lets us run an end-to-end fail-closed test with no illegal material. The list is C3P's, not NCMEC's,
  so coverage differs. Terms of use, approval process, rate limits and data location were **not found publicly** (unverified).
- **NCMEC Hash Sharing + local matching.** No per-call dependency on a vendor and no image leaves our infra. But we must store and refresh the list, tune the PDQ
  distance (HMA defaults to 0–31; tighter means more precision and less recall, per the [HMA roadmap](https://facebook.github.io/ThreatExchange/hma/roadmap.html)),
  and keep the list access-controlled. Meta's HMA is Python and AWS-oriented (Docker image available). For a Rust backend, a small in-process PDQ index is
  simpler. Treat this as **launch-time or post-launch**, after NCMEC approves hash-sharing access.
- **Thorn Safer.** The strongest product (largest aggregated list, scene video hashing, CSAM classifier, moderation UI), but the smallest public tier is
  **1M queries/month for $30,720/12 months** (Match) and $45,384 (Enterprise). There is no free tier. Hive resells Thorn's CSAM detection as "Contact Sales"
  ([Hive/Deploy pricing](https://deploy.com/pricing)). Revisit only if volume, video, or a regulator's expectations justify it.
- **Cloudflare CSAM Scanning Tool.** Free, one toggle. But it only covers content **served through the Cloudflare cache**, it reports daily, and it blocks after the fact.
  R2 content only passes through the cache when the bucket has a **custom domain** (r2.dev URLs do not support caching), per Cloudflare's R2/cache docs
  ([preview of R2 cache doc](https://codex-container-api-docs.previews.developers.cloudflare.com/cache/interaction-cloudflare-products/r2/index.md), unofficial preview host).
  Whether the tool scans R2 custom-domain traffic is **unverified**, so confirm with Cloudflare. It fits ADR-0005 only as an extra net.
- **Google.** CSAI Match is video only. The Content Safety API is a *classifier* for previously unseen CSAM, which could be a phase-2 add-on for prioritising human review.
  It is not a hash matcher.

## 3. `HashMatcher` port: confirmed, with adjustments
The ADR-0005 port works for all the gate-capable options above. Proposed contract (Rust sketch, for T-138; not code in the repo yet):

```rust
/// Precomputed once per media item by the worker, so adapters never re-decode.
pub struct MatchInput<'a> {
    pub media_id: MediaId,
    pub encoded: &'a [u8],          // re-encoded derivative (for PhotoDNA / Arachnid /v1/media)
    pub sha256: [u8; 32],
    pub pdq: Option<PdqHash>,       // 256-bit + quality; computed in-process (pdqhash, Apache-2.0)
}

pub enum MatchVerdict {
    NoMatch,
    Match { class: MatchClass, kind: MatchKind },   // kind: Exact | Near { distance: u16 }
}
pub enum MatchClass { Csam, HarmfulAbusive, Test }  // `Test` = vendor test image: run the incident flow, never report to NCMEC

pub struct MatchResult {
    pub verdict: MatchVerdict,
    pub provider: &'static str,      // "arachnid", "photodna", "ncmec-pdq-local", "fake"
    pub list_version: Option<String>,// Arachnid GET /v1/version; local list sync timestamp
    pub provider_ref: Option<String>,// opaque vendor match id, for the incident record
}

#[async_trait]
pub trait HashMatcher: Send + Sync {
    async fn check(&self, input: &MatchInput<'_>) -> Result<MatchResult, MatchError>;
}
pub enum MatchError { Unavailable, RateLimited { retry_after: Duration }, Rejected(String) } // every error ⇒ card stays held (fail closed)
```
Adjustments to the ADR:
1. **Composite adapter** (`AnyOfMatcher`): calls all configured matchers; **any match ⇒ match**; **any error ⇒ error** (fail closed). Which matchers run is set in config,
   not code (CLAUDE.md rule 8).
2. **Reporting is a separate port** (`CsamReporter` → NCMEC CyberTipline API; later an NCA adapter). Vendors differ on reporting, and the evidence and report flow must
   not depend on which matcher fired.
3. **`Test` class:** lets staging and production run the vendor test image end to end (block, hold, incident) without filing a real report. This covers the threat-model test
   "Vendor test image → blocked, no URL, account suspended".
4. **Store `provider`, `list_version`, `verdict`** on the media row for audit. This also enables a **re-scan when lists update** (later; not MVP).
5. **What to hash:** every stored image (render, mask, original if kept, avatar), as ADR-0005 already says. A client-supplied PDQ of the pre-cutout photo would be
   untrusted, so it adds nothing as a gate. It is not recommended for MVP (open question).

## 4. Legal obligations (summary; counsel must confirm)

### United States: 18 U.S.C. §2258A ([LII](https://www.law.cornell.edu/uscode/text/18/2258A))
- A provider must report to NCMEC's CyberTipline **"as soon as reasonably possible after obtaining actual knowledge"** of apparent violations (child pornography
  sections §§2251, 2251A, 2252, 2252A, 2252B, 2260; since the 2024 REPORT Act also §1591 involving a minor and §2422(b)). It must give NCMEC its contact details and a
  point of contact.
- **No duty to scan** (§2258A(f)). Scanning is our product decision (US-28), not a legal requirement in the US. **Reporting after knowledge is** mandatory.
- **Preservation:** a report counts as a preservation request for **1 year** (was 90 days before 2024). Storage must be secure, with limited access, following the NIST
  Cybersecurity Framework. Voluntary longer retention is allowed. ⇒ Our `legal-hold` bucket needs object lock ≥ 1 year, restricted IAM and an access log.
- **Penalties** for *knowing and wilful* failure to report: up to $600k for a first failure and $850k after that (providers under 100M MAU).
- Applicability to a provider **not incorporated in the US** that serves US users needs counsel's view (open). NCMEC registration is open to ESPs by email:
  espteam@ncmec.org ([NCMEC](https://www.missingkids.org/theissues/csam)).
- **CyberTipline Reporting API** ([docs](https://report.cybertip.org/ispws/documentation/index.html)): REST + XML, HTTP basic credentials from NCMEC.
  Flow: `POST /submit` (report XML) → `POST /upload` (file) → `POST /fileinfo` → `POST /finish` (or `/retract`). Production base `https://report.cybertip.org/ispws`,
  **test base `https://exttest.cybertip.org/ispws`**. Unfinished reports are deleted after 24 h. Test and production credentials are separate
  ([Coop docs](https://docs.roost.tools/coop/1.0/integrations/ncmec.html)).

### United Kingdom: Online Safety Act 2023
- **s.66 duty to report CSEA content to the NCA** has been in force for user-to-user services since **2026-04-07** (Online Safety (CSEA Content Reporting by Regulated
  User-to-User Service Providers) Regulations 2026, SI 2026/268). Providers must **register with the NCA**. Non-UK providers report **UK-linked** detected
  and unreported content. Content already reported to a corresponding foreign body such as **NCMEC counts as "reported"** and is not reported again
  ([ZwillGen](https://www.zwillgen.com/eu-uk/new-uk-csea-reporting-rule-takes-effect-user-to-user-services/), [SI 2026/268](https://www.legislation.gov.uk/uksi/2026/268/regulation/2/made),
  [s.66](https://www.legislation.gov.uk/ukpga/2023/50/section/66)). Whether NCA registration is still needed when we report everything to NCMEC is a question for counsel.
- **Ofcom Illegal Content Codes:** perceptual hash matching for CSAM is *recommended* (the safe-harbour measure) for large U2U services at medium or high risk, U2U services at high
  risk with > 700k UK MAU, and **file-storage or file-sharing services at high risk regardless of size**. Ofcom also recommends a policy for reviewing hash hits
  (summarised in search results from [Ofcom file-sharing guidance](https://www.ofcom.org.uk/online-safety/file-sharing-and-file-storage-know-the-online-safety-risks-the-rules-and-how-to-comply);
  the page returned 403 to our fetcher, so the **exact code text is unverified**). We will likely not be "large", but we hash anyway (US-28).
- The OSA illegal-content **risk assessment** duty for all regulated U2U services is from background knowledge and was **not re-verified in this session**.

### European Union
- **Interim Regulation (EU) 2021/1232** (the ePrivacy derogation for *voluntary* CSAM detection by number-independent interpersonal communications services) **lapsed on
  2026-04-03** after Parliament rejected an extension. It was then **reinstated by Regulation (EU) 2026/1881** of 24 July 2026, in force on **2026-07-31**, applying **until 2028-04-03** (a gap of about 4 months with no derogation).
  It does not apply to end-to-end encrypted communications ([eucrim](https://eucrim.eu/news/interim-rules-on-voluntary-csam-detection-reinstated-until-2028/),
  [Digital Policy Alert](https://digitalpolicyalert.org/change/17548), [EP press release 2026-03-25](https://www.europarl.europa.eu/news/en/press-room/20260325IPR39207)).
  The conditions providers must meet under the derogation (DPIA, reporting, and so on) were **not read from the Official Journal text**, so this is unverified.
  **Open for counsel:** is sending private cards to ≤ 20 friends a "number-independent interpersonal communications service", so that the derogation is what makes
  our scanning lawful under ePrivacy? Or is it a hosting service where GDPR alone (legitimate interest / legal obligation) applies? Watch the 2028 expiry
  and the permanent CSA Regulation.
- **DSA (Reg. 2022/2065):** notice-and-action and Art. 18 (notify authorities of suspected crimes threatening life or safety) apply to hosting providers.
  This is background knowledge, **not re-verified in this session** ([EUR-Lex](https://eur-lex.europa.eu/eli/reg/2022/2065/oj)).

### App stores
- Apple Guideline 1.2 requires UGC apps to have "a method for filtering objectionable material", reporting, blocking and published contact information
  ([App Review Guidelines](https://developer.apple.com/app-store/review/guidelines/)). This is a store requirement for beta (TestFlight external) and launch.

## 5. Legal steps (L-4) with owners

| # | Step | Owner | When | Cost |
|---|---|---|---|---|
| L-4.1 | Decide the **legal entity** that will be the "provider" (vendors and NCMEC vet *organisations*; we need a website, privacy policy, ToS, contact) | EM | Before applying (now) | Incorporation cost (EM) |
| L-4.2 | Name the **CSAM point of contact** and a deputy (NCMEC, NCA, vendor contact) | EM | S1 | 0 |
| L-4.3 | Apply to **Arachnid Shield** and **PhotoDNA Cloud** | EM (sign), Backend B (technical form) | S1–S2 | Free |
| L-4.4 | **NCMEC ESP registration**: CyberTipline API test and production credentials, plus Hash Sharing API access | EM | S1–S2 | Free |
| L-4.5 | Counsel review: §2258A applicability to our entity; UK NCA registration need; EU derogation scope (NI-ICS vs hosting); DSA duties; DPA with each vendor; lawful basis in the privacy notice | Counsel (EM engages) | Before closed beta | Counsel fees (EM) |
| L-4.6 | Written **CSAM incident SOP**: no viewing beyond what is needed, who may access `legal-hold`, report within the "as soon as reasonably possible" window, 1-year preservation, law-enforcement request handling, staff wellbeing | Counsel + EM; Backend B for tooling | Before closed beta | 0 |
| L-4.7 | Retention config: `legal-hold` object lock ≥ 1 year, access log, NIST-CSF-aligned controls | Backend B / platform | S7 | Storage only |
| L-4.8 | UK NCA registration (if counsel says yes) | EM | Before UK users | Free (assumed) |
| L-4.9 | Privacy notice and ToS text on automated scanning and reporting | Counsel + EM | Before closed beta | — |

## 6. Recommendation for the EM

- **Free now (dev + closed beta):**
  1. Keep the **`FakeHashMatcher`** (supports a `Test` verdict) in CI and dev. The build is never blocked.
  2. **Apply now** (all free): Arachnid Shield (primary: PDQ-hash-only, so no image transfer; has a `test` class), PhotoDNA Cloud (second source), NCMEC ESP registration
     (CyberTipline API test environment and hash sharing). Applying needs the legal entity (L-4.1).
  3. Build the `CsamReporter` against the **CyberTipline test environment** (`exttest.cybertip.org`). That needs no real content.
  4. When the media custom domain exists, toggle **Cloudflare CSAM Scanning** on it (free extra net, not the gate).
- **Launch-time (must exist before any external user, closed beta included):** at least **one real matcher in production** behind the composite adapter (Arachnid Shield;
  add PhotoDNA or a local NCMEC PDQ index when approved); **NCMEC registration and reporting live**; incident SOP and 1-year preservation; counsel sign-off on L-4.5.
  Safer stays **deferred** (≈ $30.7k/yr minimum) until scale, video or a regulator's expectations justify it.
- **Decision date:** **2026-11-18** (end of sprint 1) for the entity and point of contact (L-4.1, L-4.2) and the go-ahead to apply. Real adapter in S7 (≈ wk 15–16) per the MVP plan.
  If no vendor has approved us by the start of S7, beta is blocked by the fail-closed gate, as designed.
- **Risks:**
  - Onboarding lead time is **unknown** (no vendor publishes it), so apply in parallel.
  - Vetting may require an incorporated entity with a public site.
  - Perceptual hashes have low recall on cutouts and effects (A-19), which the nudity classifier and reports must cover.
  - Coverage differs by list: Arachnid ≠ NCMEC ≠ IWF.
  - The EU derogation lapsed once (2026-04 → 2026-07) and expires on 2028-04-03.
  - Hard dependency: if the matcher is down, sends stop (alerting is required).
  - The pdqhash crate is lightly maintained (last release 2022).

## Open questions
- Is our service an NI-ICS under the ePrivacy Directive (EU derogation scope) or only hosting? (counsel)
- Does §2258A bind our entity given where it is incorporated? Do we still need NCA registration if we report all UK-linked hits to NCMEC? (counsel)
- What are Arachnid Shield's terms, rate limits, data location and approval time? What are PhotoDNA's DPA, region and latency? (ask the vendors during onboarding)
- Does Cloudflare's CSAM tool scan R2 custom-domain traffic? (ask Cloudflare)
- Should we also re-scan stored media when hash lists update (post-MVP)?
