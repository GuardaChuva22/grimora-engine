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
import sys
import wave

# wind, fire, clang, river, squelch and gun_dry are CC0 recordings now (see CREDITS.md):
# this script no longer writes them.
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



def rain():
    """Steady rain: a bright hiss over a soft wash, and the patter of the nearer drops."""
    length = 6.0
    rng = random.Random(53)
    n = int(length * RATE)
    noise = [rng.uniform(-1, 1) for _ in range(n)]
    drops = [0.0] * n
    ring = int(0.012 * RATE)
    for _ in range(int(length * 70)):
        at = rng.randrange(n)
        strength = rng.uniform(0.2, 1.0) ** 2
        for k in range(ring):
            drops[(at + k) % n] += strength * rng.uniform(-1, 1) * math.exp(-k / (0.002 * RATE))
    swell = periodic(rng, length, [1, 2, 3])
    state = {"low": 0.0, "high": 0.0}
    a_low, a_high = one_pole(900), one_pole(5200)

    def sample(i, t):
        state["low"] += a_low * (noise[i] - state["low"])
        state["high"] += a_high * (noise[i] - state["high"])
        hiss = state["high"] - state["low"]
        return hiss * (0.72 + 0.18 * swell(t)) + state["low"] * 0.35 + drops[i] * 0.3

    return normalize(render_loop(length, sample), 0.6)


def thunder(seed, seconds):
    """One roll of thunder: a crack that breaks into a long, uneven rumble."""
    rng = random.Random(seed)
    n = int(seconds * RATE)
    roll = periodic(rng, seconds, [3, 5, 8, 13, 21])
    state = {"crack": 0.0, "rumble": 0.0, "deep": 0.0}
    a_crack, a_rumble, a_deep = one_pole(1400), one_pole(220), one_pole(35)
    out = []
    for i in range(n):
        t = i / RATE
        x = rng.uniform(-1, 1)
        state["crack"] += a_crack * (x - state["crack"])
        state["rumble"] += a_rumble * (x - state["rumble"])
        state["deep"] += a_deep * (state["rumble"] - state["deep"])
        body = (state["rumble"] - state["deep"]) * (1 - math.exp(-t * 7)) * math.exp(-t * 3.2 / seconds)
        fade = min(1.0, (seconds - t) / 0.5)
        out.append((state["crack"] * 0.5 * math.exp(-t * 11) + body * 2.4 * (0.55 + 0.45 * roll(t))) * fade)
    return normalize(out, 0.85)




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


def swing():
    """A heavy staff cutting the air: noise whose pitch and loudness rise and fall."""
    rng = random.Random(21)
    n = int(0.34 * RATE)
    svf = Svf()
    out = []
    for i in range(n):
        q = i / n
        arc = math.sin(math.pi * q) ** 2
        _, band, _ = svf.step(rng.uniform(-1, 1), 280 + 1100 * arc, 1.1)
        out.append(band * arc)
    return normalize(out, 0.7)


def hit():
    """Wood on something that gives: a falling thump under a short crack."""
    rng = random.Random(22)
    n = int(0.3 * RATE)
    out = [0.0] * n
    phase = 0.0
    for i in range(n):
        t = i / RATE
        phase += TAU * (75 + 150 * math.exp(-t / 0.018)) / RATE
        out[i] += math.sin(phase) * math.exp(-t / 0.06) * min(1.0, t / 0.001)
    svf = Svf()
    for i in range(int(0.05 * RATE)):
        t = i / RATE
        _, band, _ = svf.step(rng.uniform(-1, 1), 1900, 1.2)
        out[i] += band * math.exp(-t / 0.011) * 1.6
    fade = int(0.03 * RATE)
    for i in range(fade):
        out[n - 1 - i] *= i / fade
    return normalize(out, 0.9)




