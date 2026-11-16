//! JSON, as much of it as the framework's `JSONMethodCodec` channels
//! need: `flutter/textinput` and `flutter/platform`.
//!
//! What is parsed is the framework's message, which an app can make
//! anything of (brief M4 §15): the parser is bounded in depth, never
//! panics, and anything that is not JSON is `None`.

/// A JSON value. An object keeps its members in order; a number with no
/// fraction or exponent that an `i64` holds is an [`Json::Int`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

/// Arrays and objects nested deeper than this are not parsed: no message
/// here nests past four.
const MAX_DEPTH: usize = 32;

impl Json {
    /// An object's member.
    pub(crate) fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Int(n) => Some(*n),
            _ => None,
        }
    }

    /// A number, whole or not.
    pub(crate) fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Int(n) => Some(*n as f64),
            Json::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub(crate) fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub(crate) fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(a) => Some(a),
            _ => None,
        }
    }
}

/// Parses one JSON value, with nothing after it but white space.
pub(crate) fn parse(bytes: &[u8]) -> Option<Json> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut p = Parser {
        rest: text,
        depth: 0,
    };
    let value = p.value()?;
    p.space();
    p.rest.is_empty().then_some(value)
}

struct Parser<'a> {
    rest: &'a str,
    depth: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        self.rest = self.rest.trim_start_matches([' ', '\t', '\n', '\r']);
    }

    fn eat(&mut self, token: &str) -> bool {
        match self.rest.strip_prefix(token) {
            Some(rest) => {
                self.rest = rest;
                true
            }
            None => false,
        }
    }

    fn value(&mut self) -> Option<Json> {
        self.space();
        match self.rest.as_bytes().first()? {
            b'n' => self.eat("null").then_some(Json::Null),
            b't' => self.eat("true").then_some(Json::Bool(true)),
            b'f' => self.eat("false").then_some(Json::Bool(false)),
            b'"' => self.string().map(Json::Str),
            b'[' => self.nested(Parser::array),
            b'{' => self.nested(Parser::object),
            _ => self.number(),
        }
    }

    fn nested(&mut self, parse: fn(&mut Self) -> Option<Json>) -> Option<Json> {
        if self.depth >= MAX_DEPTH {
            return None;
        }
        self.depth += 1;
        let value = parse(self);
        self.depth -= 1;
        value
    }

    fn array(&mut self) -> Option<Json> {
        self.eat("[");
        let mut items = Vec::new();
        self.space();
        if self.eat("]") {
            return Some(Json::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.space();
            if self.eat("]") {
                return Some(Json::Array(items));
            }
            if !self.eat(",") {
                return None;
            }
        }
    }

    fn object(&mut self) -> Option<Json> {
        self.eat("{");
        let mut members = Vec::new();
        self.space();
        if self.eat("}") {
            return Some(Json::Object(members));
        }
        loop {
            self.space();
            let key = self.string()?;
            self.space();
            if !self.eat(":") {
                return None;
            }
            members.push((key, self.value()?));
            self.space();
            if self.eat("}") {
                return Some(Json::Object(members));
            }
            if !self.eat(",") {
                return None;
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        if !self.eat("\"") {
            return None;
        }
        let mut out = String::new();
        loop {
            let stop = self.rest.find(['"', '\\'])?;
            let (plain, rest) = self.rest.split_at(stop);
            // A control character is not allowed bare.
            if plain.bytes().any(|b| b < 0x20) {
                return None;
            }
            out.push_str(plain);
            self.rest = rest;
            if self.eat("\"") {
                return Some(out);
            }
            self.eat("\\");
            let mut chars = self.rest.chars();
            let escape = chars.next()?;
            self.rest = chars.as_str();
            match escape {
                '"' | '\\' | '/' => out.push(escape),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'u' => out.push(self.unicode_escape()?),
                _ => return None,
            }
        }
    }

    /// Four hex digits after `\u`.
    fn hex4(&mut self) -> Option<u32> {
        let digits = self.rest.get(..4)?;
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        self.rest = &self.rest[4..];
        u32::from_str_radix(digits, 16).ok()
    }

    /// A `\u` escape's character: a surrogate takes the escape after it
    /// for its pair, and one with no pair is U+FFFD.
    fn unicode_escape(&mut self) -> Option<char> {
        let unit = self.hex4()?;
        if (0xd800..0xdc00).contains(&unit) {
            let after = self.rest;
            if self.eat("\\u") {
                let low = self.hex4()?;
                if (0xdc00..0xe000).contains(&low) {
                    let c = 0x10000 + ((unit - 0xd800) << 10) + (low - 0xdc00);
                    return char::from_u32(c);
                }
                // Not its pair: read again as its own escape.
                self.rest = after;
            }
            return Some(char::REPLACEMENT_CHARACTER);
        }
        Some(char::from_u32(unit).unwrap_or(char::REPLACEMENT_CHARACTER))
    }

    fn number(&mut self) -> Option<Json> {
        let end = self
            .rest
            .find(|c: char| !matches!(c, '0'..='9' | '-' | '+' | '.' | 'e' | 'E'))
            .unwrap_or(self.rest.len());
        let (word, rest) = self.rest.split_at(end);
        // JSON's grammar, which Rust's parsers are looser than: a minus,
        // digits with no leading zero, a fraction, an exponent.
        let digits = word.strip_prefix('-').unwrap_or(word);
        let whole_len = digits.bytes().take_while(u8::is_ascii_digit).count();
        let (whole, tail) = digits.split_at(whole_len);
        if whole.is_empty() || (whole.len() > 1 && whole.starts_with('0')) {
            return None;
        }
        let value = if tail.is_empty() {
            match word.parse::<i64>() {
                Ok(n) => Json::Int(n),
                Err(_) => Json::Float(word.parse().ok()?),
            }
        } else {
            let (fraction, exponent) = match tail.find(['e', 'E']) {
                Some(i) => (&tail[..i], Some(&tail[i + 1..])),
                None => (tail, None),
            };
            let fraction_ok = fraction.is_empty()
                || (fraction.len() > 1
                    && fraction.starts_with('.')
                    && fraction[1..].bytes().all(|b| b.is_ascii_digit()));
            let exponent_ok = exponent.is_none_or(|e| {
                let e = e.strip_prefix(['-', '+']).unwrap_or(e);
                !e.is_empty() && e.bytes().all(|b| b.is_ascii_digit())
            });
            if !fraction_ok || !exponent_ok {
                return None;
            }
            Json::Float(word.parse().ok()?)
        };
        self.rest = rest;
        Some(value)
    }
}

/// Writes `s` as a JSON string, quotes and all.
pub(crate) fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A `JSONMethodCodec` method call: its method and its arguments, null if
/// it has none.
pub(crate) fn method_call(bytes: &[u8]) -> Option<(String, Json)> {
    let Json::Object(members) = parse(bytes)? else {
        return None;
    };
    let mut method = None;
    let mut args = Json::Null;
    for (key, value) in members {
        match (key.as_str(), value) {
            ("method", Json::Str(name)) => method = Some(name),
            ("args", value) => args = value,
            _ => {}
        }
    }
    Some((method?, args))
}

/// A `JSONMethodCodec` success envelope around `result`, itself JSON.
pub(crate) fn success(result: &str) -> Vec<u8> {
    format!("[{result}]").into_bytes()
}

/// A `JSONMethodCodec` error envelope, with no details.
pub(crate) fn error(code: &str, message: &str) -> Vec<u8> {
    let mut out = String::from("[");
    write_string(&mut out, code);
    out.push(',');
    write_string(&mut out, message);
    out.push_str(",null]");
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_parse() {
        assert_eq!(parse(b" null "), Some(Json::Null));
        assert_eq!(parse(b"true"), Some(Json::Bool(true)));
        assert_eq!(parse(b"-12"), Some(Json::Int(-12)));
        assert_eq!(parse(b"0"), Some(Json::Int(0)));
        assert_eq!(parse(b"1.5e2"), Some(Json::Float(150.0)));
        assert_eq!(parse(b"1E-1"), Some(Json::Float(0.1)));
        // Past an i64: a double, as Dart's decoder makes it.
        assert_eq!(
            parse(b"9223372036854775808"),
            Some(Json::Float(9_223_372_036_854_775_808.0))
        );
        assert_eq!(
            parse(br#"[1, {"a": "b", "c": [ ]}, {}]"#),
            Some(Json::Array(vec![
                Json::Int(1),
                Json::Object(vec![
                    ("a".to_owned(), Json::Str("b".to_owned())),
                    ("c".to_owned(), Json::Array(vec![])),
                ]),
                Json::Object(vec![]),
            ]))
        );
    }

    #[test]
    fn strings_parse_with_their_escapes() {
        let s = |text: &str| parse(text.as_bytes()).and_then(|j| j.as_str().map(str::to_owned));
        assert_eq!(s(r#""a\"b\\c\/d\n\t""#).as_deref(), Some("a\"b\\c/d\n\t"));
        assert_eq!(s(r#""\u00e9\u20ac""#).as_deref(), Some("é€"));
        assert_eq!(s(r#""\ud83d\ude04""#).as_deref(), Some("😄"));
        assert_eq!(s("\"😄\"").as_deref(), Some("😄"));
        // A surrogate with no pair, alone and before another escape.
        assert_eq!(s(r#""\ud83d""#).as_deref(), Some("\u{fffd}"));
        assert_eq!(s(r#""\ud83d\u0041""#).as_deref(), Some("\u{fffd}A"));
        assert_eq!(s(r#""\ude04""#).as_deref(), Some("\u{fffd}"));
    }

    #[test]
    fn what_is_not_json_is_none() {
        for bad in [
            &b""[..],
            b"nul",
            b"[1,]",
            b"[1 2]",
            b"{\"a\"}",
            b"{\"a\":1,}",
            b"{a:1}",
            b"\"abc",
            b"\"a\nb\"",
            b"\"\\x\"",
            b"\"\\u12g4\"",
            b"\"\\u12\"",
            b"01",
            b"-",
            b"1.",
            b".5",
            b"1e",
            b"+1",
            b"1 2",
            b"[",
            b"\xff",
            b"\"\\",
        ] {
            assert_eq!(parse(bad), None, "{:?}", String::from_utf8_lossy(bad));
        }
    }

    /// Depth is bounded: a message of brackets is refused, not recursed
    /// into without end.
    #[test]
    fn nesting_is_bounded() {
        let deep = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
        assert!(parse(deep(MAX_DEPTH).as_bytes()).is_some());
        assert!(parse(deep(MAX_DEPTH + 1).as_bytes()).is_none());
        assert!(parse("[".repeat(1 << 20).as_bytes()).is_none());
        assert!(parse("{\"a\":".repeat(1 << 16).as_bytes()).is_none());
    }

    #[test]
    fn strings_are_written_escaped() {
        let mut out = String::new();
        write_string(&mut out, "a\"b\\c\n\u{1}é😄");
        assert_eq!(out, r#""a\"b\\c\n\u0001é😄""#);
        assert_eq!(
            parse(out.as_bytes()),
            Some(Json::Str("a\"b\\c\n\u{1}é😄".to_owned()))
        );
    }

    #[test]
    fn method_calls_and_envelopes() {
        assert_eq!(
            method_call(br#"{"method":"TextInput.show","args":null}"#),
            Some(("TextInput.show".to_owned(), Json::Null))
        );
        assert_eq!(
            method_call(br#"{"args":[1],"method":"m"}"#),
            Some(("m".to_owned(), Json::Array(vec![Json::Int(1)])))
        );
        assert_eq!(
            method_call(br#"{"method":"m"}"#),
            Some(("m".to_owned(), Json::Null))
        );
        assert_eq!(method_call(br#"{"method":1}"#), None);
        assert_eq!(method_call(br#"["m"]"#), None);
        assert_eq!(success("null"), b"[null]");
        assert_eq!(error("Bad \"x\"", "no"), br#"["Bad \"x\"","no",null]"#);
    }
}
