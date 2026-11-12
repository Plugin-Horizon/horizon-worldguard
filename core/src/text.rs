//! Player-visible text: `&` colour codes as segments, and the placeholders string flags may use.

/// Legacy colour codes as RGB.
const COLOURS: [(char, u32); 16] = [
    ('0', 0x000000),
    ('1', 0x0000AA),
    ('2', 0x00AA00),
    ('3', 0x00AAAA),
    ('4', 0xAA0000),
    ('5', 0xAA00AA),
    ('6', 0xFFAA00),
    ('7', 0xAAAAAA),
    ('8', 0x555555),
    ('9', 0x5555FF),
    ('a', 0x55FF55),
    ('b', 0x55FFFF),
    ('c', 0xFF5555),
    ('d', 0xFF55FF),
    ('e', 0xFFFF55),
    ('f', 0xFFFFFF),
];

/// One run of text in one colour; `bold` from `&l` (other formats are dropped).
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub text: String,
    pub rgb: u32,
    pub bold: bool,
}

/// Splits at `&x` codes (and `§x`). `&r` resets to `default`.
pub fn segments(s: &str, default: u32) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    let (mut rgb, mut bold) = (default, false);
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if (c == '&' || c == '§')
            && let Some(&k) = chars.peek()
        {
            let k = k.to_ascii_lowercase();
            let colour = COLOURS.iter().find(|(ch, _)| *ch == k).map(|(_, v)| *v);
            if colour.is_some() || matches!(k, 'k'..='o' | 'r') {
                chars.next();
                if !cur.is_empty() {
                    out.push(Segment {
                        text: std::mem::take(&mut cur),
                        rgb,
                        bold,
                    });
                }
                match (colour, k) {
                    (Some(v), _) => {
                        rgb = v;
                        bold = false;
                    }
                    (None, 'l') => bold = true,
                    (None, 'r') => {
                        rgb = default;
                        bold = false;
                    }
                    _ => {}
                }
                continue;
            }
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(Segment { text: cur, rgb, bold });
    }
    out
}

/// Without colour codes (console).
pub fn plain(s: &str) -> String {
    segments(s, 0xFFFFFF).into_iter().map(|g| g.text).collect()
}

/// WorldGuard's `CommandUtils.replaceColorMacros`: the backtick shorthands become `&` codes,
/// and every valid `&x` code becomes the section sign form that is stored and sent.
pub fn color_macros(s: &str) -> String {
    const MACROS: [(&str, &str); 22] = [
        ("`r", "&c"),
        ("`R", "&4"),
        ("`y", "&e"),
        ("`Y", "&6"),
        ("`g", "&a"),
        ("`G", "&2"),
        ("`c", "&b"),
        ("`C", "&3"),
        ("`b", "&9"),
        ("`B", "&1"),
        ("`p", "&d"),
        ("`P", "&5"),
        ("`0", "&0"),
        ("`1", "&8"),
        ("`2", "&7"),
        ("`w", "&F"),
        ("`k", "&k"),
        ("`l", "&l"),
        ("`m", "&m"),
        ("`n", "&n"),
        ("`o", "&o"),
        ("`x", "&r"),
    ];
    let mut out = s.to_owned();
    for (from, to) in MACROS {
        out = out.replace(from, to);
    }
    let mut res = String::with_capacity(out.len());
    let mut chars = out.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&'
            && let Some(&k) = chars.peek()
            && (k.is_ascii_digit() || matches!(k.to_ascii_lowercase(), 'a'..='f' | 'k'..='o' | 'r'))
        {
            res.push('\u{a7}');
            res.push(k.to_ascii_lowercase());
            chars.next();
            continue;
        }
        res.push(c);
    }
    res
}

/// The placeholders of string flags (`%name%`, `%world%`, `%online%`, `%id%`, `%health%`) as
/// `StringMatcher.replaceMacros` fills them: `id` is the player's UUID, not a region id.
pub struct Placeholders<'a> {
    pub name: &'a str,
    pub world: &'a str,
    pub online: usize,
    pub id: &'a str,
    pub health: f32,
}

pub fn fill(s: &str, p: &Placeholders<'_>) -> String {
    s.replace("%name%", p.name)
        .replace("%id%", p.id)
        .replace("%online%", &p.online.to_string())
        .replace("%world%", p.world)
        .replace("%health%", &format!("{:?}", p.health as f64))
}

/// `MessagingUtil`: a message with `\n` (the two characters) or real line breaks, as lines.
pub fn lines_of(s: &str) -> Vec<String> {
    s.replace("\\n", "\n").split('\n').map(str::to_owned).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        let s = segments("&c&lHey!&7 no", 0xFFFFFF);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0], Segment { text: "Hey!".into(), rgb: 0xFF5555, bold: true });
        assert_eq!(s[1].rgb, 0xAAAAAA);
        assert!(!s[1].bold);
        assert_eq!(plain("a & b &zc"), "a & b &zc");
        let p = Placeholders { name: "Ann", world: "world", online: 3, id: "u-1", health: 19.5 };
        assert_eq!(fill("Hi %name% in %id% (%online%) %health%", &p), "Hi Ann in u-1 (3) 19.5");
        assert_eq!(color_macros("`rHi &l`x&zok"), "\u{a7}cHi \u{a7}l\u{a7}r&zok");
        assert_eq!(lines_of("a\\nb"), vec!["a".to_string(), "b".to_string()]);
    }
}
