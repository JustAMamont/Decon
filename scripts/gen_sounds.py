#!/usr/bin/env python3
"""Генератор процедурных звуков Decon (S2.5).

Синтезирует два сигнала оповещений без сторонних сэмплов (лицензионная
гигиена проекта — как с FFMQ-текстом): весь звук создаётся математикой
этого скрипта, результат — CC0.

- chime.wav: мягкий перезвон — три затухающие синусоиды (пентатоника,
  C6-A5-E6), быстрый attack, долгий хвост;
- bell.wav: «колокол» — одна основная частота + негармонические партиалы
  (умножители 2.0/2.4/3.9, как у настоящих колоколов), медленный нарастающий
  старт и затухание (SPEC: «не резкий колокол»).

Формат: 16-бит PCM WAV, 44100 Гц, моно. Детерминизм: без RNG.

Запуск: python3 scripts/gen_sounds.py <выходная-папка>
Скрипт остаётся в репозитории как доказательство происхождения звука.
"""

import math
import struct
import sys
import wave

RATE = 44_100


def synth_chime() -> list[float]:
    """Мягкий перезвон: три ноты пентатоники с наложением."""
    # C6=1046.5, A5=880.0, E6=1318.5 — мажорная пентатоника, спокойный
    # интервал; ноты вступают со сдвигом 90 мс
    notes = [(1046.5, 0.00, 0.9), (880.0, 0.09, 1.0), (1318.5, 0.18, 1.4)]
    total_s = 1.6
    n = int(RATE * total_s)
    out = [0.0] * n
    for freq, start_s, decay_s in notes:
        start = int(start_s * RATE)
        length = n - start
        for i in range(length):
            t = i / RATE
            # attack 8 мс, экспоненциальное затухание
            attack = min(1.0, t / 0.008)
            env = attack * math.exp(-t / decay_s)
            out[start + i] += 0.32 * env * math.sin(2 * math.pi * freq * t)
    return out


def synth_bell() -> list[float]:
    """Колокол: основная + негармонические партиалы, мягкая атака."""
    # Умножители частоты настоящих колоколов (партиалы негармонические)
    partials = [
        (1.0, 1.00, 1.8),   # основной тон, самый долгий
        (2.0, 0.55, 1.1),   # октава
        (2.4, 0.32, 0.9),   # малая терция сверху (характер «колокольности»)
        (3.9, 0.18, 0.6),   # негармонический верх
    ]
    base = 660.0  # E5 — не резкий, середина регистра
    total_s = 2.4
    n = int(RATE * total_s)
    out = [0.0] * n
    for mult, amp, decay_s in partials:
        for i in range(n):
            t = i / RATE
            # Медленная атака 30 мс — «не резкий колокол» (SPEC)
            attack = min(1.0, t / 0.030)
            env = attack * math.exp(-t / decay_s)
            out[i] += 0.24 * amp * env * math.sin(2 * math.pi * base * mult * t)
    return out


def write_wav(path: str, samples: list[float]) -> None:
    """Записать 16-бит PCM WAV, нормализация к 0.8 пиковой амплитуды."""
    peak = max(abs(s) for s in samples) or 1.0
    scale = 0.8 / peak
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        frames = bytearray()
        for s in samples:
            v = int(max(-1.0, min(1.0, s * scale)) * 32767)
            frames += struct.pack("<h", v)
        w.writeframes(bytes(frames))


def main() -> None:
    if len(sys.argv) != 2:
        print("usage: gen_sounds.py <output-dir>", file=sys.stderr)
        sys.exit(1)
    out_dir = sys.argv[1]
    write_wav(f"{out_dir}/chime.wav", synth_chime())
    write_wav(f"{out_dir}/bell.wav", synth_bell())
    print(f"OK: chime.wav + bell.wav -> {out_dir}")


if __name__ == "__main__":
    main()
