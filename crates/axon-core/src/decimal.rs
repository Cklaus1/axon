//! R21 — Exact base-10 fixed-point `Decimal` arithmetic.
//!
//! Money cannot live on `f64`: `0.1 + 0.2 != 0.3` in binary floating point.
//! Axon's `Decimal` is an **exact** fixed-point number backed by an `i128`
//! mantissa with a **fixed global scale of 9** (9 fractional digits). Every
//! `Decimal` shares the same scale, so `+ - == < >` are trivial exact i128
//! operations with no scale alignment. `*` rescales (banker's rounding) and
//! `/` requires a rounding mode.
//!
//! Representation: the rational value is `mantissa / 10^SCALE`.
//!   - `SCALE = 9` → 9 fractional digits.
//!   - i128 range ±1.7e38; divided by 1e9 the integer part spans ±1.7e29.
//!
//! Tradeoff (stated in `governance/specs/R21-decimal.md`): a single fixed scale
//! trades flexible per-value precision for *trivially sound* equality/ordering
//! and zero scale-alignment bugs. 9 dp covers every fiat currency (2–4 dp) and
//! crypto (≤8–9 dp) with headroom; the i128 mantissa keeps the integer range
//! enormous. Overflow is a **graceful panic** (checked), never a silent wrap —
//! matching Axon's checked-int discipline (I-9).

/// The fixed number of fractional digits every `Decimal` carries.
pub const SCALE: u32 = 9;

/// `10^SCALE` — the scaling factor between mantissa and rational value.
pub const SCALE_FACTOR: i128 = 1_000_000_000; // 10^9

/// Rounding mode for division and rescaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundMode {
    /// Banker's rounding — round half to even. The money default: unbiased over
    /// many operations (a stream of `.5` cases rounds up half the time, down
    /// half the time), so totals don't drift.
    HalfEven,
    /// Round half away from zero (`2.5 -> 3`, `-2.5 -> -3`). The "schoolbook" mode.
    HalfUp,
    /// Truncate toward zero (drop the remainder).
    Down,
    /// Round toward +∞ (ceiling).
    Up,
}

impl RoundMode {
    /// Parse a rounding-mode name (used by `decimal_div`/`decimal_round`).
    /// Returns `None` for an unknown mode.
    pub fn from_name(s: &str) -> Option<RoundMode> {
        match s {
            "half_even" | "banker" | "banker's" | "half-even" => Some(RoundMode::HalfEven),
            "half_up" | "half-up" => Some(RoundMode::HalfUp),
            "down" | "truncate" | "trunc" => Some(RoundMode::Down),
            "up" | "ceil" | "ceiling" => Some(RoundMode::Up),
            _ => None,
        }
    }
}

/// Parse a decimal literal/string (e.g. `"1.50"`, `"-0.1"`, `"100"`, `"1e3"`)
/// into an exact `i128` mantissa at the fixed `SCALE`.
///
/// Returns `Err(msg)` on malformed input or when the value (after scaling)
/// would not fit in `i128`. Accepts an optional leading sign, an integer part,
/// an optional fractional part, and rejects scientific notation that the parser
/// already disallows in a `Nd` literal — but `decimal_from_str` (a builtin)
/// accepts plain `[-]ddd.ddd` only, which keeps the round-trip exact.
pub fn parse_decimal(s: &str) -> Result<i128, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("empty decimal literal".to_string());
    }
    let (neg, body) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    if body.is_empty() {
        return Err(format!("malformed decimal: {s:?}"));
    }
    let (int_part, frac_part) = match body.split_once('.') {
        Some((i, f)) => (i, f),
        None => (body, ""),
    };
    // Underscores are permitted as digit separators (mirrors int literals).
    let int_digits: String = int_part.chars().filter(|c| *c != '_').collect();
    let frac_digits: String = frac_part.chars().filter(|c| *c != '_').collect();
    if int_digits.is_empty() && frac_digits.is_empty() {
        return Err(format!("malformed decimal: {s:?}"));
    }
    if !int_digits.chars().all(|c| c.is_ascii_digit())
        || !frac_digits.chars().all(|c| c.is_ascii_digit())
    {
        return Err(format!("malformed decimal: {s:?} (non-digit character)"));
    }
    if frac_digits.len() as u32 > SCALE {
        return Err(format!(
            "decimal {s:?} has {} fractional digits; max is {SCALE}",
            frac_digits.len()
        ));
    }
    let int_val: i128 = if int_digits.is_empty() {
        0
    } else {
        int_digits
            .parse::<i128>()
            .map_err(|_| format!("decimal integer part out of range: {s:?}"))?
    };
    // Right-pad the fractional digits to exactly SCALE places.
    let mut frac_padded = frac_digits.clone();
    while (frac_padded.len() as u32) < SCALE {
        frac_padded.push('0');
    }
    let frac_val: i128 = if frac_padded.is_empty() {
        0
    } else {
        frac_padded
            .parse::<i128>()
            .map_err(|_| format!("decimal fractional part out of range: {s:?}"))?
    };
    let mantissa = int_val
        .checked_mul(SCALE_FACTOR)
        .and_then(|hi| hi.checked_add(frac_val))
        .ok_or_else(|| format!("decimal {s:?} out of range for i128 fixed-point"))?;
    Ok(if neg { -mantissa } else { mantissa })
}