def sea():
    """Waves coming in on a shore: darkened noise that swells and draws back, with a hiss on top."""
    length = 14.0
    rng = random.Random(83)
    noise = [rng.uniform(-1, 1) for _ in range(int(length * RATE))]
    swell = periodic(rng, length, [2, 3, 5], {2: 1.0, 3: 0.8, 5: 0.35})
    state = {"a": 0.0, "b": 0.0, "hiss": 0.0, "sub": 0.0}
    a_sub = one_pole(70)

    def sample(i, t):
        g = max(0.0, 0.5 + 0.8 * swell(t))
        a = one_pole(260 + 1100 * g * g)
        state["a"] += a * (noise[i] - state["a"])
        state["b"] += a * (state["a"] - state["b"])
        state["sub"] += a_sub * (state["b"] - state["sub"])
        state["hiss"] += one_pole(5200) * (noise[i] - state["hiss"])
        return (state["b"] - state["sub"]) * (0.35 + 1.5 * g) + (noise[i] - state["hiss"]) * 0.05 * g * g * g

    return normalize(render_loop(length, sample), 0.7)




def underwater():
    """Being under: a pressure of deep noise that breathes, and a far bubble now and then."""
    length = 12.0
    rng = random.Random(109)
    n = int(length * RATE)
    noise = [rng.uniform(-1, 1) for _ in range(n)]
    breath = periodic(rng, length, [1, 2, 3])
    state = {"a": 0.0, "b": 0.0, "c": 0.0}
    a = one_pole(150)
    bubbles = [0.0] * n
    for _ in range(26):
        start = rng.randrange(n)
        pitch = rng.uniform(260, 900)
        size = rng.uniform(0.06, 0.2)
        phase = 0.0
        for j in range(int(0.16 * RATE)):
            t = j / RATE
            phase += TAU * pitch * (1 + 3.5 * t) / RATE
            bubbles[(start + j) % n] += math.sin(phase) * math.exp(-t * 26) * size

    def sample(i, t):
        state["a"] += a * (noise[i] - state["a"])
        state["b"] += a * (state["a"] - state["b"])
        state["c"] += a * (state["b"] - state["c"])
        return state["c"] * (0.7 + 0.3 * breath(t)) * 9.0 + bubbles[i]

    return normalize(render_loop(length, sample), 0.75)


def splash(seed, seconds):
    """Something falling in: a burst of bright noise that darkens, then bubbles."""
    rng = random.Random(seed)
    n = int(seconds * RATE)
    out = [0.0] * n
    low = 0.0
    for i in range(n):
        t = i / RATE
        low += one_pole(600 + 7000 * math.exp(-t * 7)) * (rng.uniform(-1, 1) - low)
        out[i] = low * min(1.0, t / 0.008) * (math.exp(-t * 5.5) + 0.2 * math.exp(-t * 1.5))
    for _ in range(14):
        start = int(rng.uniform(0.1, seconds * 0.7) * RATE)
        pitch = rng.uniform(400, 1400)
        phase = 0.0
        for j in range(int(0.1 * RATE)):
            if start + j >= n:
                break
            t = j / RATE
            phase += TAU * pitch * (1 + 4 * t) / RATE
            out[start + j] += math.sin(phase) * math.exp(-t * 40) * 0.12
    return normalize(out, 0.9)


def swim():
    """One stroke: water pushed aside, rising and falling away."""
    rng = random.Random(127)
    n = int(0.6 * RATE)
    out = [0.0] * n
    svf = Svf()
    for i in range(n):
        t = i / RATE
        e = min(1.0, t / 0.14) * math.exp(-max(0.0, t - 0.14) * 9)
        _, band, _ = svf.step(rng.uniform(-1, 1), 600 + 900 * e, 1.4)
        out[i] = band * e
    return normalize(out, 0.6)


def waterfall():
    """The roar of a fall: wide noise with a weight under it, which barely changes."""
    length = 9.0
    rng = random.Random(149)
    noise = [rng.uniform(-1, 1) for _ in range(int(length * RATE))]
    swell = periodic(rng, length, [2, 3, 5, 7])
    state = {"low": 0.0, "mid": 0.0, "sub": 0.0}
    a_low, a_mid, a_sub = one_pole(240), one_pole(2400), one_pole(40)

    def sample(i, t):
        g = 0.85 + 0.15 * swell(t)
        state["low"] += a_low * (noise[i] - state["low"])
        state["mid"] += a_mid * (noise[i] - state["mid"])
        state["sub"] += a_sub * (state["low"] - state["sub"])
        return ((state["low"] - state["sub"]) * 3.0 + state["mid"] * 0.8 + (noise[i] - state["mid"]) * 0.06) * g

    return normalize(render_loop(length, sample), 0.75)


