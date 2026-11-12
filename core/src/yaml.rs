//! A small YAML reader and writer for the subset region files use: block mappings and sequences,
//! flow mappings and sequences (also across lines), plain, single- and double-quoted scalars,
//! folded plain/quoted continuation lines, `|`/`>` block scalars and comments. Horizon's
//! `Cargo.lock` has no YAML crate (GAPS.md), so this is the whole parser.
//!
//! Mappings keep their key order, so a file read and written again keeps its layout.

use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Value>),
    Map(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
    /// A scalar as text, as a loose reader would take it (numbers and bools spelled out).
    pub fn to_text(&self) -> Option<String> {
        match self {
            Value::Str(s) => Some(s.clone()),
            Value::Int(i) => Some(i.to_string()),
            Value::Float(f) => Some(fmt_float(*f)),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) if f.fract() == 0.0 => Some(*f as i64),
            Value::Float(f) => Some(f.floor() as i64),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(l) => Some(l),
            _ => None,
        }
    }
    pub fn as_map(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Map(m) => Some(m),
            _ => None,
        }
    }
    fn is_scalar(&self) -> bool {
        !matches!(self, Value::List(_) | Value::Map(_))
    }
}

#[derive(Debug, PartialEq)]
pub struct Error {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line + 1, self.message)
    }
}

fn err<T>(line: usize, message: impl Into<String>) -> Result<T, Error> {
    Err(Error {
        line,
        message: message.into(),
    })
}

// ---------------------------------------------------------------------------------------------
// Reading

struct Parser {
    /// Lines without their trailing comments; a sequence entry's `- ` may be blanked out to read
    /// a mapping that starts on the same line.
    lines: Vec<String>,
    at: usize,
}

/// Parses a whole document. An empty document is `Null`.
pub fn parse(text: &str) -> Result<Value, Error> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines = text
        .lines()
        .map(|l| strip_comment(l).trim_end().to_owned())
        .filter(|l| l != "---" && l != "...")
        .collect();
    let mut p = Parser { lines, at: 0 };
    p.skip_blank();
    if p.at >= p.lines.len() {
        return Ok(Value::Null);
    }
    let indent = p.indent(p.at);
    let v = p.block(indent)?;
    p.skip_blank();
    if p.at < p.lines.len() {
        return err(p.at, "unexpected content after the document");
    }
    Ok(v)
}

/// Removes a `#` comment that is not inside quotes or a scalar (`a#b` stays).
fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut prev_space = true;
    for (i, c) in line.char_indices() {
        match quote {
            Some('\'') if c == '\'' => quote = None,
            Some('"') if c == '"' && !escaped(line, i) => quote = None,
            Some(_) => {}
            None => {
                if c == '#' && prev_space {
                    return &line[..i];
                }
                if (c == '\'' || c == '"') && starts_value(line, i) {
                    quote = Some(c);
                }
            }
        }
        prev_space = c == ' ' || c == '\t';
    }
    line
}

fn escaped(s: &str, i: usize) -> bool {
    s[..i].chars().rev().take_while(|&c| c == '\\').count() % 2 == 1
}

/// Whether a quote at `i` opens a quoted scalar (after `: `, `- `, `[`, `{`, `,` or at the start).
fn starts_value(line: &str, i: usize) -> bool {
    let before = line[..i].trim_end();
    before.is_empty()
        || before.ends_with(':')
        || before.ends_with('-')
        || before.ends_with('[')
        || before.ends_with('{')
        || before.ends_with(',')
        || before.ends_with('?')
}

impl Parser {
    fn skip_blank(&mut self) {
        while self.at < self.lines.len() && self.lines[self.at].trim().is_empty() {
            self.at += 1;
        }
    }

    fn indent(&self, i: usize) -> usize {
        self.lines[i].len() - self.lines[i].trim_start().len()
    }

    fn is_seq_item(&self, i: usize) -> bool {
        let t = self.lines[i].trim_start();
        t == "-" || t.starts_with("- ")
    }

    /// A block node whose first line is at `at` and indented `indent`.
    fn block(&mut self, indent: usize) -> Result<Value, Error> {
        if self.is_seq_item(self.at) {
            self.sequence(indent)
        } else {
            let t = self.lines[self.at].trim_start().to_owned();
            if find_key_colon(&t).is_some() {
                self.mapping(indent)
            } else {
                // A bare scalar or flow node as the whole block.
                let line = self.at;
                self.at += 1;
                self.inline(&t, line, indent)
            }
        }
    }

