"""Generates assets/sounds: the sandbox's ambience, torch fire, crystal hum and footsteps.

Everything is synthesized from seeded noise and sine waves, so the files are
original and part of the project, and the same seeds always give the same
bytes. Loops are seamless: noise repeats with the loop length, filters run
over it twice so their state wraps around, and every modulation completes a
whole number of cycles. Standard library only:

    python -I tools/gen_sounds.py
"""

import math
import os
import random
import struct
import wave

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "sounds")
RATE = 22050
TAU = 2 * math.pi


# ---------------------------------------------------------------- helpers

def one_pole(cutoff):
    """Coefficient of a one-pole lowpass: y += a * (x - y)."""
    return 1 - math.exp(-TAU * cutoff / RATE)


class Svf:
    """Chamberlin state-variable filter; `step` returns (low, band, high)."""

    def __init__(self):
        self.low = self.band = 0.0

    def step(self, x, cutoff, q):
        f = 2 * math.sin(math.pi * min(cutoff, RATE / 6) / RATE)
        self.low += f * self.band
        high = x - self.low - self.band / q
        self.band += f * high
        return self.low, self.band, high


def periodic(rng, length, cycles, weights=None):
    """Smooth random wave in -1..1 that repeats exactly every `length` seconds."""
    parts = [(k, rng.uniform(0, TAU), (weights or {}).get(k, 1 / k)) for k in cycles]
    norm = sum(w for _, _, w in parts)
    return lambda t: sum(w * math.sin(TAU * k * t / length + ph) for k, ph, w in parts) / norm


def render_loop(seconds, sample):
    """Runs `sample(i, t)` over the loop twice and keeps the second pass, so
    every filter's state at the end matches the start: no click at the seam."""
    n = int(seconds * RATE)
    out = [0.0] * n
    for rep in range(2):
        for i in range(n):
            v = sample(i, i / RATE)
            if rep:
                out[i] = v
    return out


def normalize(samples, peak):
    top = max(abs(s) for s in samples) or 1.0
    return [s * peak / top for s in samples]


def write_wav(name, samples):
    path = os.path.join(OUT, name)
    data = b"".join(struct.pack("<h", int(max(-1.0, min(1.0, s)) * 32767)) for s in samples)
    with wave.open(path, "wb") as f:
        f.setnchannels(1)
        f.setsampwidth(2)
        f.setframerate(RATE)
        f.writeframes(data)
    print(f"wrote {os.path.normpath(path)} ({len(samples) / RATE:.2f} s, {len(data) // 1024} KiB)")


# ---------------------------------------------------------------- sounds

def wind():
    """Soft valley wind: darkened noise that swells in gusts, with a faint whistle."""
    length = 8.0
    rng = random.Random(41)
    noise = [rng.uniform(-1, 1) for _ in range(int(length * RATE))]
    gust = periodic(rng, length, [1, 2, 3, 5])
    whistle_pitch = periodic(rng, length, [1, 3, 4])
    state = {"a": 0.0, "b": 0.0, "sub": 0.0}
    a_sub = one_pole(140)
    svf = Svf()

    def sample(i, t):
        g = 0.5 + 0.5 * gust(t)  # 0..1
        a = one_pole(380 + 900 * g)
        state["a"] += a * (noise[i] - state["a"])
        state["b"] += a * (state["a"] - state["b"])
        # Remove the deep rumble: small speakers can't play it and it reads as mic noise.
        state["sub"] += a_sub * (state["b"] - state["sub"])
        _, band, _ = svf.step(noise[i], 520 + 300 * whistle_pitch(t), 9.0)
        return ((state["b"] - state["sub"]) * 2.2 + band * 0.04 * g * g) * (0.3 + 0.7 * g)

    return normalize(render_loop(length, sample), 0.6)