def add_glass(out, rng, grains, start, span, low, high, loudness, decay=40.0):
    """Scatters little rings of ice over `span` seconds from `start`: each a pair of high
    partials that die in a moment."""
    for _ in range(grains):
        begin = int((start + rng.uniform(0, span)) * RATE)
        freq = rng.uniform(low, high)
        gain = loudness * rng.uniform(0.3, 1.0)
        rate = decay * rng.uniform(0.7, 1.6)
        length = min(int(5.0 / rate * RATE), len(out) - begin)
        for k in range(max(0, length)):
            t = k / RATE
            out[begin + k] += gain * math.exp(-rate * t) * (
                math.sin(TAU * freq * t) + 0.5 * math.sin(TAU * freq * 2.76 * t)
            )


def ice_tick():
    """One small ring of ice, for choosing things."""
    rng = random.Random(201)
    out = [0.0] * int(0.22 * RATE)
    add_glass(out, rng, 2, 0.0, 0.012, 3000, 4200, 0.3, decay=32.0)
    return out


def ice_form():
    """Ice growing fast: a rising crackle over a breath of cold air."""
    rng = random.Random(202)
    n = int(0.75 * RATE)
    out = [0.0] * n
    svf = Svf()
    for i in range(n):
        t = i / RATE
        swell = min(t / 0.3, 1.0) * max(0.0, 1.0 - (t - 0.3) / 0.45) if t > 0.3 else t / 0.3
        band = svf.step(rng.uniform(-1, 1), 900 + 3200 * min(t / 0.45, 1.0), 2.0)[1]
        out[i] = band * swell * 0.16
    for step_at in range(14):
        add_glass(out, rng, 2, step_at / 14 * 0.42, 0.02, 1900 + step_at * 240, 2600 + step_at * 300, 0.1 + step_at * 0.008)
    add_glass(out, rng, 3, 0.42, 0.01, 4200, 6400, 0.2, decay=16.0)
    return out


def ice_throw():
    """A spear going: air torn past it, falling in pitch as it leaves, and a shiver of ice."""
    rng = random.Random(203)
    n = int(0.7 * RATE)
    out = [0.0] * n
    svf = Svf()
    for i in range(n):
        t = i / RATE
        envelope = min(t / 0.05, 1.0) * math.exp(-5.0 * t)
        band = svf.step(rng.uniform(-1, 1), 3400 * math.exp(-2.6 * t) + 500, 1.6)[1]
        out[i] = band * envelope * 0.75
    add_glass(out, rng, 16, 0.02, 0.22, 3000, 6000, 0.05)
    return out


def ice_shatter():
    """Ice breaking on something: a hard knock, a crash of glass and pieces falling after."""
    rng = random.Random(204)
    n = int(1.5 * RATE)
    out = [0.0] * n
    low = one_pole(1800)
    state = 0.0
    for i in range(n):
        t = i / RATE
        state += low * (rng.uniform(-1, 1) - state)
        out[i] = state * math.exp(-16.0 * t) * 1.3 + math.sin(TAU * (120 - 50 * t) * t) * math.exp(-22.0 * t) * 0.5
    add_glass(out, rng, 60, 0.0, 0.1, 1800, 6800, 0.11)
    add_glass(out, rng, 30, 0.1, 0.45, 2500, 7000, 0.06)
    add_glass(out, rng, 16, 0.5, 0.7, 3200, 7000, 0.025)
    return out


def ice_stomp():
    """A heel coming down, and the ground freezing away from it."""
    rng = random.Random(205)
    n = int(1.6 * RATE)
    out = [0.0] * n
    low = one_pole(420)
    state = 0.0
    for i in range(n):
        t = i / RATE
        state += low * (rng.uniform(-1, 1) - state)
        out[i] = state * math.exp(-13.0 * t) * 2.2 + math.sin(TAU * (78 - 42 * t) * t) * math.exp(-8.0 * t) * 0.72
    for k in range(26):
        at = 0.03 + k * 0.026
        add_glass(out, rng, 3, at, 0.02, 1100 + k * 40, 3400 + k * 50, 0.09 * (1 - k / 34))
    return out