    fn sequence(&mut self, indent: usize) -> Result<Value, Error> {
        let mut items = Vec::new();
        loop {
            self.skip_blank();
            if self.at >= self.lines.len()
                || self.indent(self.at) != indent
                || !self.is_seq_item(self.at)
            {
                break;
            }
            let line = self.at;
            let rest = self.lines[line].trim_start()[1..].to_owned();
            let rest_trim = rest.trim_start();
            if rest_trim.is_empty() {
                self.at += 1;
                self.skip_blank();
                if self.at < self.lines.len() && self.indent(self.at) > indent {
                    let inner = self.indent(self.at);
                    items.push(self.block(inner)?);
                } else {
                    items.push(Value::Null);
                }
            } else if find_key_colon(rest_trim).is_some() {
                // `- key: value`: a mapping whose column is that of `key`.
                let col = indent + 1 + (rest.len() - rest_trim.len());
                let mut blanked = " ".repeat(col);
                blanked.push_str(rest_trim);
                self.lines[line] = blanked;
                items.push(self.mapping(col)?);
            } else {
                self.at += 1;
                items.push(self.inline(rest_trim, line, indent)?);
            }
        }
        Ok(Value::List(items))
    }

    fn mapping(&mut self, indent: usize) -> Result<Value, Error> {
        let mut entries: Vec<(String, Value)> = Vec::new();
        loop {
            self.skip_blank();
            if self.at >= self.lines.len() || self.indent(self.at) != indent {
                break;
            }
            if self.is_seq_item(self.at) {
                break;
            }
            let line = self.at;
            let t = self.lines[line].trim_start().to_owned();
            let Some(colon) = find_key_colon(&t) else {
                return err(line, format!("expected `key: value`, found `{t}`"));
            };
            let key = unquote_key(t[..colon].trim(), line)?;
            let rest = t[colon + 1..].trim().to_owned();
            self.at += 1;
            let value = if rest.is_empty() {
                self.skip_blank();
                if self.at < self.lines.len() && self.indent(self.at) > indent {
                    let inner = self.indent(self.at);
                    self.block(inner)?
                } else if self.at < self.lines.len()
                    && self.indent(self.at) == indent
                    && self.is_seq_item(self.at)
                {
                    self.sequence(indent)?
                } else {
                    Value::Null
                }
            } else if rest == "|" || rest == ">" || rest.starts_with("|-") || rest.starts_with(">-")
                || rest.starts_with("|+") || rest.starts_with(">+")
            {
                self.block_scalar(&rest, indent)?
            } else {
                self.inline(&rest, line, indent)?
            };
            if entries.iter().any(|(k, _)| *k == key) {
                // Last one wins, as SnakeYAML's default.
                entries.retain(|(k, _)| *k != key);
            }
            entries.push((key, value));
        }
        Ok(Value::Map(entries))
    }

    /// `|` and `>` scalars: the lines indented more than `indent`.
    fn block_scalar(&mut self, header: &str, indent: usize) -> Result<Value, Error> {
        let folded = header.starts_with('>');
        let keep = header.contains('+');
        let strip = header.contains('-');
        let mut raw: Vec<String> = Vec::new();
        let mut inner: Option<usize> = None;
        while self.at < self.lines.len() {
            let l = &self.lines[self.at];
            if l.trim().is_empty() {
                raw.push(String::new());
                self.at += 1;
                continue;
            }
            let ind = self.indent(self.at);
            if ind <= indent {
                break;
            }
            let base = *inner.get_or_insert(ind);
            raw.push(l[base.min(ind)..].to_owned());
            self.at += 1;
        }
        // Trailing blank lines belong to the chomping, not the content.
        let mut trailing = 0;
        while raw.last().is_some_and(|l| l.is_empty()) {
            raw.pop();
            trailing += 1;
        }
        let mut s = if folded {
            let mut out = String::new();
            for (i, l) in raw.iter().enumerate() {
                if i > 0 {
                    if l.is_empty() || raw[i - 1].is_empty() {
                        out.push('\n');
                    } else {
                        out.push(' ');
                    }
                }
                out.push_str(l);
            }
            out
        } else {
            raw.join("\n")
        };
        if !strip && !raw.is_empty() {
            s.push('\n');
        }
        if keep {
            for _ in 0..trailing {
                s.push('\n');
            }
        }
        Ok(Value::Str(s))
    }

