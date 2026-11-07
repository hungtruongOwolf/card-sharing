# 2026-11-07 — EM: slow down, choose the cutout and AI-effect models first

Source: EM chat (translated from Vietnamese) after sprint 10 phase 2 merged.

## EM message (translated)
"Right now we are going too fast and doing too much. We need to discuss again which model to use for cutting out and
which model to use for the effects and so on, first."

## Options presented by the lead (from the 2026-11-04 research plus a 2026-11-07 price refresh)
- Cutout: (A) keep BiRefNet_lite in the browser (115-224 MB, failed on the dev PC, too heavy for phone browsers);
  (B) light in-browser tap-to-select model (MediaPipe Interactive Segmenter "Magic Touch", ~6 MB, Apache-2.0, not
  pixel-perfect; or SlimSAM); (C) server cutout through an API (BiRefNet v2 / Bria RMBG 2.0 for the main subject,
  SAM 3 point prompts for "this item"); (D) self-hosted BiRefNet on a GPU (later). Native later: Apple Vision / ML Kit.
- AI effects: image-edit models (Nano Banana 2.x, Seedream 5.x, FLUX Kontext / FLUX.2 edit, Qwen-Image-Edit, GPT
  Image) through fal.ai so the model is a config change; Gemini API terms require 18+ users.
- Proposed a bake-off: about 30 photos, 3-4 cutout models and 3 effect models side by side, the EM picks by eye.

## EM decisions (Q&A)
1. Items people will cut: **all of** objects, pets, people/selfies, and photos with several items (pick one).
2. Cutout on the web test: **send the photo to a server/API (with consent), prefer the best-looking cut** (option C);
   native later uses Apple Vision / ML Kit on the device.
3. AI effect kinds: **style change, new background/scene, meme/funny expression — and "not sure, want to see"**.
4. **Run the bake-off before writing more code.**