def fire():
    """Torch fire: a low breathing roar, a little hiss and random crackles and pops."""
    length = 4.0
    n = int(length * RATE)
    rng = random.Random(7)
    noise = [rng.uniform(-1, 1) for _ in range(n)]
    flutter = periodic(rng, length, [3, 5, 8, 13, 21], {3: 0.5, 5: 0.4, 8: 0.35, 13: 0.3, 21: 0.25})
    state = {"lo1": 0.0, "lo2": 0.0, "sub": 0.0, "hp": 0.0}
    a_lo, a_sub, a_hp = one_pole(600), one_pole(120), one_pole(3000)

    def sample(i, t):
        x = noise[i]
        state["lo1"] += a_lo * (x - state["lo1"])
        state["lo2"] += a_lo * (state["lo1"] - state["lo2"])
        state["sub"] += a_sub * (state["lo2"] - state["sub"])
        state["hp"] += a_hp * (x - state["hp"])
        breath = 0.6 + 0.4 * flutter(t)
        roar = state["lo2"] - state["sub"]  # a band around 120-600 Hz
        return roar * 2.0 * breath + (x - state["hp"]) * 0.05 * breath

    out = render_loop(length, sample)

    # Crackles: short bright noise bursts, wrapped around the loop end.
    for _ in range(int(length * 15)):
        start = rng.randrange(n)
        pop = rng.random() < 0.12
        amp = rng.uniform(0.15, 1.0) ** 2 * (0.5 if pop else 0.3)
        decay = rng.uniform(0.0015, 0.006)
        svf = Svf()
        center = rng.uniform(1200, 4500)
        tone = rng.uniform(180, 380)
        for j in range(int(decay * 8 * RATE)):
            tj = j / RATE
            env = math.exp(-tj / decay) * min(1.0, j / 8)
            _, band, _ = svf.step(rng.uniform(-1, 1), center, 1.8)
            v = band * env
            if pop:  # a woody knock under the snap
                v += math.sin(TAU * tone * tj) * math.exp(-tj / (decay * 3)) * 0.8
            out[(start + j) % n] += v * amp
    return normalize(out, 0.7)


def hum():
    """The heart crystal: a quiet drone of detuned partials that beat slowly."""
    length = 6.0
    rng = random.Random(3)
    # Frequencies are multiples of 1/length Hz, so every partial loops cleanly.
    partials = [(110.0, 1.0), (110.5, 0.5), (165.0, 0.35), (220.0 + 1 / 3, 0.3), (330.0, 0.12), (440.5, 0.08)]
    phases = [rng.uniform(0, TAU) for _ in partials]
    swell = periodic(rng, length, [1, 2])

    def sample(_i, t):
        v = sum(a * math.sin(TAU * f * t + ph) for (f, a), ph in zip(partials, phases))
        return v * (0.7 + 0.3 * swell(t))

    n = int(length * RATE)
    return normalize([sample(i, i / RATE) for i in range(n)], 0.5)


def step(seed, weight):
    """A footstep on packed dirt and grass: a dull thump with a crunchy scuff."""
    rng = random.Random(seed)
    n = int(0.32 * RATE)
    out = [0.0] * n
    # Thump: a falling low tone, quickly damped.
    f0, f1 = 170 * weight, 85 * weight
    phase = 0.0
    for i in range(n):
        t = i / RATE
        phase += TAU * (f1 + (f0 - f1) * math.exp(-t / 0.02)) / RATE
        out[i] += math.sin(phase) * math.exp(-t / 0.03) * min(1.0, t / 0.002) * 0.5
    # Crunch: a few grains of band-passed noise as the sole rolls down.
    for _ in range(rng.randint(6, 9)):
        start = int(rng.uniform(0.0, 0.08) * RATE)
        decay = rng.uniform(0.004, 0.016)
        svf = Svf()
        center, amp = rng.uniform(900, 3200), rng.uniform(0.5, 1.2)
        for j in range(min(int(decay * 7 * RATE), n - start)):
            _, band, _ = svf.step(rng.uniform(-1, 1), center, 1.4)
            out[start + j] += band * math.exp(-j / RATE / decay) * amp
    fade = int(0.03 * RATE)
    for i in range(fade):
        out[n - 1 - i] *= i / fade
    return normalize(out, 0.85)


def main():
    os.makedirs(OUT, exist_ok=True)
    write_wav("wind.wav", wind())
    write_wav("fire.wav", fire())
    write_wav("hum.wav", hum())
    write_wav("step_1.wav", step(seed=11, weight=1.0))
    write_wav("step_2.wav", step(seed=12, weight=0.9))


if __name__ == "__main__":
    main()
