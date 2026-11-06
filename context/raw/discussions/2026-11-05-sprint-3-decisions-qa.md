# 2026-11-05 — Sprint 3 decisions (EM Q&A)
Translated from the EM's answers in chat.

1. A minor who turns 18 stays hidden from handle search until they turn "findable by handle" on themselves
   (the T-45 default of `true` on turning 18 is rejected).
2. Minor invite links: use cap 5 (with the 7-day TTL) — keep.
3. Caption term lists: served by the server through `/v1/config`, not bundled in the app (#213).
4. Keep the remaining defaults: default style die-cut with a subject and whole photo without; handle search padded to
   a 150 ms floor; revoking another device needs no confirm; locked effect tiles visible but not selectable.
