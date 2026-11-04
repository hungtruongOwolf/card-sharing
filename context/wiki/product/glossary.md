---
title: Glossary
type: concept
status: draft
updated: 2026-11-04
sources: [raw/discussions/2026-11-04-idea-round-3.md]
related: [[vision]], [[prd-mvp]], [[effects-catalog]]
---
# Glossary
Shared vocabulary. Code, UI copy and docs must use these exact terms.

| Term | Meaning | Not to be confused with |
|---|---|---|
| Capture | Original photo taken in-app or imported. Private; never shown to others unless sent as a card. | Card |
| Mask | 8-bit subject mask from on-device segmentation; source of truth for cut styles ([[subject-cutout]]). | Cutout image |
| Card | Cutout + cut style + finish + optional effect + optional caption (≤ 80 chars). Unit of sending/pinning. | Capture, Pin |
| Cut style | Shape of the cutout edge: die-cut border, torn paper, shape clip… ([[effects-catalog]]) | Finish |
| Finish | Surface look/feel: matte, glossy, holo (gyro), plus squish physics, haptics, sound. | Effect |
| Effect | Registry entry applied to a card (shader, particles, anchored prop…), tiered per [[ADR-0003-effects-tiers]]. | Finish |
| Board | Scrapbook canvas of pinned cards + decorations. Personal or shared. | Album (Phase 2) |
| Pin | Placement of a card/decoration on a board (position, scale, rotation, z-order). References the card, does not copy it. | Card |
| Public link | Unguessable, revocable token URL rendering a board on the web preview page ([[prd-mvp]]). | Share-out export |