def ice_blast():
    """Everything freezing at once: a deep blow, a wave of cracking going out, a long shimmer."""
    rng = random.Random(206)
    n = int(2.8 * RATE)
    out = [0.0] * n
    svf = Svf()
    low = one_pole(300)
    state = 0.0
    for i in range(n):
        t = i / RATE
        state += low * (rng.uniform(-1, 1) - state)
        thump = math.sin(TAU * (64 - 30 * min(t, 1.0)) * t) * math.exp(-4.2 * t) * 0.8
        band = svf.step(rng.uniform(-1, 1), 5200 * math.exp(-1.2 * t) + 700, 1.3)[1]
        out[i] = thump + state * math.exp(-7.0 * t) * 1.9 + band * math.exp(-3.0 * t) * min(t / 0.03, 1.0) * 0.32
    add_glass(out, rng, 70, 0.0, 0.25, 1400, 6200, 0.085)
    add_glass(out, rng, 70, 0.2, 1.0, 2400, 7200, 0.04)
    add_glass(out, rng, 40, 1.0, 1.5, 3600, 7400, 0.016, decay=22.0)
    return out


def main():
    os.makedirs(OUT, exist_ok=True)
    write_wav("rain.wav", rain())
    write_wav("thunder_1.wav", thunder(seed=71, seconds=5.0))
    write_wav("thunder_2.wav", thunder(seed=72, seconds=7.0))
    write_wav("hum.wav", hum())
    write_wav("step_1.wav", step(seed=11, weight=1.0))
    write_wav("step_2.wav", step(seed=12, weight=0.9))
    write_wav("swing.wav", swing())
    write_wav("hit.wav", hit())
    write_wav("sea.wav", sea())
    write_wav("underwater.wav", underwater())
    write_wav("splash_1.wav", splash(seed=131, seconds=1.4))
    write_wav("splash_2.wav", splash(seed=137, seconds=1.1))
    write_wav("swim.wav", swim())
    write_wav("waterfall.wav", waterfall())
    write_ice()
    write_animals()
    write_ambience()


def gull_cry(rng, start, length, top, bottom, loud, out):
    """One "kee-ow": up fast, then a long fall, rough from a buzz in the throat."""
    phase = 0.0
    svf_a, svf_b = Svf(), Svf()
    begin = int(start * RATE)
    count = int(length * RATE)
    for i in range(count):
        t = i / count
        pitch = top * (t / 0.12) ** 0.5 if t < 0.12 else bottom + (top - bottom) * (1 - (t - 0.12) / 0.88) ** 1.6
        pitch *= 1 + 0.03 * math.sin(TAU * 34 * i / RATE)
        phase += pitch / RATE
        buzz = 1 + 0.5 * math.sin(TAU * 71 * i / RATE)
        wave = (2 * (phase % 1) - 1) * buzz + rng.uniform(-0.12, 0.12)
        _, a, _ = svf_a.step(wave, 1900 + 500 * (1 - t), 3.0)
        _, b, _ = svf_b.step(wave, 3400, 4.0)
        envelope = min(t / 0.04, 1) * (1 - t) ** 0.7
        out[begin + i] += (a + 0.45 * b) * envelope * loud


def gull(seed, cries):
    rng = random.Random(seed)
    last = max(start + length for start, length, *_ in cries)
    out = [0.0] * (int((last + 0.05) * RATE))
    for start, length, top, bottom, loud in cries:
        gull_cry(rng, start, length, top, bottom, loud, out)
    return normalize(out, 0.8)


def clicks(seed, times, cutoff, length=0.012, q=5.0):
    """Hard little things touching: each a burst of noise through a narrow filter."""
    rng = random.Random(seed)
    out = [0.0] * int((times[-1] + 0.08) * RATE)
    svf = Svf()
    for start in times:
        begin = int(start * RATE)
        for i in range(int(length * RATE)):
            out[begin + i] += rng.uniform(-1, 1) * (1 - i / (length * RATE)) ** 2
    return normalize([svf.step(x, cutoff, q)[1] for x in out], 0.7)


