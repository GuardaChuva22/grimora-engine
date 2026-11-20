"""Cattle sounds, made from nothing: lows, a snort, a hoof scraping dirt, a horn hitting and a
dying groan. Written to assets/sounds/animals/. The game plays the lows lower for the bull and
higher for the calves.

A low is a buzzing throat (harmonics of a pitch that rises and falls, roughened) shaped by a
mouth that opens from "mmm" to "ooo" and closes again: the loudness of each harmonic comes
from how near it is to the mouth's resonances at that moment.

Run: python -I tools/gen_cattle_sounds.py
"""

import pathlib
import wave

import numpy as np

RATE = 44100
OUT = pathlib.Path(__file__).resolve().parent.parent / "assets/sounds/animals"


def write(name, samples, peak=0.85):
    samples = np.asarray(samples, dtype=np.float64)
    samples = samples / max(np.max(np.abs(samples)), 1e-9) * peak
    data = (samples * 32767).astype("<i2").tobytes()
    OUT.mkdir(parents=True, exist_ok=True)
    with wave.open(str(OUT / name), "wb") as f:
        f.setnchannels(1)
        f.setsampwidth(2)
        f.setframerate(RATE)
        f.writeframes(data)


def lowpass(x, cutoff):
    a = 1 - np.exp(-2 * np.pi * cutoff / RATE)
    y = np.empty_like(x)
    acc = 0.0
    for i, v in enumerate(x):
        acc += a * (v - acc)
        y[i] = acc
    return y


def bandpass(x, center, q):
    """Chamberlin state-variable filter, band output."""
    f = 2 * np.sin(np.pi * min(center, RATE / 6) / RATE)
    low = band = 0.0
    y = np.empty_like(x)
    for i, v in enumerate(x):
        low += f * band
        high = v - low - band / q
        band += f * high
        y[i] = band
    return y


def smooth_noise(rng, n, rate_hz):
    """Random wobble in -1..1, changing about `rate_hz` times a second."""
    points = max(int(n / RATE * rate_hz) + 2, 2)
    return np.interp(np.arange(n), np.linspace(0, n, points), rng.uniform(-1, 1, points))


def opening_guess(t):
    return np.clip((t - 0.2) / 0.3, 0, 1) * np.clip((1 - t) / 0.3, 0, 1)


def low(seed, seconds, pitch, rise, fall, open_at, rough, tilt=1.15, bright=1.0, nasal=0.5, cutoff=3000, vibrato=0.0):
    """`tilt`: how fast the harmonics fade (a cow's voice is dark, not buzzy); `bright`: how far the
    mouth opens its formants up; `nasal`: how much the closed-mouth hum (and the nose) carries."""
    rng = np.random.default_rng(seed)
    n = int(seconds * RATE)
    t = np.linspace(0, 1, n)
    # Pitch: up a little as the mouth opens, a long sag at the end.
    f0 = pitch * (1 + rise * np.sin(np.pi * np.clip(t / 0.55, 0, 1)) - fall * np.clip((t - 0.55) / 0.45, 0, 1) ** 1.5)
    f0 *= 1 + 0.01 * smooth_noise(rng, n, 6) + 0.006 * smooth_noise(rng, n, 23) + vibrato * np.sin(2 * np.pi * 5.5 * np.arange(n) / RATE) * opening_guess(t)
    phase = np.cumsum(f0) / RATE
    # The mouth: closed (mmm), open (ooo), closing again.
    opening = np.clip((t - 0.05) / max(open_at - 0.05, 0.01), 0, 1) ** 1.3 * np.clip((1 - t) / 0.22, 0, 1)
    formants = [
        (220 + 300 * opening * bright, 110 + 70 * opening, 1.0),
        (550 + 350 * opening * bright, 150, 0.45 * opening + 0.05),
        (1900, 260, 0.06 * opening),
        # The nose: a hum low down that stays while the mouth is shut.
        (180, 60, nasal * (1 - 0.6 * opening)),
    ]
    out = np.zeros(n)
    for k in range(1, 40):
        freq = k * f0
        amp = np.zeros(n)
        for center, width, gain in formants:
            amp += gain / (1 + ((freq - center) / width) ** 2)
        amp *= k ** -tilt * (freq < 4000)
        jitter = 1 + 0.05 * smooth_noise(rng, n, 14)
        out += amp * jitter * np.sin(2 * np.pi * k * phase + rng.uniform(0, 2 * np.pi))
    # A little roughness (subharmonic flutter) in the open middle, and breath through the nose.
    out *= 1 + rough * 0.18 * opening * np.sin(np.pi * phase)
    breath = lowpass(bandpass(rng.uniform(-1, 1, n), 700, 0.7), 2500) * (0.02 + 0.05 * (1 - opening))
    envelope = np.clip(t / 0.12, 0, 1) ** 1.2 * np.clip((1 - t) / 0.3, 0, 1) ** 1.4
    return lowpass(out + breath, cutoff) * envelope


