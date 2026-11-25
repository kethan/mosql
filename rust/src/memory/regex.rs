//! A small backtracking regex engine (no dependencies).
//!
//! Covers the subset the memory engine needs for `$regex` and SQL `$like`:
//! literals, `.`, `*`, `+`, `?`, `{m,n}`, `^`, `$`, `|`, groups, character
//! classes, and the escapes `\d \D \w \W \s \S`. Matching is leftmost-first with
//! the usual greedy / lazy quantifier semantics.

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
#[derive(Clone, Debug)]
enum Node {
    Empty,
    Char(char),
    Any,
    Class {
        negated: bool,
        ranges: Vec<(char, char)>,
    },
    Start,
    End,
    Seq(Vec<Node>),
    Alt(Vec<Node>),
    Repeat {
        node: Box<Node>,
        min: u32,
        max: u32,
        greedy: bool,
    },
}

pub struct Regex {
    root: Node,
    case_insensitive: bool,
}

impl Regex {
    pub fn new(pattern: &str, case_insensitive: bool) -> Result<Regex, String> {
        let chars: Vec<char> = pattern.chars().collect();
        let mut p = Parser { c: &chars, i: 0 };
        let root = p.alt()?;
        if p.i != chars.len() {
            return Err(format!("unexpected `{}`", chars[p.i]));
        }
        Ok(Regex {
            root,
            case_insensitive,
        })
    }

    /// `RegExp.test`
    pub fn is_match(&self, text: &str) -> bool {
        let s: Vec<char> = text.chars().collect();
        for start in 0..=s.len() {
            let mut ends = Vec::new();
            self.ends(&self.root, &s, start, &mut ends);
            if !ends.is_empty() {
                return true;
            }
        }
        false
    }

    /// Replace every match with `replacement` (no `$1` capture support).
    pub fn replace_all(&self, text: &str, replacement: &str) -> String {
        let s: Vec<char> = text.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i <= s.len() {
            let mut ends = Vec::new();
            self.ends(&self.root, &s, i, &mut ends);
            if let Some(end) = ends.first() {
                out.push_str(replacement);
                if *end > i {
                    i = *end;
                } else {
                    if i < s.len() {
                        out.push(s[i]);
                    }
                    i += 1;
                }
            } else if i < s.len() {
                out.push(s[i]);
                i += 1;
            } else {
                break;
            }
        }
        out
    }

    fn eq(&self, a: char, b: char) -> bool {
        if a == b {
            return true;
        }
        if self.case_insensitive {
            return a.to_lowercase().eq(b.to_lowercase());
        }
        false
    }

    fn class_has(&self, ranges: &[(char, char)], c: char) -> bool {
        ranges.iter().any(|(a, b)| {
            c >= *a && c <= *b
                || (self.case_insensitive
                    && (c.to_ascii_lowercase() >= *a && c.to_ascii_lowercase() <= *b
                        || c.to_ascii_uppercase() >= *a && c.to_ascii_uppercase() <= *b))
        })
    }

    /// Push every possible end position for `node` starting at `i`, in preference
    /// order (greedy = longest first).
    fn ends(&self, node: &Node, s: &[char], i: usize, out: &mut Vec<usize>) {
        match node {
            Node::Empty => out.push(i),
            Node::Char(c) => {
                if i < s.len() && self.eq(s[i], *c) {
                    out.push(i + 1);
                }
            }
            Node::Any => {
                if i < s.len() && s[i] != '\n' {
                    out.push(i + 1);
                }
            }
            Node::Class { negated, ranges } => {
                if i < s.len() && self.class_has(ranges, s[i]) != *negated {
                    out.push(i + 1);
                }
            }
            Node::Start => {
                if i == 0 {
                    out.push(i);
                }
            }
            Node::End => {
                if i == s.len() {
                    out.push(i);
                }
            }
            Node::Seq(items) => {
                self.seq_ends(items, s, i, out);
            }
            Node::Alt(branches) => {
                for b in branches {
                    self.ends(b, s, i, out);
                }
            }
            Node::Repeat {
                node,
                min,
                max,
                greedy,
            } => {
                self.repeat_ends(node, *min, *max, *greedy, s, i, out);
            }
        }
    }

    fn seq_ends(&self, items: &[Node], s: &[char], i: usize, out: &mut Vec<usize>) {
        if items.is_empty() {
            out.push(i);
            return;
        }
        let mut first = Vec::new();
        self.ends(&items[0], s, i, &mut first);
        for j in first {
            self.seq_ends(&items[1..], s, j, out);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn repeat_ends(
        &self,
        node: &Node,
        min: u32,
        max: u32,
        greedy: bool,
        s: &[char],
        i: usize,
        out: &mut Vec<usize>,
    ) {
        let push_stop = |out: &mut Vec<usize>| {
            if min == 0 {
                out.push(i);
            }
        };

        if !greedy {
            push_stop(out);
        }

        if max > 0 {
            let mut first = Vec::new();
            self.ends(node, s, i, &mut first);
            for j in first {
                if j == i {
                    continue; // empty body: stop to avoid infinite recursion
                }
                let next_min = min.saturating_sub(1);
                let next_max = max - 1;
                if next_min == 0 && next_max == 0 {
                    out.push(j);
                } else {
                    self.repeat_ends(node, next_min, next_max, greedy, s, j, out);
                }
            }
        }

        if greedy {
            push_stop(out);
        }
    }
}

struct Parser<'a> {
    c: &'a [char],
    i: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    fn alt(&mut self) -> Result<Node, String> {
        let mut branches = vec![self.seq()?];
        while self.peek() == Some('|') {
            self.i += 1;
            branches.push(self.seq()?);
        }
        if branches.len() == 1 {
            Ok(branches.pop().unwrap())
        } else {
            Ok(Node::Alt(branches))
        }
    }

    fn seq(&mut self) -> Result<Node, String> {
        let mut items = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.atom()?;
            let atom = self.quantify(atom)?;
            items.push(atom);
        }
        if items.is_empty() {
            Ok(Node::Empty)
        } else if items.len() == 1 {
            Ok(items.pop().unwrap())
        } else {
            Ok(Node::Seq(items))
        }
    }