def sand_dig():
    rng = random.Random(211)
    count = int(0.45 * RATE)
    svf = Svf()
    out = []
    for i in range(count):
        t = i / count
        grain = rng.uniform(-1, 1) * (rng.random() < 0.35)
        scoop = abs(math.sin(TAU * 2.2 * t)) ** 2
        _, band, _ = svf.step(grain, 1300 + 900 * (1 - t), 0.9)
        out.append(band * scoop * (1 - t) ** 0.6)
    return normalize(out, 0.6)


def jelly_sting():
    """A burn: a thin, fizzing rise that doesn't stop at once."""
    rng = random.Random(223)
    count = int(0.42 * RATE)
    svf = Svf()
    phase = 0.0
    out = []
    for i in range(count):
        t = i / count
        phase += (1400 + 2200 * t) / RATE
        tone = math.sin(TAU * phase) * 0.35
        fizz = rng.uniform(-1, 1) * (rng.random() < 0.5)
        _, band, _ = svf.step(fizz, 3800, 2.0)
        flutter = 0.5 + 0.5 * math.sin(TAU * 48 * i / RATE)
        out.append((band + tone) * flutter * min(t / 0.03, 1) * (1 - t) ** 1.4)
    return normalize(out, 0.7)


def spine_sting():
    """A thump in the foot, and something sharp going in."""
    rng = random.Random(227)
    count = int(0.38 * RATE)
    svf = Svf()
    out = []
    for i in range(count):
        t = i / count
        seconds = i / RATE
        thump = math.sin(TAU * (130 - 70 * t) * seconds) * math.exp(-seconds * 22)
        _, band, _ = svf.step(rng.uniform(-1, 1), 5200 - 3600 * min(t * 3, 1), 3.5)
        stab = band * math.exp(-seconds * 16)
        out.append(thump + stab * 0.9)
    return normalize(out, 0.9)


def crunch():
    """A shell giving way, in pieces."""
    rng = random.Random(229)
    out = [0.0] * int(0.3 * RATE)
    svf = Svf()
    for _ in range(14):
        begin = int(rng.uniform(0.0, 0.17) * RATE)
        loud = rng.uniform(0.3, 1.0)
        for i in range(int(rng.uniform(0.004, 0.018) * RATE)):
            out[begin + i] += rng.uniform(-1, 1) * loud * (1 - i / 400)
    return normalize([svf.step(x, 2100, 1.6)[1] for x in out], 0.75)




def write_animals():
    write_wav("gull_1.wav", gull(241, [(0.0, 0.55, 1750, 980, 1.0)]))
    write_wav("gull_2.wav", gull(251, [(0.0, 0.3, 1900, 1250, 0.9), (0.33, 0.5, 1800, 950, 1.0)]))
    write_wav(
        "gull_3.wav",
        gull(257, [(0.0, 0.16, 1500, 1200, 0.7), (0.19, 0.16, 1550, 1200, 0.8), (0.38, 0.17, 1550, 1150, 0.9), (0.58, 0.45, 1650, 900, 1.0)]),
    )
    write_wav("crab_click.wav", clicks(263, [0.0, 0.07, 0.16, 0.22], 3400))
    write_wav("sand_dig.wav", sand_dig())
    write_wav("pinch.wav", clicks(269, [0.0, 0.035], 2500, length=0.02, q=3.0))
    write_wav("jelly_sting.wav", jelly_sting())
    write_wav("spine_sting.wav", spine_sting())
    write_wav("crunch.wav", crunch())


