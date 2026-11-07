"""Generate the app's touch-feedback sounds (design-system.md section 3.7) as tiny mono WAV files.

The sounds are synthesized here from sine waves and seeded noise, so they are our own work with no third-party
license. Re-run after changing a recipe:

    python tools/dev/gen_feedback_sounds.py

Output: apps/mobile/assets/sounds/{squeak,pop,whoosh}.wav (22.05 kHz, 16-bit PCM, a few KB each).
"""

from __future__ import annotations

import math
import random
import struct
import wave
from pathlib import Path

RATE = 22_050
OUT = Path(__file__).resolve().parents[2] / "apps" / "mobile" / "assets" / "sounds"


def envelope(i: int, n: int, attack: float, release: float) -> float:
    """Linear attack, exponential-ish release, so clips never start or end with a click."""
    t = i / n
    if t < attack:
        return t / attack
    return max(0.0, 1.0 - (t - attack) / (1.0 - attack)) ** release


def squeak() -> list[float]:
    """A rubbery toy squeak: a short upward chirp with a little vibrato."""
    n = int(RATE * 0.14)
    phase, out = 0.0, []
    for i in range(n):
        t = i / RATE
        freq = 900 + 1500 * (i / n) + 60 * math.sin(2 * math.pi * 38 * t)
        phase += 2 * math.pi * freq / RATE
        tone = math.sin(phase) + 0.25 * math.sin(2 * phase)
        out.append(0.5 * tone * envelope(i, n, 0.08, 1.6))
    return out


def pop() -> list[float]:
    """A soft bubble pop: a fast downward sine sweep with a quick decay."""
    n = int(RATE * 0.07)
    phase, out = 0.0, []
    for i in range(n):
        freq = 1200 - 800 * (i / n)
        phase += 2 * math.pi * freq / RATE
        out.append(0.6 * math.sin(phase) * envelope(i, n, 0.03, 3.0))
    return out


def whoosh() -> list[float]:
    """A send whoosh: seeded noise through a one-pole low-pass whose cutoff sweeps up and back."""
    rng = random.Random(246)
    n = int(RATE * 0.28)
    low, out = 0.0, []
    for i in range(n):
        sweep = math.sin(math.pi * i / n)
        alpha = 0.02 + 0.25 * sweep
        low += alpha * (rng.uniform(-1.0, 1.0) - low)
        out.append(1.6 * low * envelope(i, n, 0.35, 1.2))
    return out


def write(name: str, samples: list[float]) -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    with wave.open(str(OUT / f"{name}.wav"), "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(RATE)
        clipped = (max(-1.0, min(1.0, s)) for s in samples)
        wav.writeframes(b"".join(struct.pack("<h", int(s * 32_000)) for s in clipped))


if __name__ == "__main__":
    for name, recipe in (("squeak", squeak), ("pop", pop), ("whoosh", whoosh)):
        write(name, recipe())
        print(f"wrote assets/sounds/{name}.wav")
