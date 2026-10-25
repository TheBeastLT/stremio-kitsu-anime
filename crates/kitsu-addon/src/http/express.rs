use std::{fmt::Write, sync::LazyLock};

use axum::{
    body::Body,
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use fastjs_regex::Regex;

static UNSAFE_URL_CHARS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:[^\x21\x23-\x3B\x3D\x3F-\x5F\x61-\x7A\x7C\x7E]|%(?:[^0-9A-Fa-f]|[0-9A-Fa-f][^0-9A-Fa-f]|$))+", "g")
        .expect("valid regex")
});

pub const JSON_CONTENT_TYPE: &str = "application/json; charset=utf-8";
const STATUS_MESSAGE: &str = "Temporary Redirect";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Text,
    Html,
    Other,
}

pub fn redirect(url: &str, accept: Option<&str>) -> Response {
    let location = encode_url(url);
    let (content_type, body) = match format(accept) {
        Format::Text => ("text/plain; charset=utf-8", format!("{STATUS_MESSAGE}. Redirecting to {location}")),
        Format::Html => {
            ("text/html; charset=utf-8", format!("<p>{STATUS_MESSAGE}. Redirecting to {}</p>", escape_html(&location)))
        }
        Format::Other => (JSON_CONTENT_TYPE, String::new()),
    };
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = StatusCode::TEMPORARY_REDIRECT;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    if let Ok(location) = HeaderValue::from_str(&location) {
        headers.insert(header::LOCATION, location);
    }
    headers.insert(header::VARY, HeaderValue::from_static("Accept"));
    response
}

pub fn encode_url(url: &str) -> String {
    UNSAFE_URL_CHARS.replace_all_with(url, |found| encode_uri(&url[found.range()])).into_owned()
}

fn encode_uri(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b";,/?:@&=+$-_.!~*'()#".contains(&byte) {
            encoded.push(byte as char);
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            c => escaped.push(c),
        }
    }
    escaped
}

struct Range<'a> {
    kind: &'a str,
    subtype: &'a str,
    q: f64,
    order: usize,
}

fn ranges(accept: &str) -> Vec<Range<'_>> {
    accept
        .split(',')
        .enumerate()
        .filter_map(|(order, part)| {
            let mut params = part.split(';');
            let (kind, subtype) = params.next()?.trim().split_once('/')?;
            let q = params
                .filter_map(|param| param.trim().split_once('='))
                .find(|(name, _)| name.trim() == "q")
                .map_or(Some(1.0), |(_, value)| value.trim().parse::<f64>().ok())?;
            Some(Range { kind: kind.trim(), subtype: subtype.trim(), q, order })
        })
        .collect()
}

fn priority(ranges: &[Range<'_>], kind: &str, subtype: &str) -> Option<(f64, u8, usize)> {
    ranges
        .iter()
        .filter_map(|range| {
            let kind_matches = range.kind.eq_ignore_ascii_case(kind) || range.kind == "*";
            let subtype_matches = range.subtype.eq_ignore_ascii_case(subtype) || range.subtype == "*";
            if !kind_matches || !subtype_matches {
                return None;
            }
            let specificity = u8::from(range.kind.eq_ignore_ascii_case(kind)) * 4
                + u8::from(range.subtype.eq_ignore_ascii_case(subtype)) * 2;
            Some((range.q, specificity, range.order))
        })
        .max_by(|a, b| a.1.cmp(&b.1).then(a.0.total_cmp(&b.0)).then(a.2.cmp(&b.2)))
}

fn format(accept: Option<&str>) -> Format {
    let Some(accept) = accept else { return Format::Text };
    let ranges = ranges(accept);
    let candidates =
        [(Format::Text, priority(&ranges, "text", "plain")), (Format::Html, priority(&ranges, "text", "html"))];
    candidates
        .into_iter()
        .filter_map(|(format, priority)| priority.filter(|(q, _, _)| *q > 0.0).map(|priority| (format, priority)))
        .enumerate()
        .max_by(|(index_a, (_, a)), (index_b, (_, b))| {
            a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(b.2.cmp(&a.2)).then(index_b.cmp(index_a))
        })
        .map_or(Format::Other, |(_, (format, _))| format)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirects_without_accept_answer_in_text() {
        assert_eq!(format(None), Format::Text);
    }

    #[test]
    fn any_type_answers_in_text() {
        assert_eq!(format(Some("*/*")), Format::Text);
    }

    #[test]
    fn html_browsers_get_html() {
        assert_eq!(format(Some("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")), Format::Html);
    }

    #[test]
    fn json_clients_get_an_empty_body() {
        assert_eq!(format(Some("application/json")), Format::Other);
    }

    #[test]
    fn a_higher_quality_wins() {
        assert_eq!(format(Some("text/plain;q=0.5, text/html")), Format::Html);
    }

    #[test]
    fn a_zero_quality_is_refused() {
        assert_eq!(format(Some("text/plain;q=0, text/*;q=0.5")), Format::Html);
    }

    #[test]
    fn encode_url_keeps_valid_escapes_and_encodes_spaces() {
        assert_eq!(encode_url("https://x.test/a b%20c%zz/é"), "https://x.test/a%20b%20c%25zz/%C3%A9");
    }

    #[test]
    fn escape_html_escapes_markup() {
        assert_eq!(escape_html("<a href=\"x\">'&'</a>"), "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;");
    }
}