def gunshot(seed, seconds, body_from, body_to, weight, tail):
    """Black powder going off: a crack, the falling boom under it, and the air ringing with
    the slap of it coming back from the distance. Heavier guns boom lower and ring longer."""
    rng = random.Random(seed)
    n = int(seconds * RATE)
    out = [0.0] * n
    # The crack: a few milliseconds of everything.
    tone = Svf()
    for i in range(int(0.018 * RATE)):
        t = i / RATE
        _, _, high = tone.step(rng.uniform(-1, 1), 1800, 0.8)
        out[i] += high * math.exp(-t / 0.0035) * 1.5
    # The boom: a sine falling fast, and low noise opening and closing around it.
    phase = 0.0
    low = Svf()
    for i in range(n):
        t = i / RATE
        phase += TAU * (body_to + (body_from - body_to) * math.exp(-t / 0.035)) / RATE
        out[i] += math.sin(phase) * math.exp(-t / (0.11 * weight)) * min(1.0, t / 0.0015) * 0.95
        cut = 240 + 2100 * math.exp(-t / (0.05 * weight))
        lows, _, _ = low.step(rng.uniform(-1, 1), cut, 0.9)
        out[i] += lows * math.exp(-t / (0.16 * weight)) * min(1.0, t / 0.001) * 1.1
    # The ringing: noise that keeps getting darker, with a few slaps off things far away.
    air = Svf()
    for i in range(n):
        t = i / RATE
        lows, _, _ = air.step(rng.uniform(-1, 1), 170 + 900 * math.exp(-t / 0.35), 0.8)
        out[i] += lows * math.exp(-t / tail) * 0.26 * min(1.0, t / 0.03)
    for delay, loud, cut in [(0.19, 0.22, 900), (0.37, 0.14, 620), (0.68, 0.09, 420), (1.05, 0.05, 300)]:
        start = int(delay * RATE)
        slap = Svf()
        for j in range(min(int(0.28 * RATE), n - start)):
            lows, _, _ = slap.step(rng.uniform(-1, 1), cut, 0.9)
            out[start + j] += lows * math.exp(-j / RATE / 0.07) * loud * min(1.0, j / (0.006 * RATE))
    fade = int(0.15 * RATE)
    for i in range(fade):
        out[n - 1 - i] *= i / fade
    return normalize(out, 0.95)


def flint():
    """The cock falling: steel on flint, and the priming hissing right after it."""
    rng = random.Random(503)
    n = int(0.2 * RATE)
    out = [0.0] * n
    ring = Svf()
    for i in range(int(0.02 * RATE)):
        _, band, _ = ring.step(rng.uniform(-1, 1) * (1 - i / (0.02 * RATE)) ** 2, 3400, 7.0)
        out[i] += band * 1.6
    hiss = Svf()
    for i in range(int(0.012 * RATE), n):
        t = i / RATE - 0.012
        _, _, high = hiss.step(rng.uniform(-1, 1), 3000, 0.9)
        out[i] += high * math.exp(-t / 0.05) * min(1.0, t / 0.003) * 0.42
    return normalize(out, 0.8)


def cock():
    """The thumb pulling the hammer back: a click up to half cock and a firmer one to full."""
    rng = random.Random(509)
    n = int(0.3 * RATE)
    out = [0.0] * n
    for start, cut, loud in [(0.0, 2300, 0.45), (0.018, 3600, 0.3), (0.15, 2000, 1.0), (0.165, 3100, 0.55)]:
        begin = int(start * RATE)
        ring = Svf()
        for i in range(int(0.04 * RATE)):
            _, band, _ = ring.step(rng.uniform(-1, 1) * (1 - min(1.0, i / (0.006 * RATE))), cut, 8.0)
            out[begin + i] += band * loud
    return normalize(out, 0.75)




def ball_drop():
    """A ball rattling down a barrel: quick ticks getting closer, then the bottom, then still."""
    rng = random.Random(523)
    n = int(0.42 * RATE)
    out = [0.0] * n
    start, gap = 0.0, 0.07
    for bounce in range(6):
        begin = int(start * RATE)
        loud = 0.4 + 0.1 * bounce
        cut = 2900 - 240 * bounce
        ring = Svf()
        for i in range(min(int(0.05 * RATE), n - begin)):
            _, band, _ = ring.step(rng.uniform(-1, 1) * (1 - min(1.0, i / (0.003 * RATE))), cut, 11.0)
            out[begin + i] += band * loud
        start += gap
        gap *= 0.78
    # The thud at the bottom.
    begin = int(start * RATE)
    phase = 0.0
    for i in range(n - begin):
        t = i / RATE
        phase += TAU * 240 / RATE
        out[begin + i] += math.sin(phase) * math.exp(-t / 0.018) * 0.6
    return normalize(out, 0.7)