    fn quantify(&mut self, atom: Node) -> Result<Node, String> {
        let (min, max) = match self.peek() {
            Some('*') => {
                self.i += 1;
                (0, u32::MAX)
            }
            Some('+') => {
                self.i += 1;
                (1, u32::MAX)
            }
            Some('?') => {
                self.i += 1;
                (0, 1)
            }
            Some('{') => {
                // {m} | {m,} | {m,n}
                let save = self.i;
                self.i += 1;
                let mut m = String::new();
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    m.push(self.c[self.i]);
                    self.i += 1;
                }
                if m.is_empty() {
                    self.i = save;
                    return Ok(atom);
                }
                let min: u32 = m.parse().map_err(|_| "bad repeat".to_string())?;
                let max = if self.peek() == Some(',') {
                    self.i += 1;
                    let mut n = String::new();
                    while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                        n.push(self.c[self.i]);
                        self.i += 1;
                    }
                    if n.is_empty() {
                        u32::MAX
                    } else {
                        n.parse().map_err(|_| "bad repeat".to_string())?
                    }
                } else {
                    min
                };
                if self.peek() != Some('}') {
                    self.i = save;
                    return Ok(atom);
                }
                self.i += 1;
                (min, max)
            }
            _ => return Ok(atom),
        };
        let greedy = if self.peek() == Some('?') {
            self.i += 1;
            false
        } else {
            true
        };
        Ok(Node::Repeat {
            node: Box::new(atom),
            min,
            max,
            greedy,
        })
    }

    fn atom(&mut self) -> Result<Node, String> {
        match self.peek() {
            None => Ok(Node::Empty),
            Some('(') => {
                self.i += 1;
                // non-capturing marker `(?:` is treated as a plain group
                if self.peek() == Some('?') {
                    self.i += 1;
                    if self.peek() == Some(':') {
                        self.i += 1;
                    }
                }
                let inner = self.alt()?;
                if self.peek() != Some(')') {
                    return Err("missing `)`".to_string());
                }
                self.i += 1;
                Ok(inner)
            }
            Some('[') => self.class(),
            Some('.') => {
                self.i += 1;
                Ok(Node::Any)
            }
            Some('^') => {
                self.i += 1;
                Ok(Node::Start)
            }
            Some('$') => {
                self.i += 1;
                Ok(Node::End)
            }
            Some('\\') => {
                self.i += 1;
                let e = self.peek().ok_or("trailing backslash")?;
                self.i += 1;
                self.escape(e)
            }
            Some(c) => {
                self.i += 1;
                Ok(Node::Char(c))
            }
        }
    }

    fn escape(&mut self, e: char) -> Result<Node, String> {
        Ok(match e {
            'd' => Node::Class {
                negated: false,
                ranges: vec![('0', '9')],
            },
            'D' => Node::Class {
                negated: true,
                ranges: vec![('0', '9')],
            },
            'w' => Node::Class {
                negated: false,
                ranges: vec![('a', 'z'), ('A', 'Z'), ('0', '9'), ('_', '_')],
            },
            'W' => Node::Class {
                negated: true,
                ranges: vec![('a', 'z'), ('A', 'Z'), ('0', '9'), ('_', '_')],
            },
            's' => Node::Class {
                negated: false,
                ranges: vec![('\t', '\n'), ('\u{c}', '\r'), (' ', ' ')],
            },
            'S' => Node::Class {
                negated: true,
                ranges: vec![('\t', '\n'), ('\u{c}', '\r'), (' ', ' ')],
            },
            'n' => Node::Char('\n'),
            'r' => Node::Char('\r'),
            't' => Node::Char('\t'),
            other => Node::Char(other),
        })
    }

    fn class(&mut self) -> Result<Node, String> {
        self.i += 1; // '['
        let negated = if self.peek() == Some('^') {
            self.i += 1;
            true
        } else {
            false
        };
        let mut ranges: Vec<(char, char)> = Vec::new();
        let mut first = true;
        loop {
            let c = self.peek().ok_or("unterminated class")?;
            if c == ']' && !first {
                self.i += 1;
                break;
            }
            first = false;
            self.i += 1;
            let lo = if c == '\\' {
                let e = self.peek().ok_or("trailing backslash")?;
                self.i += 1;
                match self.escape(e)? {
                    Node::Char(ch) => ch,
                    Node::Class { ranges: r, .. } => {
                        ranges.extend(r);
                        continue;
                    }
                    _ => e,
                }
            } else {
                c
            };
            if self.peek() == Some('-') && self.c.get(self.i + 1).copied().is_some_and(|x| x != ']')
            {
                self.i += 1;
                let hi = self.peek().ok_or("unterminated range")?;
                self.i += 1;
                ranges.push((lo, hi));
            } else {
                ranges.push((lo, lo));
            }
        }
        Ok(Node::Class { negated, ranges })
    }
}
