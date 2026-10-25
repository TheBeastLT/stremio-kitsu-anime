fn is_js_space(c: char) -> bool {
    (c.is_whitespace() && c != '\u{85}') || c == '\u{FEFF}'
}

pub fn parse_int(text: &str, radix: Option<u32>) -> Option<f64> {
    let text = text.trim_start_matches(is_js_space);
    let (negative, text) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let (radix, text) = match radix {
        None | Some(16) if text.starts_with("0x") || text.starts_with("0X") => (16, &text[2..]),
        None => (10, text),
        Some(radix) => (radix, text),
    };
    let mut value: Option<f64> = None;
    for c in text.chars() {
        let Some(digit) = c.to_digit(radix) else { break };
        value = Some(value.unwrap_or(0.0) * f64::from(radix) + f64::from(digit));
    }
    value.map(|value| if negative { -value } else { value })
}

pub fn to_number(text: &str) -> f64 {
    let text = text.trim_matches(is_js_space);
    if text.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(digits) = text.strip_prefix(prefix) {
            return if !digits.is_empty() && digits.chars().all(|c| c.is_digit(radix)) {
                parse_int(digits, Some(radix)).unwrap_or(f64::NAN)
            } else {
                f64::NAN
            };
        }
    }
    match text {
        "Infinity" | "+Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        _ if text.chars().all(|c| c.is_ascii_digit() || "+-.eE".contains(c)) => text.parse().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_int_reads_the_leading_integer() {
        assert_eq!(parse_int(" 12abc", Some(10)), Some(12.0));
        assert_eq!(parse_int("-5", None), Some(-5.0));
        assert_eq!(parse_int("0x10", None), Some(16.0));
        assert_eq!(parse_int("0x10", Some(10)), Some(0.0));
        assert_eq!(parse_int("abc", None), None);
        assert_eq!(parse_int("", Some(10)), None);
        assert_eq!(parse_int("-", None), None);
    }

    #[test]
    fn to_number_follows_js_number_conversion() {
        assert!((to_number(" 12 ") - 12.0).abs() < f64::EPSILON);
        assert!(to_number("").abs() < f64::EPSILON);
        assert!((to_number("0x1f") - 31.0).abs() < f64::EPSILON);
        assert!((to_number("1e3") - 1000.0).abs() < f64::EPSILON);
        assert!(to_number("12abc").is_nan());
        assert!(to_number("inf").is_nan());
        assert!(to_number("-0x1").is_nan());
        assert_eq!(to_number("-Infinity"), f64::NEG_INFINITY);
    }
}
