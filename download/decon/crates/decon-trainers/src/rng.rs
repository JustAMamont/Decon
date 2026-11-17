//! Детерминированный RNG тренажёров (S3).
//!
//! Требование — ROADMAP S3, критерий 2: один сид → побайтово идентичный
//! прогон. Внешний `rand` не тянем: движку нужны только u64-числа, а
//! собственный PCG-DXSM короче, чем зависимость, и полностью
//! воспроизводим на любых платформах (никаких различий libc/ОС).
//! Алгоритм — O'Neill, PCG-XSH-RR (перманентная ссылка pcg-random.org);
//! качество достаточно для игровых раскладок, не для криптографии.

#![deny(missing_docs)]

/// Генератор PCG-XSH-RR 64/32 (LCG 64-бит, вывод 32 бита).
///
/// Состояние — два u64; `next_u32` — 0 аллокаций, тривиально инлайнится.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pcg32 {
    state: u64,
    inc: u64,
}

impl Pcg32 {
    /// Конструктор по сиду; `seq` — поток (sequence id), позволяет из
    /// одного сида получать независимые подпотоки (раскладка ≠ движение).
    pub const fn new(seed: u64, seq: u64) -> Self {
        // Инициализация по спеке PCG: state=0, затем two step-и.
        let mut rng = Pcg32 {
            state: 0,
            inc: (seq << 1) | 1,
        };
        rng.state = rng.state.wrapping_add(seed);
        rng.step();
        rng.state = rng.state.wrapping_add(rng.inc);
        rng.step();
        rng
    }

    /// Один шаг LCG (const, чтобы работать в new).
    const fn step(&mut self) {
        // Мультипликатор PCG: 6364136223846793005
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(self.inc);
    }

    /// Следующее 32-битное число.
    pub fn next_u32(&mut self) -> u32 {
        self.step();
        // XSH-RR: xorshift-high + random-rotate
        let x = self.state;
        let xorshifted = (((x >> 18) ^ x) >> 27) as u32;
        let rot = (x >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Равномерное число в `0..n` (n > 0), без модульного смещения
    /// (rejection sampling по старшим битам; Лемир не требуется —
    /// интервалы малы).
    pub fn below(&mut self, n: u32) -> u32 {
        debug_assert!(n > 0);
        if n <= 1 {
            return 0;
        }
        // Порог: наибольшее кратное n, помещающееся в u32.
        let threshold = n.wrapping_neg() % n;
        loop {
            let r = self.next_u32();
            if r >= threshold {
                return r % n;
            }
        }
    }

    /// Равномерный f32 в `0.0..1.0` (24 бита мантиссы).
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }

    /// Тасование Фишера—Йетса слайса — 0 аллокаций.
    pub fn shuffle<T>(&mut self, slice: &mut [T]) {
        let len = slice.len();
        if len < 2 {
            return;
        }
        for i in (1..len).rev() {
            let j = self.below((i + 1) as u32) as usize;
            slice.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Pcg32::new(42, 0);
        let mut b = Pcg32::new(42, 0);
        for _ in 0..100 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn different_seed_different_sequence() {
        let mut a = Pcg32::new(42, 0);
        let mut b = Pcg32::new(43, 0);
        let mut diff = 0;
        for _ in 0..32 {
            if a.next_u32() != b.next_u32() {
                diff += 1;
            }
        }
        assert!(diff > 24, "sequences unexpectedly close: {diff}/32");
    }

    #[test]
    fn below_respects_range() {
        let mut rng = Pcg32::new(7, 1);
        for _ in 0..1000 {
            let v = rng.below(10);
            assert!(v < 10);
        }
    }

    #[test]
    fn next_f32_in_unit() {
        let mut rng = Pcg32::new(9, 2);
        for _ in 0..1000 {
            let v = rng.next_f32();
            assert!((0.0..1.0).contains(&v));
        }
    }

    #[test]
    fn shuffle_is_permutation_and_deterministic() {
        let mut a = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut b = [1u8, 2, 3, 4, 5, 6, 7, 8];
        let mut rng = Pcg32::new(1234, 3);
        rng.shuffle(&mut a);
        let mut rng2 = Pcg32::new(1234, 3);
        rng2.shuffle(&mut b);
        assert_eq!(a, b);
        a.sort_unstable();
        assert_eq!(a, [1, 2, 3, 4, 5, 6, 7, 8]);
    }
}