def snort(seed):
    rng = np.random.default_rng(seed)
    out = np.zeros(int(0.9 * RATE))
    for start, length, loud in [(0.0, 0.32, 1.0), (0.42, 0.38, 0.8)]:
        n = int(length * RATE)
        t = np.linspace(0, 1, n)
        burst = rng.uniform(-1, 1, n) * (np.clip(t / 0.06, 0, 1) * (1 - t) ** 1.5)
        burst = bandpass(burst, 700, 0.7) + 0.5 * bandpass(burst, 1800, 1.5)
        flutter = 1 + 0.6 * np.sin(2 * np.pi * 38 * np.arange(n) / RATE)
        i = int(start * RATE)
        out[i : i + n] += burst * flutter * loud
    return out


def hoof_scrape(seed):
    rng = np.random.default_rng(seed)
    n = int(0.55 * RATE)
    t = np.linspace(0, 1, n)
    # A thud as the hoof goes in, then grit dragged back.
    thud = np.sin(2 * np.pi * 70 * t * 0.55) * np.exp(-t * 18)
    grit = rng.uniform(-1, 1, n) * (rng.random(n) < 0.3) * np.clip((t - 0.05) / 0.1, 0, 1) * (1 - t) ** 1.2
    grit = bandpass(grit, 1500, 0.8) + 0.6 * lowpass(grit, 500)
    return 1.4 * thud + grit


def horn_hit(seed):
    rng = np.random.default_rng(seed)
    n = int(0.45 * RATE)
    t = np.linspace(0, 1, n)
    body = np.sin(2 * np.pi * 85 * np.arange(n) / RATE * (1 - 0.3 * t)) * np.exp(-t * 14)
    crack = bandpass(rng.uniform(-1, 1, n), 1300, 1.2) * np.exp(-t * 30)
    return 1.3 * body + crack


def cow_death(seed):
    groan = low(seed, 2.6, 107.3, 0.06, 0.45, 0.2, 0.8, 0.551, 1.920, 0.913, 2500, 0.02)
    rng = np.random.default_rng(seed + 1)
    n = len(groan) + int(0.4 * RATE)
    out = np.zeros(n)
    out[: len(groan)] += groan
    # The body coming down at the end.
    i = int(2.1 * RATE)
    m = int(0.5 * RATE)
    t = np.linspace(0, 1, m)
    fall = np.sin(2 * np.pi * 55 * np.arange(m) / RATE) * np.exp(-t * 9) + lowpass(rng.uniform(-1, 1, m), 300) * np.exp(-t * 7)
    out[i : i + m] += 1.6 * fall
    return out


def main():
    # Picked by earshot (`earshot rank "a cow mooing"`) from a search over these knobs.
    write("moo_1.wav", low(40, 2.0, 119.2, 0.303, 0.362, 0.160, 0.000, 0.551, 2.400, 0.913, 2479, 0.0068), 0.6)
    write("moo_2.wav", low(39, 2.3, 138.9, 0.336, 0.364, 0.150, 0.367, 0.597, 2.323, 0.666, 3969, 0.0062), 0.6)
    write("moo_3.wav", low(36, 1.6, 123.0, 0.336, 0.342, 0.159, 0.046, 0.726, 2.400, 0.705, 3126, 0.0049), 0.6)
    write("snort.wav", snort(41), 0.7)
    write("hoof_scrape.wav", hoof_scrape(43), 0.7)
    write("horn_hit.wav", horn_hit(47), 0.9)
    write("cow_death.wav", cow_death(53), 0.85)
    print("cattle sounds written to", OUT)


if __name__ == "__main__":
    main()