/// Render an `i128` mantissa as a canonical decimal string, e.g. `1500000000`
/// → `"1.5"`, `300000000` → `"0.3"`, `100000000000` → `"100"`. Trailing zeros
/// in the fractional part are stripped; a whole number prints with no point.
pub fn format_decimal(mantissa: i128) -> String {
    let neg = mantissa < 0;
    let abs = mantissa.unsigned_abs();
    let int_part = abs / (SCALE_FACTOR as u128);
    let frac_part = abs % (SCALE_FACTOR as u128);
    let mut s = String::new();
    if neg {
        s.push('-');
    }
    s.push_str(&int_part.to_string());
    if frac_part != 0 {
        // Zero-pad to SCALE then strip trailing zeros.
        let frac = format!("{frac_part:0width$}", width = SCALE as usize);
        let trimmed = frac.trim_end_matches('0');
        s.push('.');
        s.push_str(trimmed);
    }
    s
}

/// Exact addition. Overflow → `Err`.
pub fn add(a: i128, b: i128) -> Result<i128, String> {
    a.checked_add(b)
        .ok_or_else(|| "decimal overflow on add".to_string())
}

/// Exact subtraction. Overflow → `Err`.
pub fn sub(a: i128, b: i128) -> Result<i128, String> {
    a.checked_sub(b)
        .ok_or_else(|| "decimal overflow on sub".to_string())
}

/// Multiplication: `(a/10^S) * (b/10^S) = (a*b)/10^(2S)`. We rescale back to
/// scale S by dividing by `10^S`, rounding half-even. The full product is taken
/// in i128; if `a*b` overflows i128 we report overflow rather than wrap.
pub fn mul(a: i128, b: i128) -> Result<i128, String> {
    let prod = a
        .checked_mul(b)
        .ok_or_else(|| "decimal overflow on mul".to_string())?;
    Ok(rescale_div(prod, SCALE_FACTOR, RoundMode::HalfEven))
}

/// Division with an explicit rounding mode. `(a/10^S) / (b/10^S) = a/b`, but we
/// want the result at scale S, so compute `a * 10^S / b` rounded per `mode`.
/// Division by zero → `Err`.
pub fn div(a: i128, b: i128, mode: RoundMode) -> Result<i128, String> {
    if b == 0 {
        return Err("decimal division by zero".to_string());
    }
    let scaled = a
        .checked_mul(SCALE_FACTOR)
        .ok_or_else(|| "decimal overflow on div (numerator scaling)".to_string())?;
    Ok(rescale_div(scaled, b, mode))
}

/// Remainder: exact `a mod b` on the mantissas (same scale in, same scale out).
/// `a % b` where both are at scale S yields a value at scale S directly.
pub fn rem(a: i128, b: i128) -> Result<i128, String> {
    if b == 0 {
        return Err("decimal remainder by zero".to_string());
    }
    Ok(a.wrapping_rem(b))
}

/// Absolute value. Overflow only at i128::MIN, reported as a graceful error.
pub fn abs(a: i128) -> Result<i128, String> {
    a.checked_abs()
        .ok_or_else(|| "decimal overflow on abs".to_string())
}

/// Negation. Overflow only at i128::MIN.
pub fn neg(a: i128) -> Result<i128, String> {
    a.checked_neg()
        .ok_or_else(|| "decimal overflow on neg".to_string())
}

/// Round a Decimal mantissa to `dp` fractional digits (`0 <= dp <= SCALE`),
/// keeping the result at the canonical SCALE. E.g. rounding `1.235` to 2 dp
/// with HalfEven gives `1.24` (stored at scale 9 as `1240000000`).
pub fn round_dp(mantissa: i128, dp: u32, mode: RoundMode) -> Result<i128, String> {
    if dp > SCALE {
        return Err(format!("cannot round to {dp} dp; max is {SCALE}"));
    }
    if dp == SCALE {
        return Ok(mantissa);
    }
    let drop = SCALE - dp;
    let divisor = pow10(drop);
    let quotient = rescale_div(mantissa, divisor, mode); // value at scale `dp`
    // Re-expand back to scale SCALE.
    quotient
        .checked_mul(divisor)
        .ok_or_else(|| "decimal overflow on round".to_string())
}

