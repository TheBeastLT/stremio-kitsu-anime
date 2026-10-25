use crate::{application::Extras, infrastructure::js};

const MAX_PAIRS: usize = 1000;
const SUFFIX: usize = ".json".len();

pub fn resource_extras(path: &str) -> Extras {
    let segment = path.rsplit('/').next().unwrap_or_default();
    let raw = segment.len().checked_sub(SUFFIX).and_then(|end| segment.get(..end)).unwrap_or_default();
    parse_query(raw)
}

pub fn parse_query(query: &str) -> Extras {
    let mut parser = Parser::default();
    if query.is_empty() {
        return parser.extras;
    }
    let bytes = query.as_bytes();
    for (index, &code) in bytes.iter().enumerate() {
        if code == b'&' {
            if !parser.separate(query, index) {
                return parser.extras;
            }
            continue;
        }
        if !parser.after_eq {
            if code == b'=' {
                parser.take_key(query, index);
                parser.after_eq = true;
                parser.encode_check = 0;
                parser.last = index + 1;
                continue;
            }
            if !parser.key_encoded && parser.check_escape(code, true) {
                continue;
            }
            if code == b'+' {
                parser.take_key(query, index);
                parser.key.push(' ');
                parser.last = index + 1;
                continue;
            }
        }
        if code == b'+' {
            parser.take_value(query, index);
            parser.value.push(' ');
            parser.last = index + 1;
        } else if !parser.value_encoded {
            parser.check_escape(code, false);
        }
    }
    if parser.last < query.len() {
        if parser.after_eq {
            parser.value.push_str(&query[parser.last..]);
        } else {
            parser.key.push_str(&query[parser.last..]);
        }
    } else if !parser.after_eq && parser.key.is_empty() {
        return parser.extras;
    }
    parser.add();
    parser.extras
}

#[derive(Default)]
struct Parser {
    extras: Extras,
    pairs: usize,
    last: usize,
    after_eq: bool,
    key: String,
    value: String,
    key_encoded: bool,
    value_encoded: bool,
    encode_check: u8,
}

impl Parser {
    fn take_key(&mut self, query: &str, end: usize) {
        if self.last < end {
            self.key.push_str(&query[self.last..end]);
        }
    }

    fn take_value(&mut self, query: &str, end: usize) {
        if self.last < end {
            self.value.push_str(&query[self.last..end]);
        }
    }

    fn check_escape(&mut self, code: u8, key: bool) -> bool {
        if code == b'%' {
            self.encode_check = 1;
            return key;
        }
        if self.encode_check == 0 {
            return false;
        }
        if !code.is_ascii_hexdigit() {
            self.encode_check = 0;
            return false;
        }
        self.encode_check += 1;
        if self.encode_check == 3 {
            if key {
                self.key_encoded = true;
            } else {
                self.value_encoded = true;
            }
        }
        key
    }

    fn separate(&mut self, query: &str, index: usize) -> bool {
        if self.after_eq {
            self.take_value(query, index);
        } else if self.last < index {
            self.take_key(query, index);
        } else if self.key.is_empty() {
            self.pairs += 1;
            self.last = index + 1;
            return self.pairs < MAX_PAIRS;
        }
        self.add();
        self.pairs += 1;
        self.key_encoded = false;
        self.value_encoded = false;
        self.encode_check = 0;
        self.last = index + 1;
        self.after_eq = false;
        self.pairs < MAX_PAIRS
    }

    fn add(&mut self) {
        let key = std::mem::take(&mut self.key);
        let value = std::mem::take(&mut self.value);
        let key = if self.key_encoded && !key.is_empty() { decode(&key) } else { key };
        let value = if self.value_encoded && !value.is_empty() { decode(&value) } else { value };
        self.extras.add(key, value);
    }
}

fn decode(text: &str) -> String {
    js::decode_uri_component(text).unwrap_or_else(|| js::unescape_buffer(text))
}

#[cfg(test)]
mod tests;