    /// A value that starts on a line after `key:` or `- `: a flow node or scalar, which may go on
    /// over following, deeper indented lines.
    fn inline(&mut self, first: &str, line: usize, indent: usize) -> Result<Value, Error> {
        let mut text = first.to_owned();
        if first.starts_with('{') || first.starts_with('[') {
            while !flow_balanced(&text) {
                if self.at >= self.lines.len() {
                    return err(line, "unterminated flow collection");
                }
                text.push(' ');
                text.push_str(self.lines[self.at].trim());
                self.at += 1;
            }
            let mut f = Flow {
                s: text.as_bytes(),
                text: &text,
                i: 0,
                line,
            };
            let v = f.node()?;
            f.ws();
            if f.i < text.len() {
                return err(line, "unexpected text after a flow collection");
            }
            return Ok(v);
        }
        if first.starts_with('"') || first.starts_with('\'') {
            let q = first.as_bytes()[0] as char;
            let mut joined = first.to_owned();
            while !quoted_closed(&joined, q) {
                if self.at >= self.lines.len() {
                    return err(line, "unterminated quoted scalar");
                }
                let next = self.lines[self.at].trim().to_owned();
                self.at += 1;
                // Folding: a line break is a space, an empty line a newline.
                if next.is_empty() {
                    joined.push('\n');
                } else if joined.ends_with('\n') {
                    joined.push_str(&next);
                } else if q == '"' && joined.ends_with('\\') && !escaped(&joined, joined.len() - 1)
                {
                    joined.pop();
                    joined.push_str(&next);
                } else {
                    joined.push(' ');
                    joined.push_str(&next);
                }
            }
            let mut f = Flow {
                s: joined.as_bytes(),
                text: &joined,
                i: 0,
                line,
            };
            let v = f.quoted()?;
            f.ws();
            if f.i < joined.len() {
                return err(line, "unexpected text after a quoted scalar");
            }
            return Ok(Value::Str(v));
        }
        // Plain scalar, folded over deeper lines that are not keys of their own.
        while self.at < self.lines.len() {
            let l = &self.lines[self.at];
            if l.trim().is_empty() {
                break;
            }
            if self.indent(self.at) <= indent {
                break;
            }
            text.push(' ');
            text.push_str(l.trim());
            self.at += 1;
        }
        Ok(plain_scalar(&text))
    }
}