/// Compute `10^n` as i128 (n small, ≤ SCALE).
fn pow10(n: u32) -> i128 {
    let mut p: i128 = 1;
    for _ in 0..n {
        p *= 10;
    }
    p
}

/// Compute `numer / divisor` rounded to an integer per `mode`. `divisor > 0`.
/// This is the single rounding kernel shared by mul/div/round so all paths
/// agree exactly.
fn rescale_div(numer: i128, divisor: i128, mode: RoundMode) -> i128 {
    debug_assert!(divisor > 0, "rescale_div divisor must be positive");
    let q = numer / divisor; // truncated toward zero
    let r = numer % divisor; // same sign as numer
    if r == 0 {
        return q;
    }
    let r2 = r.abs() * 2;
    let neg = numer < 0;
    let round_away = match mode {
        RoundMode::Down => false,
        RoundMode::Up => true,
        RoundMode::HalfUp => r2 >= divisor,
        RoundMode::HalfEven => {
            if r2 > divisor {
                true
            } else if r2 < divisor {
                false
            } else {
                // Exactly half — round to even quotient.
                q % 2 != 0
            }
        }
    };
    if round_away {
        if neg {
            q - 1
        } else {
            q + 1
        }
    } else {
        q
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> i128 {
        parse_decimal(s).unwrap()
    }

    #[test]
    fn parse_and_format_round_trip() {
        for s in ["0", "1", "100", "1.5", "0.1", "0.2", "0.3", "-2.75", "123.456789"] {
            assert_eq!(format_decimal(d(s)), s, "round-trip {s}");
        }
    }

    #[test]
    fn exact_point_one_plus_point_two() {
        // The headline: 0.1 + 0.2 == 0.3 EXACTLY.
        assert_eq!(add(d("0.1"), d("0.2")).unwrap(), d("0.3"));
    }

    #[test]
    fn add_sub_exact() {
        assert_eq!(add(d("1.50"), d("2.25")).unwrap(), d("3.75"));
        assert_eq!(sub(d("10.00"), d("0.01")).unwrap(), d("9.99"));
    }

    #[test]
    fn mul_exact() {
        assert_eq!(mul(d("1.5"), d("2")).unwrap(), d("3"));
        assert_eq!(mul(d("0.1"), d("0.1")).unwrap(), d("0.01"));
    }

    #[test]
    fn mul_rounds_half_even() {
        // 1.005 * 1 = 1.005; rounding to 2dp via round_dp half-even = 1.00 (even).
        assert_eq!(round_dp(d("1.005"), 2, RoundMode::HalfEven).unwrap(), d("1.00"));
        // 1.015 -> 1.02 (even)
        assert_eq!(round_dp(d("1.015"), 2, RoundMode::HalfEven).unwrap(), d("1.02"));
    }

    #[test]
    fn div_rounding_modes() {
        // 1 / 3 at scale 9 = 0.333333333 (truncated) — all modes within ulp.
        let third_down = div(d("1"), d("3"), RoundMode::Down).unwrap();
        assert_eq!(format_decimal(third_down), "0.333333333");
        // 2 / 3 half-even rounds the last digit up: 0.666666667
        let two_thirds = div(d("2"), d("3"), RoundMode::HalfEven).unwrap();
        assert_eq!(format_decimal(two_thirds), "0.666666667");
    }

    #[test]
    fn negative_half_even() {
        assert_eq!(round_dp(d("-2.5"), 0, RoundMode::HalfEven).unwrap(), d("-2"));
        assert_eq!(round_dp(d("2.5"), 0, RoundMode::HalfEven).unwrap(), d("2"));
        assert_eq!(round_dp(d("3.5"), 0, RoundMode::HalfEven).unwrap(), d("4"));
    }

    #[test]
    fn overflow_is_error_not_wrap() {
        let huge = i128::MAX;
        assert!(add(huge, d("1")).is_err());
        assert!(mul(huge, d("2")).is_err());
    }

    #[test]
    fn parse_rejects_excess_precision() {
        assert!(parse_decimal("1.0000000001").is_err()); // 10 fractional digits
        assert!(parse_decimal("abc").is_err());
        assert!(parse_decimal("").is_err());
    }

    #[test]
    fn abs_and_neg() {
        assert_eq!(abs(d("-5.5")).unwrap(), d("5.5"));
        assert_eq!(neg(d("5.5")).unwrap(), d("-5.5"));
    }
}
