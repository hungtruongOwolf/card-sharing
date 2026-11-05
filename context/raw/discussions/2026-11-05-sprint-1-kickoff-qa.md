---
source: chat Q&A with EM (AskUserQuestion) + chat
date: 2026-11-05
type: discussion
---
# Sprint 1 kickoff Q&A (EM answers)

| Question | EM answer |
|---|---|
| Accounts for staging/app builds (Apple Developer, Google Play/Cloud, Fly.io + Cloudflare + Upstash, Expo EAS) | "None yet. Prioritize free options and do the rest later; I will register properly later if the app idea and execution are good." |
| Test devices | iPhone only (no Mac, no Android phone) |
| How to run sprint 1 on a 16 GB machine | 3 parallel lanes (backend Rust sequential; mobile UI kit; desk-research spikes) |

Earlier the same session (2026-11-04/05): the EM rejected the VHDX dev-drive approach and moving the repo to C:; "download the
needed libraries on C: but keep running on E: as normal". Implemented as the symlink-free pnpm layout (commit 738b81e).

Facts found while starting:
- GitHub branch protection is unavailable for this private repo on GitHub Free (HTTP 403 "Upgrade to GitHub Pro or make this
  repository public"). CI therefore cannot be made a required check yet.
- `pnpm audit --prod` reports 2 high advisories, both transitive via Expo with no compatible fix (decode-uri-component via
  expo-router > query-string@7; zod@3 via @expo/cli).

## Follow-up Q&A after T-33 (same day)
| Question | EM answer |
|---|---|
| Tree snapshots instead of pixel screenshots for UI tests | Snapshots now, pixel screenshots later (once device builds exist) |
| When to bundle brand fonts (OFL) | With the next mobile ticket (T-21), to pay the slow install only once |
| Contrast fixes: light link/focus #3E7FA8 → #336C91; new `accent.onMustard` for dark mode | Approved |

## Follow-up Q&A during T-31/T-32 (same day)
| Question | EM answer |
|---|---|
| How to run backend tests after a low-memory kill | Low-RAM mode now: 1 build job + 1 test thread |
| Handle typo during onboarding vs 30-day change cooldown | Grace: free change for 24 h after the first claim; the 30-day cooldown applies after that |

## Follow-up Q&A after T-126 (same day)
| Question | EM answer |
|---|---|
| Share page enlarge: baked only, baked + small WebGL holo, or CanvasKit on fast networks | Baked + small WebGL holo (~5 KB, starts < 1 s) |
| Share-page LCP still limited to ~100 KB (WebP/AVIF ~600 px) | Accepted |

## Follow-up Q&A after T-52 (same day)
| Question | EM answer |
|---|---|
| How to get on-device holo frame-time numbers (only an iPhone available) | Borrow a mid-range Android phone; lead prepares the APK and test script |
| Ambient holo animation when the phone is still | All visible cards shimmer (not only the focused one) |
| Approve the new thin-film holo look as the shared base | Approve for now; fine-tune on a device later |

## Follow-up Q&A during sprint 2 (same day)
| Question | EM answer |
|---|---|
| Devices without on-device subject cutout (iOS < 17, Android without the bundled model) | Offer whole-photo card styles with mask-free shape cuts (rounded square, circle, heart, torn edge); subject cutout appears when the device supports it |
| Next mobile work after the sprint-2 lane finished early | Card editor (T-64) + captions (T-69) |
| Delete the merged leftover branch feat/t61-contours | Delete |