def ramrod():
    """Iron sliding down a bore, knocking home at the end."""
    rng = random.Random(541)
    n = int(0.3 * RATE)
    out = [0.0] * n
    slide = Svf()
    grit = 0.0
    for i in range(n):
        t = i / RATE
        grit += (rng.uniform(-1, 1) - grit) * 0.05
        rise = min(1.0, t / 0.12) * (1.0 if t < 0.16 else math.exp(-(t - 0.16) / 0.02))
        _, band, _ = slide.step(rng.uniform(-1, 1) * (0.5 + abs(grit) * 3), 1450 + 900 * t / 0.3, 5.0)
        out[i] += band * rise * 0.55
    begin = int(0.17 * RATE)
    ring = Svf()
    phase = 0.0
    for i in range(n - begin):
        t = i / RATE
        _, band, _ = ring.step(rng.uniform(-1, 1) * max(0.0, 1 - t / 0.004), 2500, 12.0)
        phase += TAU * 310 / RATE
        out[begin + i] += band * 0.9 + math.sin(phase) * math.exp(-t / 0.02) * 0.55
    return normalize(out, 0.7)


def bullet_hit():
    """A ball into something solid: a sharp crack and earth or wood giving under it."""
    rng = random.Random(547)
    n = int(0.3 * RATE)
    out = [0.0] * n
    tone = Svf()
    for i in range(int(0.012 * RATE)):
        _, band, _ = tone.step(rng.uniform(-1, 1), 3000, 1.2)
        out[i] += band * (1 - i / (0.012 * RATE)) ** 2 * 1.6
    phase = 0.0
    for i in range(n):
        t = i / RATE
        phase += TAU * (110 + 180 * math.exp(-t / 0.018)) / RATE
        out[i] += math.sin(phase) * math.exp(-t / 0.03) * 0.8
    for _ in range(10):
        begin = int(rng.uniform(0.008, 0.11) * RATE)
        svf = Svf()
        cut, loud, decay = rng.uniform(900, 3500), rng.uniform(0.15, 0.5), rng.uniform(0.004, 0.012)
        for j in range(min(int(decay * 6 * RATE), n - begin)):
            _, band, _ = svf.step(rng.uniform(-1, 1), cut, 1.5)
            out[begin + j] += band * math.exp(-j / RATE / decay) * loud
    return normalize(out, 0.85)


def bullet_ricochet():
    """The same crack, and then the ball going off through the air with a falling whine."""
    rng = random.Random(557)
    n = int(0.62 * RATE)
    out = bullet_hit()[: int(0.15 * RATE)] + [0.0] * n
    out = out[:n]
    phase = 0.0
    begin = int(0.015 * RATE)
    for i in range(n - begin):
        t = i / RATE
        wobble = math.sin(TAU * 34 * t) * 90
        phase += TAU * (2100 * math.exp(-t / 0.55) + 650 + wobble) / RATE
        loud = min(1.0, t / 0.015) * math.exp(-t / 0.13)
        out[begin + i] += (math.sin(phase) + rng.uniform(-0.15, 0.15)) * loud * 0.42
    return normalize(out, 0.8)


def write_guns():
    write_wav("pistol_shot.wav", gunshot(433, 1.5, 300, 62, 1.0, 0.3))
    write_wav("musket_shot.wav", gunshot(439, 2.3, 210, 44, 1.55, 0.5))
    write_wav("gun_flint.wav", flint())
    write_wav("gun_cock.wav", cock())
    write_wav("ball_drop.wav", ball_drop())
    write_wav("ramrod.wav", ramrod())
    write_wav("bullet_hit.wav", bullet_hit())
    write_wav("bullet_ricochet.wav", bullet_ricochet())


def write_ice():
    write_wav("ice_tick.wav", ice_tick())
    write_wav("ice_form.wav", ice_form())
    write_wav("ice_throw.wav", ice_throw())
    write_wav("ice_shatter.wav", ice_shatter())
    write_wav("ice_stomp.wav", ice_stomp())
    write_wav("ice_blast.wav", ice_blast())


# ---------------------------------------------------------------- savanna ambience

