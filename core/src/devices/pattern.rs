//! Minimal matcher for the Java regex subset Gadgetbridge uses in Garmin device-name
//! patterns (`Pattern.matcher(name).matches()`: the whole name must match).
//!
//! Supported: literal characters, `\d`, escaped literals (`\+`, `\.`, ...), classes
//! `[...]` with ranges and `\d`, groups `( )`, alternation `|` (lowest precedence),
//! postfix `?` `*` `+`, anchors `^` (start) and `$` (end). Anything else makes
//! [`is_match`] return false for that pattern (and `compile` returns `None`).

#[derive(Debug)]
enum Node {
    Char(char),
    Digit,
    Class(Vec<ClassItem>),
    Start,
    End,
    Group(Vec<Vec<Node>>),
    Repeat(Box<Node>, usize, Option<usize>),
}

#[derive(Debug)]
enum ClassItem {
    Range(char, char),
    Digit,
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn alternatives(&mut self, nested: bool) -> Option<Vec<Vec<Node>>> {
        let mut alts = vec![Vec::new()];
        loop {
            match self.peek() {
                None => return if nested { None } else { Some(alts) },
                Some(')') => {
                    return if nested { Some(alts) } else { None };
                }
                Some('|') => {
                    self.pos += 1;
                    alts.push(Vec::new());
                }
                Some(_) => {
                    let atom = self.atom()?;
                    let node = self.quantified(atom);
                    alts.last_mut()?.push(node);
                }
            }
        }
    }

    fn quantified(&mut self, atom: Node) -> Node {
        let (min, max) = match self.peek() {
            Some('?') => (0, Some(1)),
            Some('*') => (0, None),
            Some('+') => (1, None),
            _ => return atom,
        };
        self.pos += 1;
        Node::Repeat(Box::new(atom), min, max)
    }

    fn atom(&mut self) -> Option<Node> {
        let c = self.peek()?;
        self.pos += 1;
        match c {
            '^' => Some(Node::Start),
            '$' => Some(Node::End),
            '(' => {
                let alts = self.alternatives(true)?;
                self.pos += 1; // ')'
                Some(Node::Group(alts))
            }
            '[' => self.class(),
            '\\' => {
                let e = self.peek()?;
                self.pos += 1;
                Some(if e == 'd' { Node::Digit } else { Node::Char(e) })
            }
            '?' | '*' | '+' | '{' | '}' | '.' => None,
            _ => Some(Node::Char(c)),
        }
    }

    fn class(&mut self) -> Option<Node> {
        let mut items = Vec::new();
        loop {
            let c = self.peek()?;
            self.pos += 1;
            let lo = match c {
                ']' => return Some(Node::Class(items)),
                '^' if items.is_empty() => return None, // negated classes unused
                '\\' => {
                    let e = self.peek()?;
                    self.pos += 1;
                    if e == 'd' {
                        items.push(ClassItem::Digit);
                        continue;
                    }
                    e
                }
                other => other,
            };
            if self.peek() == Some('-') && self.chars.get(self.pos + 1).is_some_and(|c| *c != ']') {
                let hi = self.chars[self.pos + 1];
                self.pos += 2;
                items.push(ClassItem::Range(lo, hi));
            } else {
                items.push(ClassItem::Range(lo, lo));
            }
        }
    }
}

fn compile(pattern: &str) -> Option<Vec<Vec<Node>>> {
    let mut p = Parser {
        chars: pattern.chars().collect(),
        pos: 0,
    };
    p.alternatives(false)
}

/// Match `seq[i..]` against `text[pos..]`, then call `k` with the end position.
fn seq(nodes: &[Node], text: &[char], pos: usize, k: &mut dyn FnMut(usize) -> bool) -> bool {
    let Some((first, rest)) = nodes.split_first() else {
        return k(pos);
    };
    one(first, text, pos, &mut |p| seq(rest, text, p, k))
}

fn one(node: &Node, text: &[char], pos: usize, k: &mut dyn FnMut(usize) -> bool) -> bool {
    let cur = text.get(pos).copied();
    match node {
        Node::Start => pos == 0 && k(pos),
        Node::End => pos == text.len() && k(pos),
        Node::Char(c) => cur == Some(*c) && k(pos + 1),
        Node::Digit => cur.is_some_and(|c| c.is_ascii_digit()) && k(pos + 1),
        Node::Class(items) => {
            cur.is_some_and(|c| {
                items.iter().any(|i| match i {
                    ClassItem::Range(lo, hi) => (*lo..=*hi).contains(&c),
                    ClassItem::Digit => c.is_ascii_digit(),
                })
            }) && k(pos + 1)
        }
        Node::Group(alts) => alts.iter().any(|a| seq(a, text, pos, k)),
        Node::Repeat(inner, min, max) => repeat(inner, *min, *max, text, pos, k),
    }
}

fn repeat(
    inner: &Node,
    min: usize,
    max: Option<usize>,
    text: &[char],
    pos: usize,
    k: &mut dyn FnMut(usize) -> bool,
) -> bool {
    // Greedy: try one more repetition first (only if it consumes input, which
    // guarantees termination), then fall back to stopping here.
    if max != Some(0)
        && one(inner, text, pos, &mut |p| {
            p > pos && repeat(inner, min.saturating_sub(1), max.map(|m| m - 1), text, p, k)
        })
    {
        return true;
    }
    min == 0 && k(pos)
}

/// True when `name` matches the whole of `pattern` (Java `Pattern.matches`).
/// A pattern outside the supported subset never matches.
pub fn is_match(pattern: &str, name: &str) -> bool {
    let Some(alts) = compile(pattern) else {
        return false;
    };
    let text: Vec<char> = name.chars().collect();
    alts.iter()
        .any(|a| seq(a, &text, 0, &mut |end| end == text.len()))
}
