---
title: ADR-0005 Media pipeline, storage & CDN
type: decision
status: accepted
updated: 2026-11-04
sources: [raw/discussions/2026-11-04-infra-qa.md, raw/discussions/2026-11-04-security-qa.md]
related: [[overview]], [[api-mvp]], [[data-model]], [[security-baseline]], [[subject-cutout]], [[prd-mvp]], [[ADR-0006-job-queue]], [[threat-model-mvp]]
---
# ADR-0005: Media pipeline, storage & CDN

## Context
Cards are private photos shared with ≤ 20 friends. PRD: upload rendered card + mask (original only with "keep original", AC-13.4);
every image hash-matched against known CSAM **before delivery**, fail closed (US-28); every send also passes a nudity/sexual-content
classifier (on-device pre-check + server check, flagged ⇒ held for review, appeal ⇒ moderation queue; AC-28.3, EM decision D1);
originals are owner-only (AC-13.6); the OG image is a watermarked board snapshot (AC-24.7, D4); per-viewer signed URLs, TTL ≤ 10 min; public
pages show resized **watermarked** derivatives only (AC-24.5); EXIF stripped; ≤ 10 MB; re-encode + magic-byte check
([[security-baseline]] §4–5). Est. ~30 GB/day new media, egress-heavy (feeds, widgets).

## Options considered
| Topic | Option | Pros | Cons |
|---|---|---|---|
| Upload path | Presigned PUT direct to bucket | No API bandwidth | Size/type not enforceable on PUT for all providers (R2 lacks POST policy); validation only after the fact |
| | **Streamed through API to quarantine** | Enforce size, magic bytes, auth, idempotency at the boundary; provider-agnostic | API bandwidth (~1–10 uploads/s, fine for Rust); swap later behind `StorageProvider` |
| Scan placement | Client-side | — | Untrusted |
| | CDN-level scanning (e.g. CDN CSAM tool) | Zero code | Runs on serve, not before delivery; no control of evidence/reporting |
| | **Worker job before delivery** | Fail-closed gate, evidence + reporting flow under our control | Adds seconds to delivery (within p50 10 s budget) |
| Nudity check | On-device only | Instant, private | Bypassable by a modified client — advisory only |
| | **On-device pre-check + server classifier in the same worker job** | UX warning before upload; authoritative server verdict before delivery | Classifier latency + false positives (needs hold/appeal flow) |
| Delivery | Presigned GET straight from bucket | Simple | No edge cache; egress cost; per-viewer URLs defeat caching |
| | **CDN with edge-verified signed URLs, cache key = path** | Cached once, authorized per viewer; cheap egress | Need CDN that supports token auth; revocation only at TTL |

## Decision
- **Buckets (private, no public ACL):** `quarantine` (uploads, 48 h lifecycle), `clean` (scanned originals + derivatives),
  `legal-hold` (CSAM evidence, restricted IAM, object lock), `exports` (24 h lifecycle). Object keys are random 128-bit, no ids/names.
- **Upload:** `POST /v1/media` streams (≤ 10 MB, magic bytes PNG/JPEG/WebP/HEIC) into `quarantine`, dedupes `(owner, sha256, kind)`.
- **`media.process` job:** decode in a sandboxed, resource-limited task → re-encode (WebP/PNG, strip EXIF/metadata) → derivatives
  (`feed_1080`, `widget_512`, `thumb_256`) → **hash match via `HashMatcher` port** on the decoded image (provider options and proposed port changes: [[content-safety]]) → move to `clean`, mark media
  `clean`, then `cards.deliver`. Unavailable matcher ⇒ retry with backoff, card held (never delivered unscanned); match ⇒ block card,
  suspend account, copy to `legal-hold`, open `csam_incident`, queue report. Avatars, masks and originals go through the same gate.
- **Nudity/sexual-content classifier (AC-28.3, D1):** step after the hash match, via a `ContentClassifier` port, on every card render
  (and avatars). Score ≥ `moderation.nudity_threshold` ⇒ media `held` (stays in `quarantine`), card `held_review`, `content_holds` row,
  sender gets `card.held`; nothing is delivered and no URL is issued except to the owner. The owner may appeal once
  (`POST /v1/cards/{id}/appeal`) ⇒ `reports` row `source=appeal` in the moderation queue; `release_hold` ⇒ media `clean` + `cards.deliver`,
  `uphold_hold` ⇒ card `blocked` (+ optional warn). Unappealed holds expire after 30 d ⇒ card deleted. **Fail closed** like CSAM:
  classifier unavailable ⇒ retry with backoff, card stays `pending_scan`, alert at queue age > 5 min. Model id + score stored for audit.
- **On-device pre-check:** a small bundled classifier runs before upload; a hit shows a warning and the send does not upload unless the
  user chooses "Send for review" (then the server check decides). Advisory only — the server verdict is authoritative.
- **Originals (AC-13.6):** `original` media gets no recipient derivatives and is signed only for its owner; recipients only receive
  `feed_1080` / `widget_512` / `thumb_256`.
- **Delivery:** CDN in front of `clean` with **signed URLs verified at the edge** (signature over path + expiry, + viewer id for audit),
  cache key excludes signature; TTL 10 min (app/web), 5 min (staff), 24 h (export ZIP). Access decisions live in the module that
  returns the DTO; `media` only signs. Card delete ⇒ objects purged ≤ 24 h, outstanding URLs die at TTL (AC-14.1 accepted).
- **Public pages:** `sharing.snapshot` renders per-link watermarked derivatives (`public_wm`, `og`; ≤ 1080 px, brand mark) of the owner's
  cards only; the `og` image is a 1200×630 watermarked snapshot of the board with the owner's cards (friends' pins as private tiles,
  AC-24.7, D4); revoke ⇒ derivatives deleted + CDN purge. OG served via a token-checked route (`s-maxage` 60 s) that 302s to a fresh
  signed URL. **Accepted risk (D4):** social crawlers / chat apps that already fetched the preview keep their own copy after revoke; the
  owner is told once in the app; minors cannot create public links (AC-24.6).
  Implemented (T-130): derivatives are per **card** (not per link) in `sharing.public_media`, shared by every snapshot that shows
  the card, so revoking one link of several does not remove images another live link still uses; see [[api-mvp]] "Snapshots".
- **Provider:** behind `StorageProvider` + `UrlSigner` ports; S3-compatible storage + CDN with token auth and low/zero egress fees
  — **Cloudflare R2 + Cloudflare CDN** per [[ADR-0008-hosting]].

## Consequences
- Positive: one fail-closed gate for all images; private by default; edge caching despite per-viewer authorization; provider swappable.
- Negative / trade-offs accepted: deleted/revoked media viewable until URL TTL (≤ 10 min); API carries upload bandwidth; scanning adds
  latency and two hard dependencies (sends stop if the matcher or classifier is down — alerting required); classifier false positives
  delay legitimate cards until appeal review; OG previews can outlive revoke in third-party caches (D4); image decoding is attack
  surface (sandbox + limits).
- Follow-ups: pick hash provider (PRD Q9: PhotoDNA vs Thorn Safer) and document the reporting process with counsel; pick the classifier
  (self-hosted model vs vendor API, threshold tuned on a labeled set) and moderation staffing for appeals (D6); decoding library
  fuzz/limits spike; decide watermark style with design ([[subject-cutout]] for mask format).
