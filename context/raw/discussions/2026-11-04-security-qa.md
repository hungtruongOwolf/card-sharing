---
source: chat Q&A with EM (AskUserQuestion) on threat-model decisions
date: 2026-11-04
type: discussion
---
# Security decisions Q&A (EM answers)

| Decision | EM answer | Agent recommendation |
|---|---|---|
| D1 nudity classifier | **On for every send** | same |
| D3 widget on locked phone | **Always show the photo, like Locket** | redact by default (EM chose otherwise — accepted risk) |
| D4 public-link OG image | **Real board image with watermark** | generic image (EM chose otherwise — accepted risk) |
| D2 per-recipient invisible watermark | Post-beta | same |
| Board membership | Invitee must accept | same |
| Third-party cards on shared boards | "Private card" tile for non-recipients | same |
| D7 residual risks (screenshots, modified-client export, DOB lying) | Accept; deterrence (report, ban, attestation, later forensic watermark) | same |