/// The byte index of the `:` that ends a block mapping key in `t`, if `t` is `key: ...`.
fn find_key_colon(t: &str) -> Option<usize> {
    let b = t.as_bytes();
    if b.is_empty() {
        return None;
    }
    if b[0] == b'"' || b[0] == b'\'' {
        let q = b[0];
        let mut i = 1;
        while i < b.len() {
            if b[i] == q {
                if q == b'\'' && i + 1 < b.len() && b[i + 1] == b'\'' {
                    i += 2;
                    continue;
                }
                if q == b'"' && escaped(t, i) {
                    i += 1;
                    continue;
                }
                let after = t[i + 1..].trim_start();
                if let Some(rest) = after.strip_prefix(':')
                    && (rest.is_empty() || rest.starts_with(' '))
                {
                    return Some(t.len() - after.len());
                }
                return None;
            }
            i += 1;
        }
        return None;
    }
    if b[0] == b'{' || b[0] == b'[' {
        return None;
    }
    let mut i = 0;
    while i < b.len() {
        if b[i] == b':' && (i + 1 == b.len() || b[i + 1] == b' ' || b[i + 1] == b'\t') {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn unquote_key(k: &str, line: usize) -> Result<String, Error> {
    if k.starts_with('"') || k.starts_with('\'') {
        let mut f = Flow {
            s: k.as_bytes(),
            text: k,
            i: 0,
            line,
        };
        return f.quoted();
    }
    Ok(k.to_owned())
}

fn quoted_closed(s: &str, q: char) -> bool {
    let b = s.as_bytes();
    let mut i = 1;
    while i < b.len() {
        if b[i] as char == q {
            if q == '\'' && i + 1 < b.len() && b[i + 1] == b'\'' {
                i += 2;
                continue;
            }
            if q == '"' && escaped(s, i) {
                i += 1;
                continue;
            }
            return true;
        }
        i += 1;
    }
    false
}

fn flow_balanced(s: &str) -> bool {
    let mut depth = 0i32;
    let mut quote: Option<u8> = None;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match quote {
            Some(b'\'') => {
                if c == b'\'' {
                    if i + 1 < b.len() && b[i + 1] == b'\'' {
                        i += 1;
                    } else {
                        quote = None;
                    }
                }
            }
            Some(_) => {
                if c == b'"' && !escaped(s, i) {
                    quote = None;
                }
            }
            None => match c {
                b'{' | b'[' => depth += 1,
                b'}' | b']' => depth -= 1,
                b'"' | b'\'' if flow_quote_start(b, i) => quote = Some(c),
                _ => {}
            },
        }
        i += 1;
    }
    depth <= 0 && quote.is_none()
}

fn flow_quote_start(b: &[u8], i: usize) -> bool {
    let mut j = i;
    while j > 0 {
        j -= 1;
        match b[j] {
            b' ' | b'\t' => continue,
            b'{' | b'[' | b',' | b':' => return true,
            _ => return false,
        }
    }
    true
}

/// A plain scalar's type, as YAML 1.1 core schema readers (SnakeYAML) see it.
pub fn plain_scalar(s: &str) -> Value {
    let t = s.trim();
    match t {
        "" | "~" | "null" | "Null" | "NULL" => return Value::Null,
        "true" | "True" | "TRUE" | "yes" | "Yes" | "YES" | "on" | "On" | "ON" => {
            return Value::Bool(true);
        }
        "false" | "False" | "FALSE" | "no" | "No" | "NO" | "off" | "Off" | "OFF" => {
            return Value::Bool(false);
        }
        _ => {}
    }
    if looks_int(t)
        && let Ok(i) = t.replace('_', "").parse::<i64>()
    {
        return Value::Int(i);
    }
    if looks_float(t)
        && let Ok(f) = t.replace('_', "").parse::<f64>()
    {
        return Value::Float(f);
    }
    match t {
        ".inf" | ".Inf" | "+.inf" => return Value::Float(f64::INFINITY),
        "-.inf" | "-.Inf" => return Value::Float(f64::NEG_INFINITY),
        ".nan" | ".NaN" => return Value::Float(f64::NAN),
        _ => {}
    }
    Value::Str(t.to_owned())
}

fn looks_int(t: &str) -> bool {
    let d = t.strip_prefix(['-', '+']).unwrap_or(t);
    !d.is_empty() && d.bytes().all(|c| c.is_ascii_digit() || c == b'_') && d.as_bytes()[0] != b'_'
}

fn looks_float(t: &str) -> bool {
    let d = t.strip_prefix(['-', '+']).unwrap_or(t);
    let mut digits = false;
    let mut dot = false;
    let mut exp = false;
    let b = d.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'0'..=b'9' => digits = true,
            b'.' if !dot && !exp => dot = true,
            b'e' | b'E' if digits && !exp => {
                exp = true;
                if i + 1 < b.len() && (b[i + 1] == b'-' || b[i + 1] == b'+') {
                    i += 1;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    digits && (dot || exp)
}

struct Flow<'a> {
    s: &'a [u8],
    text: &'a str,
    i: usize,
    line: usize,
}

impl Flow<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && (self.s[self.i] == b' ' || self.s[self.i] == b'\t') {
            self.i += 1;
        }
    }

    fn node(&mut self) -> Result<Value, Error> {
        self.ws();
        match self.s.get(self.i) {
            Some(b'{') => self.map(),
            Some(b'[') => self.list(),
            Some(b'"') | Some(b'\'') => Ok(Value::Str(self.quoted()?)),
            Some(_) => Ok(plain_scalar(&self.plain(false))),
            None => Ok(Value::Null),
        }
    }

    fn map(&mut self) -> Result<Value, Error> {
        self.i += 1;
        let mut entries: Vec<(String, Value)> = Vec::new();
        loop {
            self.ws();
            match self.s.get(self.i) {
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Value::Map(entries));
                }
                Some(b',') => {
                    self.i += 1;
                    continue;
                }
                None => return err(self.line, "unterminated flow mapping"),
                _ => {}
            }
            let key = match self.s[self.i] {
                b'"' | b'\'' => self.quoted()?,
                _ => self.plain(true),
            };
            self.ws();
            let value = if self.s.get(self.i) == Some(&b':') {
                self.i += 1;
                self.ws();
                match self.s.get(self.i) {
                    Some(b',') | Some(b'}') => Value::Null,
                    _ => self.node()?,
                }
            } else {
                Value::Null
            };
            entries.retain(|(k, _)| *k != key);
            entries.push((key.trim().to_owned(), value));
        }
    }

    fn list(&mut self) -> Result<Value, Error> {
        self.i += 1;
        let mut items = Vec::new();
        loop {
            self.ws();
            match self.s.get(self.i) {
                Some(b']') => {
                    self.i += 1;
                    return Ok(Value::List(items));
                }
                Some(b',') => {
                    self.i += 1;
                    continue;
                }
                None => return err(self.line, "unterminated flow sequence"),
                _ => items.push(self.node()?),
            }
        }
    }

    /// A plain scalar inside a flow node; `key` stops at `: ` as well.
    fn plain(&mut self, _key: bool) -> String {
        let start = self.i;
        while self.i < self.s.len() {
            let c = self.s[self.i];
            if c == b',' || c == b'}' || c == b']' {
                break;
            }
            if c == b':' {
                let next = self.s.get(self.i + 1).copied();
                if matches!(next, None | Some(b' ') | Some(b',') | Some(b'}') | Some(b']')) {
                    break;
                }
            }
            self.i += 1;
        }
        self.text[start..self.i].trim().to_owned()
    }

    fn quoted(&mut self) -> Result<String, Error> {
        let q = self.s[self.i];
        self.i += 1;
        let mut out = String::new();
        if q == b'\'' {
            loop {
                let Some(rest) = self.text.get(self.i..) else {
                    return err(self.line, "unterminated quoted scalar");
                };
                let Some(c) = rest.chars().next() else {
                    return err(self.line, "unterminated quoted scalar");
                };
                self.i += c.len_utf8();
                if c == '\'' {
                    if self.s.get(self.i) == Some(&b'\'') {
                        out.push('\'');
                        self.i += 1;
                    } else {
                        return Ok(out);
                    }
                } else {
                    out.push(c);
                }
            }
        }
        loop {
            let Some(c) = self.text.get(self.i..).and_then(|r| r.chars().next()) else {
                return err(self.line, "unterminated quoted scalar");
            };
            self.i += c.len_utf8();
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let Some(e) = self.text.get(self.i..).and_then(|r| r.chars().next()) else {
                        return err(self.line, "bad escape");
                    };
                    self.i += e.len_utf8();
                    match e {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        '0' => out.push('\0'),
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        ' ' => out.push(' '),
                        'e' => out.push('\u{1b}'),
                        'x' => out.push(self.hex(2)?),
                        'u' => out.push(self.hex(4)?),
                        'U' => out.push(self.hex(8)?),
                        _ => return err(self.line, format!("unknown escape \\{e}")),
                    }
                }
                _ => out.push(c),
            }
        }
    }

    fn hex(&mut self, n: usize) -> Result<char, Error> {
        let digits = self.text.get(self.i..self.i + n).unwrap_or("");
        self.i += n;
        u32::from_str_radix(digits, 16)
            .ok()
            .and_then(char::from_u32)
            .ok_or(Error {
                line: self.line,
                message: format!("bad unicode escape {digits}"),
            })
    }
}