def savanna_day():
    """Hot afternoon: cicadas swelling and fading at different distances, grasshoppers ticking
    in the grass, and a dry hiss under it all."""
    length = 12.0
    rng = random.Random(301)
    n = int(length * RATE)
    noise = [rng.uniform(-1, 1) for _ in range(n)]
    # Each cicada: a pitch, a pulse rate, a distance (loudness and how much air dulls it), and
    # a swell that repeats a whole number of times in the loop.
    cicadas = []
    for k, (pitch, pulse, loud, swells) in enumerate([(4300, 210, 1.0, 1), (5200, 180, 0.55, 2), (3800, 240, 0.35, 3)]):
        cicadas.append((pitch, pulse, loud, periodic(rng, length, [swells, swells * 2], {swells: 1.0, swells * 2: 0.4}), Svf(), rng.uniform(0, 1)))
    ticks = [(rng.uniform(0, length), rng.uniform(0.25, 1.0)) for _ in range(46)]
    tick_at = {}
    for start, loud in ticks:
        for j in range(int(0.006 * RATE)):
            i = (int(start * RATE) + j) % n
            tick_at[i] = tick_at.get(i, 0.0) + loud * (1 - j / (0.006 * RATE)) ** 2
    tick_svf = Svf()
    state = {"hiss": 0.0, "low": 0.0}

    def sample(i, t):
        out = 0.0
        for pitch, pulse, loud, swell, svf, phase in cicadas:
            g = max(0.0, swell(t) * 1.4 + 0.25)
            beat = 0.5 + 0.5 * math.sin(TAU * (pulse * t + phase))
            _, band, _ = svf.step(noise[i], pitch, 9.0)
            out += band * beat ** 3 * g * g * loud
        tick = tick_at.get(i, 0.0) * rng.uniform(-1, 1)
        _, tick_band, _ = tick_svf.step(tick, 6500, 4.0)
        state["hiss"] += one_pole(7000) * (noise[i] - state["hiss"])
        state["low"] += one_pole(900) * (state["hiss"] - state["low"])
        return out * 0.9 + tick_band * 0.5 + (state["hiss"] - state["low"]) * 0.05

    return normalize(render_loop(length, sample), 0.6)


def savanna_night():
    """Warm night: crickets in a loose chorus, each chirping at its own steady rate, a deep
    murmur of frogs far off, and the air."""
    length = 12.0
    rng = random.Random(311)
    n = int(length * RATE)
    noise = [rng.uniform(-1, 1) for _ in range(n)]
    # (pitch, chirps in the loop, pulses per chirp, loudness): every rate fits the loop exactly.
    crickets = [(4400, 20, 3, 1.0), (4650, 24, 3, 0.6), (4100, 16, 4, 0.45), (4900, 30, 2, 0.3), (3950, 18, 3, 0.25)]
    offsets = [rng.uniform(0, 1) for _ in crickets]
    frogs = periodic(rng, length, [3, 4, 7], {3: 1.0, 4: 0.7, 7: 0.4})
    state = {"low": 0.0, "lower": 0.0, "air": 0.0}

    def sample(i, t):
        out = 0.0
        for (pitch, chirps, pulses, loud), offset in zip(crickets, offsets):
            # Where in its chirp this cricket is: pulses of 18 ms, 30 ms apart.
            u = (t * chirps / length + offset) % 1.0 * (length / chirps)
            pulse = int(u / 0.03)
            if pulse < pulses:
                v = u - pulse * 0.03
                if v < 0.018:
                    env = math.sin(math.pi * v / 0.018)
                    out += math.sin(TAU * pitch * t) * env * loud
        # Far frogs: a low, throbbing grumble.
        state["low"] += one_pole(380) * (noise[i] - state["low"])
        state["lower"] += one_pole(380) * (state["low"] - state["lower"])
        throb = 0.5 + 0.5 * math.sin(TAU * 11 * t)
        croak = state["lower"] * max(0.0, frogs(t)) * throb * 2.2
        state["air"] += one_pole(2500) * (noise[i] - state["air"])
        return out * 0.35 + croak + state["air"] * 0.03

    return normalize(render_loop(length, sample), 0.55)


def write_ambience():
    write_wav("savanna_day.wav", savanna_day())
    write_wav("savanna_night.wav", savanna_night())


if __name__ == "__main__":
    # `animals`, `guns` or `ambience` writes only those, and leaves the rest as they are.
    if sys.argv[1:] == ["animals"]:
        write_animals()
    elif sys.argv[1:] == ["guns"]:
        write_guns()
    elif sys.argv[1:] == ["ambience"]:
        write_ambience()
    else:
        main()
