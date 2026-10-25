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

fn hex_byte(pair: &[u8]) -> Option<u8> {
    match pair {
        [high, low] if high.is_ascii_hexdigit() && low.is_ascii_hexdigit() => {
            u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()
        }
        _ => None,
    }
}

pub fn decode_uri_component(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            decoded.push(hex_byte(bytes.get(index + 1..index + 3)?)?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

pub fn unescape_buffer(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = (bytes[index] == b'%').then(|| bytes.get(index + 1..index + 3).and_then(hex_byte)).flatten();
        if let Some(byte) = escaped {
            decoded.push(byte);
            index += 3;
        } else {
            decoded.push(if bytes[index] == b'+' { b' ' } else { bytes[index] });
            index += 1;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

const MAX_TIME: f64 = 8.64e15;
const MILLIS_PER_DAY: f64 = 86_400_000.0;

pub fn time_clip(time: f64) -> f64 {
    if !time.is_finite() || time.abs() > MAX_TIME { f64::NAN } else { time.trunc() + 0.0 }
}

pub fn date_parse(text: &str) -> f64 {
    iso_date_time(text.as_bytes()).map_or(f64::NAN, time_clip)
}

fn iso_date_time(bytes: &[u8]) -> Option<f64> {
    let mut cursor = Cursor { bytes, position: 0 };
    let year = match cursor.peek() {
        Some(sign @ (b'+' | b'-')) => {
            cursor.position += 1;
            let year = cursor.digits(6)?;
            if sign == b'-' && year == 0 {
                return None;
            }
            if sign == b'-' { -year } else { year }
        }
        _ => cursor.digits(4)?,
    };
    let (month, day) = if cursor.eat(b'-') {
        let month = cursor.digits(2)?;
        (month, if cursor.eat(b'-') { cursor.digits(2)? } else { 1 })
    } else {
        (1, 1)
    };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let mut time = 0;
    if cursor.eat(b'T') {
        let hours = cursor.digits(2)?;
        cursor.expect(b':')?;
        let minutes = cursor.digits(2)?;
        let seconds = if cursor.eat(b':') { cursor.digits(2)? } else { 0 };
        let mut millis = 0;
        if cursor.eat(b'.') {
            let start = cursor.position;
            while cursor.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                cursor.position += 1;
            }
            let fraction = &bytes[start..cursor.position];
            if fraction.is_empty() {
                return None;
            }
            millis = fraction.iter().chain(b"00").take(3).fold(0, |value, digit| value * 10 + i64::from(digit - b'0'));
        }
        if hours > 24 || minutes > 59 || seconds > 59 || (hours == 24 && (minutes, seconds, millis) != (0, 0, 0)) {
            return None;
        }
        time = ((hours * 60 + minutes) * 60 + seconds) * 1000 + millis;
        match cursor.peek() {
            Some(b'Z' | b'z') => cursor.position += 1,
            Some(sign @ (b'+' | b'-')) => {
                cursor.position += 1;
                let offset_hours = cursor.digits(2)?;
                cursor.expect(b':')?;
                let offset_minutes = cursor.digits(2)?;
                if offset_hours > 23 || offset_minutes > 59 {
                    return None;
                }
                let offset = (offset_hours * 60 + offset_minutes) * 60_000;
                time += if sign == b'+' { -offset } else { offset };
            }
            _ => {}
        }
    }
    if cursor.position != bytes.len() {
        return None;
    }
    Some(super::store::days_from_civil(year, month, day) as f64 * MILLIS_PER_DAY + time as f64)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn eat(&mut self, byte: u8) -> bool {
        let found = self.peek() == Some(byte);
        if found {
            self.position += 1;
        }
        found
    }

    fn expect(&mut self, byte: u8) -> Option<()> {
        self.eat(byte).then_some(())
    }

    fn digits(&mut self, count: usize) -> Option<i64> {
        let digits = self.bytes.get(self.position..self.position + count)?;
        if !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        self.position += count;
        Some(digits.iter().fold(0, |value, digit| value * 10 + i64::from(digit - b'0')))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_uri_component_decodes_utf8_escapes() {
        assert_eq!(decode_uri_component("a%20b%2F%C3%A9").as_deref(), Some("a b/é"));
    }

    #[test]
    fn decode_uri_component_rejects_what_javascript_rejects() {
        for text in ["%", "%2", "%zz", "%+1", "%E0%A4%A", "%FF", "%ED%A0%80", "%C0%AF"] {
            assert_eq!(decode_uri_component(text), None, "{text}");
        }
    }

    #[test]
    fn unescape_buffer_keeps_invalid_escapes_and_replaces_broken_utf8() {
        assert_eq!(unescape_buffer("%E0%A4%A+x%zz"), "\u{FFFD}%A x%zz");
    }

    #[test]
    fn date_parse_reads_the_iso_formats_as_v8() {
        let cases = [
            ("2019-02-30", 1_551_484_800_000.0),
            ("2019-02-31", 1_551_571_200_000.0),
            ("2019", 1_546_300_800_000.0),
            ("2019-01", 1_546_300_800_000.0),
            ("2019-01-08T10:00", 1_546_941_600_000.0),
            ("2019-01-08T10:00:00.5Z", 1_546_941_600_500.0),
            ("2019-01-08T10:00:00.123456Z", 1_546_941_600_123.0),
            ("2019-01-08T24:00:00Z", 1_546_992_000_000.0),
            ("+002019-01-08", 1_546_905_600_000.0),
            ("2019-01-08T10:00:00+01:00", 1_546_938_000_000.0),
            ("2019-01-08T10:00:00.000z", 1_546_941_600_000.0),
            ("2020-01-01T00:00:00.000Z", 1_577_836_800_000.0),
        ];
        for (text, expected) in cases {
            assert!((date_parse(text) - expected).abs() < f64::EPSILON, "{text}");
        }
    }

    #[test]
    fn date_parse_rejects_what_v8_rejects() {
        for text in ["2019-13-01", "2019-00-10", "2019-01-32", "", "-000000-01-01", "2019-01-08T25:00Z", "2019-01-08T"]
        {
            assert!(date_parse(text).is_nan(), "{text}");
        }
    }

    #[test]
    fn time_clip_drops_out_of_range_and_fractional_times() {
        assert!(time_clip(8.64e15 + 1.0).is_nan());
        assert!((time_clip(1.7) - 1.0).abs() < f64::EPSILON);
        assert!(time_clip(f64::NAN).is_nan());
    }

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