// ---------------------------------------------------------------------------------------------
// Writing

/// Writes `value` (normally a mapping) in block style with `indent` spaces a level. Collections
/// whose children are all scalars are written in flow style (`{x: 1, y: 2}`, `[a, b]`), as
/// SnakeYAML's automatic style does; empty ones as `{}` and `[]`.
pub fn emit(value: &Value, indent: usize) -> String {
    let mut out = String::new();
    match value {
        Value::Map(entries) if !entries.is_empty() => emit_map(&mut out, entries, 0, indent),
        Value::List(items) if !items.is_empty() => emit_list(&mut out, items, 0, indent),
        other => {
            out.push_str(&flow(other));
            out.push('\n');
        }
    }
    out
}

fn all_scalar(v: &Value) -> bool {
    match v {
        Value::Map(m) => m.iter().all(|(_, v)| v.is_scalar()),
        Value::List(l) => l.iter().all(Value::is_scalar),
        _ => true,
    }
}

fn emit_map(out: &mut String, entries: &[(String, Value)], level: usize, step: usize) {
    let pad = " ".repeat(level);
    for (k, v) in entries {
        let key = scalar_text(k, true);
        match v {
            Value::Map(m) if !m.is_empty() && !all_scalar(v) => {
                let _ = writeln!(out, "{pad}{key}:");
                emit_map(out, m, level + step, step);
            }
            Value::List(l) if !l.is_empty() && !all_scalar(v) => {
                let _ = writeln!(out, "{pad}{key}:");
                emit_list(out, l, level + step, step);
            }
            _ => {
                let _ = writeln!(out, "{pad}{key}: {}", flow(v));
            }
        }
    }
}

