//! Dependency-free floating point helpers.
//!
//! `no_std` has no libm, so the `f64` methods that JavaScript's `Math` relies
//! on (`floor`, `round`, `pow`, `sqrt`, …) are implemented here. They are only
//! used by the expression engine, and they aim for double-precision agreement
//! with JS rather than the last ulp of a full libm.

use core::f64::consts::LN_2;

/// Truncate toward zero.
pub fn trunc(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    if x.abs() >= 9.007_199_254_740_992e15 {
        // |x| >= 2^53: already integral.
        return x;
    }
    let i = x as i64;
    i as f64
}

pub fn fract(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    x - trunc(x)
}

pub fn floor(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let t = trunc(x);
    if x < t {
        t - 1.0
    } else {
        t
    }
}

pub fn ceil(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let t = trunc(x);
    if x > t {
        t + 1.0
    } else {
        t
    }
}

/// JavaScript `Math.round`: half-way cases go toward +Infinity.
pub fn round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let t = trunc(x + 0.5);
    // trunc(x + 0.5) already rounds halves up for positives; for negatives JS
    // gives -2.5 -> -2 and trunc(-2.0) = -2, so this matches.
    t
}

/// Euclidean remainder (result has the sign of `y`, or zero).
pub fn rem_euclid(x: f64, y: f64) -> f64 {
    if y == 0.0 || x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    let r = x % y;
    if r < 0.0 {
        if y < 0.0 {
            r - y
        } else {
            r + y
        }
    } else {
        r
    }
}

fn ldexp(x: f64, k: i32) -> f64 {
    let mut x = x;
    let mut k = k;
    while k > 1023 {
        x *= f64::from_bits(0x7fe0_0000_0000_0000); // 2^1023
        k -= 1023;
    }
    while k < -1022 {
        x *= f64::from_bits(0x0010_0000_0000_0000); // 2^-1022
        k += 1022;
    }
    x * f64::from_bits(((k + 1023) as u64) << 52)
}

/// Natural logarithm.
pub fn ln(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x.is_infinite() {
        return f64::INFINITY;
    }

    let bits = x.to_bits();
    let raw_exp = ((bits >> 52) & 0x7ff) as i32;
    let frac_bits = bits & 0x000f_ffff_ffff_ffff;
    let (m, e) = if raw_exp == 0 {
        // Subnormal: normalize.
        (
            f64::from_bits(frac_bits | (1023u64 << 52)) * f64::from_bits(0x4330_0000_0000_0000),
            -1022 - 52,
        )
    } else {
        (f64::from_bits(frac_bits | (1023u64 << 52)), raw_exp - 1023)
    };

    // ln(m) = 2 * atanh((m-1)/(m+1)) for m in [1, 2)
    let z = (m - 1.0) / (m + 1.0);
    let z2 = z * z;
    let mut term = z;
    let mut sum = 0.0f64;
    let mut k = 1.0f64;
    for _ in 0..40 {
        let add = term / k;
        sum += add;
        if add.abs() < 1e-19 {
            break;
        }
        term *= z2;
        k += 2.0;
    }
    2.0 * sum + (e as f64) * LN_2
}

/// Exponential.
pub fn exp(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x > 710.0 {
        return f64::INFINITY;
    }
    if x < -746.0 {
        return 0.0;
    }
    let k = floor(x / LN_2 + 0.5);
    let r = x - k * LN_2;
    let mut term = 1.0f64;
    let mut sum = 1.0f64;
    let mut n = 1.0f64;
    for _ in 0..40 {
        term *= r / n;
        sum += term;
        if term.abs() < 1e-19 {
            break;
        }
        n += 1.0;
    }
    ldexp(sum, k as i32)
}

/// `x ** y` with JavaScript's edge-case behaviour.
pub fn powf(x: f64, y: f64) -> f64 {
    if y == 0.0 {
        return 1.0;
    }
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    if y == 1.0 {
        return x;
    }
    if x == 0.0 {
        // 0 ** negative -> Infinity, 0 ** positive -> 0 (sign per JS rules)
        return if y < 0.0 { f64::INFINITY } else { 0.0 };
    }
    // Integer exponent: exact and fast.
    if y == trunc(y) && y.abs() <= 1024.0 {
        let n = y as i32;
        let mut base = x;
        let mut acc = 1.0f64;
        let mut e = n.unsigned_abs();
        let neg = n < 0;
        while e > 0 {
            if e & 1 == 1 {
                acc *= base;
            }
            base *= base;
            e >>= 1;
        }
        if neg {
            return 1.0 / acc;
        }
        return acc;
    }
    if x < 0.0 {
        // Negative base with a fractional exponent is NaN in JS.
        return f64::NAN;
    }
    exp(y * ln(x))
}

/// Square root.
pub fn sqrt(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return x;
    }
    if x.is_infinite() {
        return x;
    }
    // Halve the exponent for the initial guess, then refine with Newton steps.
    let bits = x.to_bits();
    let guess = f64::from_bits((bits >> 1) + (1023u64 << 51));
    let mut y = guess;
    for _ in 0..40 {
        let next = 0.5 * (y + x / y);
        if (next - y).abs() <= y.abs() * 1e-16 {
            y = next;
            break;
        }
        y = next;
    }
    y
}
