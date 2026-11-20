//! The world reads the sign chest protection needs: block ids and the text of signs through the
//! host (the sign's block entity data, `front_text.messages`).

use crate::worldguard_core::chest::Reader;
use mod_api::{ServerHost, Tag};

/// Reads one dimension through the host.
pub struct HostReader<'a> {
    pub host: &'a dyn ServerHost,
    pub dimension: &'a str,
}

fn component_text(tag: &Tag) -> String {
    match tag {
        Tag::String(s) => {
            let s = s.to_string_lossy();
            let t = s.trim();
            if t.starts_with('{') {
                if let Some(i) = t.find("\"text\":\"") {
                    let rest = &t[i + 8..];
                    let mut out = String::new();
                    let mut chars = rest.chars();
                    while let Some(c) = chars.next() {
                        match c {
                            '\\' => {
                                if let Some(n) = chars.next() {
                                    out.push(n);
                                }
                            }
                            '"' => break,
                            _ => out.push(c),
                        }
                    }
                    return out;
                }
                String::new()
            } else if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
                t[1..t.len() - 1].to_owned()
            } else {
                s
            }
        }
        Tag::Compound(_) => match tag.get("text") {
            Some(Tag::String(s)) => s.to_string_lossy(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

pub fn sign_lines(host: &dyn ServerHost, dim: &str, pos: [i32; 3]) -> Option<Vec<String>> {
    let level = host.level(dim)?;
    let entity = host.block_entity(level, pos)?;
    let data = entity.data(host)?;
    let root = Tag::Compound(data);
    let messages = root.get("front_text")?.get("messages")?;
    let Tag::List(list) = messages else { return None };
    let mut lines: Vec<String> = list.iter().map(component_text).collect();
    lines.resize(4, String::new());
    Some(lines)
}

impl Reader for HostReader<'_> {
    fn block(&self, pos: [i32; 3]) -> Option<String> {
        let level = self.host.level(self.dimension)?;
        if !level.is_loaded(self.host, pos) {
            return None;
        }
        level.block(self.host, pos).map(|k| k.id(self.host).to_string())
    }

    fn sign_lines(&self, pos: [i32; 3]) -> Option<Vec<String>> {
        sign_lines(self.host, self.dimension, pos)
    }
}