fn emit_list(out: &mut String, items: &[Value], level: usize, step: usize) {
    let pad = " ".repeat(level);
    for v in items {
        match v {
            Value::Map(m) if !m.is_empty() && !all_scalar(v) => {
                // `- ` then the first key on the same line, the rest under it.
                let mut inner = String::new();
                emit_map(&mut inner, m, level + 2, step);
                let trimmed = &inner[level + 2..];
                let _ = write!(out, "{pad}- {trimmed}");
            }
            Value::List(l) if !l.is_empty() && !all_scalar(v) => {
                let _ = writeln!(out, "{pad}-");
                emit_list(out, l, level + step, step);
            }
            _ => {
                let _ = writeln!(out, "{pad}- {}", flow(v));
            }
        }
    }
}

/// A value in flow style.
pub fn flow(v: &Value) -> String {
    match v {
        Value::Null => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => fmt_float(*f),
        Value::Str(s) => scalar_text(s, false),
        Value::List(l) => {
            let parts: Vec<String> = l.iter().map(flow).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Map(m) => {
            let parts: Vec<String> = m
                .iter()
                .map(|(k, v)| format!("{}: {}", scalar_text(k, true), flow(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

pub fn fmt_float(f: f64) -> String {
    if f.is_nan() {
        ".NaN".to_owned()
    } else if f.is_infinite() {
        if f > 0.0 { ".inf" } else { "-.inf" }.to_owned()
    } else if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{f:.1}")
    } else {
        format!("{f}")
    }
}

/// A string as a scalar: plain when that reads back as the same string, else single-quoted, or
/// double-quoted when it holds control characters.
fn scalar_text(s: &str, _key: bool) -> String {
    if needs_double(s) {
        let mut out = String::from("\"");
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                '\r' => out.push_str("\\r"),
                c if (c as u32) < 0x20 || c == '\u{7f}' => {
                    let _ = write!(out, "\\x{:02X}", c as u32);
                }
                c => out.push(c),
            }
        }
        out.push('"');
        return out;
    }
    if plain_ok(s) {
        return s.to_owned();
    }
    format!("'{}'", s.replace('\'', "''"))
}

fn needs_double(s: &str) -> bool {
    s.chars().any(|c| (c as u32) < 0x20 || c == '\u{7f}')
}

fn plain_ok(s: &str) -> bool {
    if s.is_empty() || s != s.trim() {
        return false;
    }
    if !matches!(plain_scalar(s), Value::Str(ref t) if t == s) {
        return false;
    }
    let first = s.as_bytes()[0];
    if b"-?:,[]{}#&*!|>'\"%@`".contains(&first) {
        // `-x` and `:x` are fine for YAML, but quote them as SnakeYAML does to stay safe.
        return false;
    }
    !(s.contains(": ")
        || s.contains(" #")
        || s.ends_with(':')
        || s.contains(',')
        || s.contains('[')
        || s.contains(']')
        || s.contains('{')
        || s.contains('}'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "#\n# header\n#\nregions:\n    lobby:\n        min: {x: -201, y: 2, z: -202}\n        max: {x: 122, y: 287, z: 90}\n        members: {}\n        flags: {block-break: deny, pvp: deny, mob-spawning: deny, block-place: deny}\n        owners: {}\n        type: cuboid\n        priority: 10\n    __global__:\n        members: {}\n        flags: {tnt: deny}\n        owners: {}\n        type: global\n        priority: 0\n";

    #[test]
    fn reads_region_file_shape() {
        let v = parse(SAMPLE).unwrap();
        let lobby = v.get("regions").unwrap().get("lobby").unwrap();
        assert_eq!(lobby.get("min").unwrap().get("x"), Some(&Value::Int(-201)));
        assert_eq!(
            lobby.get("flags").unwrap().get("pvp"),
            Some(&Value::Str("deny".into()))
        );
        assert_eq!(lobby.get("members"), Some(&Value::Map(vec![])));
        assert_eq!(lobby.get("priority"), Some(&Value::Int(10)));
    }

    #[test]
    fn round_trips() {
        let v = parse(SAMPLE).unwrap();
        let text = emit(&v, 4);
        assert!(text.contains("        flags: {block-break: deny, pvp: deny, mob-spawning: deny, block-place: deny}\n"));
        assert_eq!(parse(&text).unwrap(), v);
    }

    #[test]
    fn nested_blocks_lists_and_points() {
        let t = "a:\n    members:\n        players: [bob, alice]\n        unique-ids:\n        - 0ea8eca3-dbf6-47cc-9d1a-c64551ca975c\n    points:\n    - {x: 1, z: 2}\n    - {x: -3, z: 4}\n    min-y: 0\n";
        let v = parse(t).unwrap();
        let a = v.get("a").unwrap();
        assert_eq!(
            a.get("members").unwrap().get("unique-ids").unwrap().as_list().unwrap().len(),
            1
        );
        let pts = a.get("points").unwrap().as_list().unwrap();
        assert_eq!(pts[1].get("x"), Some(&Value::Int(-3)));
        assert_eq!(a.get("min-y"), Some(&Value::Int(0)));
        let back = parse(&emit(&v, 4)).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn quoting_and_multiline() {
        let t = "g: '&aHello, it''s me'\nh: \"tab\\there\"\nlong: \"first part\n    second part\"\nplain: one\n    two\nflow: {a: [1,\n    2], b: 'x: y'}\n";
        let v = parse(t).unwrap();
        assert_eq!(v.get("g").unwrap().as_str(), Some("&aHello, it's me"));
        assert_eq!(v.get("h").unwrap().as_str(), Some("tab\there"));
        assert_eq!(v.get("long").unwrap().as_str(), Some("first part second part"));
        assert_eq!(v.get("plain").unwrap().as_str(), Some("one two"));
        assert_eq!(
            v.get("flow").unwrap().get("b").unwrap().as_str(),
            Some("x: y")
        );
        assert_eq!(parse(&emit(&v, 2)).unwrap(), v);
    }

    #[test]
    fn scalars_that_need_quotes() {
        for s in ["true", "10", "1.5", "", " x", "a: b", "#x", "&a", "null", "[x]", "it's"] {
            let v = Value::Map(vec![("k".into(), Value::Str(s.into()))]);
            assert_eq!(parse(&emit(&v, 4)).unwrap(), v, "{s:?}");
        }
    }

    #[test]
    fn comments_and_block_scalar() {
        let t = "a: 1 # c\nb: 'x # y'\nc: |\n  line1\n  line2\nd: x#y\n";
        let v = parse(t).unwrap();
        assert_eq!(v.get("a"), Some(&Value::Int(1)));
        assert_eq!(v.get("b").unwrap().as_str(), Some("x # y"));
        assert_eq!(v.get("c").unwrap().as_str(), Some("line1\nline2\n"));
        assert_eq!(v.get("d").unwrap().as_str(), Some("x#y"));
    }

    #[test]
    fn list_of_block_maps() {
        let t = "l:\n- a: 1\n  b:\n    c: [x]\n- a: 2\n";
        let v = parse(t).unwrap();
        let l = v.get("l").unwrap().as_list().unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].get("b").unwrap().get("c").unwrap().as_list().unwrap().len(), 1);
        assert_eq!(parse(&emit(&v, 2)).unwrap(), v);
    }
}
