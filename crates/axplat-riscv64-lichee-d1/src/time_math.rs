/// Scale `value * mul / div` with u128 intermediate precision, rounding down.
/// Saturates at `u64::MAX` on overflow or when `div == 0`.
pub fn mul_div_floor(value: u64, mul: u64, div: u64) -> u64 {
    if div == 0 {
        return u64::MAX;
    }
    let wide = (value as u128) * (mul as u128);
    let result = wide / (div as u128);
    if result > u64::MAX as u128 {
        u64::MAX
    } else {
        result as u64
    }
}

#[cfg(test)]
mod tests {
    use super::mul_div_floor;

    const TIMER_FREQ: u64 = 24_000_000;
    const NANOS: u64 = 1_000_000_000;

    #[test]
    fn test_mul_div_floor_one_second() {
        // 24_000_000 ticks × 1e9 / 24_000_000 == 1_000_000_000 ns
        assert_eq!(mul_div_floor(24_000_000, NANOS, TIMER_FREQ), 1_000_000_000);
    }

    #[test]
    fn test_mul_div_floor_nanos_to_ticks() {
        // 1_000_000_000 ns × 24_000_000 / 1e9 == 24_000_000 ticks
        assert_eq!(mul_div_floor(1_000_000_000, TIMER_FREQ, NANOS), 24_000_000);
    }

    #[test]
    fn test_mul_div_floor_round_trip() {
        // ticks→nanos→ticks round-trips exactly when ticks is a multiple of 3
        // (NANOS/TIMER_FREQ = 1e9/24e6 = 125/3 — exact only when the numerator
        // after multiplying by ticks is divisible by 3).
        for i in 0..334u64 {
            let ticks = i * 3;
            let nanos = mul_div_floor(ticks, NANOS, TIMER_FREQ);
            let back = mul_div_floor(nanos, TIMER_FREQ, NANOS);
            assert_eq!(
                back, ticks,
                "round-trip failed: ticks={} → nanos={} → back={}",
                ticks, nanos, back
            );
        }
    }

    #[test]
    fn test_mul_div_floor_zero() {
        assert_eq!(mul_div_floor(0, NANOS, TIMER_FREQ), 0);
    }

    #[test]
    fn test_mul_div_floor_one_tick() {
        // floor(1 × 1e9 / 24e6) = floor(1000/24) = 41 ns
        assert_eq!(mul_div_floor(1, NANOS, TIMER_FREQ), 41);
    }

    #[test]
    fn test_mul_div_floor_one_ns() {
        // floor(1 × 24e6 / 1e9) = floor(24/1000) = 0 ticks
        assert_eq!(mul_div_floor(1, TIMER_FREQ, NANOS), 0);
    }

    #[test]
    fn test_mul_div_floor_saturation() {
        // u64::MAX × 1e9 / 24e6 ≈ 7.69e20 > u64::MAX → saturates
        assert_eq!(mul_div_floor(u64::MAX, NANOS, TIMER_FREQ), u64::MAX);
    }

    #[test]
    fn test_mul_div_floor_div_by_zero() {
        assert_eq!(mul_div_floor(100, 1, 0), u64::MAX);
    }

    #[test]
    fn test_mul_div_floor_monotonic() {
        // Larger tick values must yield larger-or-equal nanoseconds.
        for ticks in 0..10000u64 {
            let n1 = mul_div_floor(ticks, NANOS, TIMER_FREQ);
            let n2 = mul_div_floor(ticks + 1, NANOS, TIMER_FREQ);
            assert!(
                n2 >= n1,
                "not monotonic at ticks={}: n1={}, n2={}",
                ticks,
                n1,
                n2
            );
        }
    }

    #[test]
    fn test_mul_div_floor_frequency_boundaries() {
        const NANOS: u64 = 1_000_000_000;
        // Exact at nominal frequency
        assert_eq!(mul_div_floor(24_000_000, NANOS, 24_000_000), 1_000_000_000);
        // frequency-1: 23_999_999 — 1s of ticks → slightly more ns
        let ns_lo = mul_div_floor(24_000_000, NANOS, 23_999_999);
        assert!(ns_lo > 1_000_000_000);
        // frequency+1: 24_000_001 — 1s of ticks → slightly fewer ns
        let ns_hi = mul_div_floor(24_000_000, NANOS, 24_000_001);
        assert!(ns_hi < 1_000_000_000);
        assert!(ns_hi > 0);
    }

    #[test]
    fn test_mul_div_floor_general_round_trip() {
        const NANOS: u64 = 1_000_000_000;
        const FREQ: u64 = 24_000_000;
        // Test ticks that are NOT multiples of 3 (the exact-round-trip case).
        // For general ticks, ticks→nanos→ticks error must be ≤ 1 tick.
        for ticks in 1..1000u64 {
            if ticks % 3 == 0 {
                continue;
            } // skip exact-round-trip values
            let nanos = mul_div_floor(ticks, NANOS, FREQ);
            let back = mul_div_floor(nanos, FREQ, NANOS);
            let diff = if back >= ticks {
                back - ticks
            } else {
                ticks - back
            };
            assert!(
                diff <= 1,
                "round-trip error > 1 tick at ticks={}: nanos={} → back={} diff={}",
                ticks,
                nanos,
                back,
                diff
            );
        }
    }

    #[test]
    fn test_mul_div_floor_large_round_trip() {
        const NANOS: u64 = 1_000_000_000;
        const FREQ: u64 = 24_000_000;
        // Test larger arbitrary values
        let test_vals = [
            100_000u64,
            1_000_000,
            12_000_000,
            24_000_001,
            100_000_000,
            500_000_000,
        ];
        for &ticks in &test_vals {
            let nanos = mul_div_floor(ticks, NANOS, FREQ);
            let back = mul_div_floor(nanos, FREQ, NANOS);
            let diff = if back >= ticks {
                back - ticks
            } else {
                ticks - back
            };
            assert!(
                diff <= 1,
                "round-trip error > 1 tick at ticks={}: nanos={} → back={} diff={}",
                ticks,
                nanos,
                back,
                diff
            );
        }
    }
}
