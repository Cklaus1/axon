//! A **Pike VM** regex engine — linear time, no backtracking, ever.
//!
//! R42 Slice 5 / T12.
//!
//! # Why no backtracking is a security requirement, not a performance choice
//!
//! Backtracking engines are exponential on patterns like `(a+)+$`. Axon runs
//! model-authored code under capability sandboxes and per-principal budgets, so
//! an unbounded-time builtin would let sandboxed code burn arbitrary CPU **with
//! no capability at all** — defeating containment rather than merely being slow.
//! So the engine simulates all alternatives in lockstep (Thompson/Pike), giving
//! O(pattern × input), and constructs that cannot be simulated that way are
//! REFUSED at compile time (`E2203`) rather than supported slowly.
//!
//! Refused: **backreferences** (`\1`) and **lookaround** (`(?=`, `(?!`, `(?<=`,
//! `(?<!`). Both require backtracking. This matches RE2 and Rust's `regex`, so it
//! does not break ordinary use.
//!
//! # Leftmost-FIRST, not leftmost-longest
//!
//! An earlier draft of the spec said POSIX leftmost-longest. That is wrong for
//! this language's authorship model: models write PCRE-shaped patterns and expect
//! Perl semantics, where `re_find("a|ab", "ab")` is `"a"`. Under leftmost-longest
//! the lazy quantifiers models write constantly (`.*?`) are also meaningless.
//! Leftmost-first is available in linear time — it is what RE2 and rust-regex do —
//! but it requires a **Pike** VM (threads carrying capture slots, explored in
//! priority order) rather than a plain Thompson simulation. That is also what
//! `re_captures` needs; plain Thompson could not have implemented it.
//!
//! Priority falls out of one rule: `Split(a, b)` explores `a` before `b`, and the
//! FIRST thread to reach `Match` at a given position wins. Greedy quantifiers put
//! the "consume more" branch first; lazy ones swap them.
//!
//! # The `{n,m}` cap
//!
//! Counted repetition is compiled by REPEATING the sub-program, so `a{1,100000}`
//! is linear in a program 100,000× larger than the pattern — a memory and time
//! blow-up hiding inside a bound advertised as linear. Program size is capped
//! (`MAX_PROGRAM`); beyond it, `E2203`.

use std::cell::RefCell;
use std::collections::HashMap;

/// Instruction budget for one compiled pattern. Generous for anything hand- or
/// model-written, and small enough that `a{1,100000}` is refused rather than
/// allocating for it.
const MAX_PROGRAM: usize = 20_000;

/// Maximum capture groups. Bounded so a pathological pattern cannot make each
/// VM thread's capture vector enormous — thread count is already bounded by
/// program length, so slot count is the other multiplier.
const MAX_GROUPS: usize = 32;

#[derive(Clone, Debug)]
enum Inst {
    /// Match one specific character.
    Char(char),
    /// Match any character. `.` does not match `\n`, matching PCRE's default.
    Any,
    /// A character class: inclusive ranges, optionally negated.
    Class { neg: bool, ranges: Vec<(char, char)> },
    /// Try `0` first, then `1`. This ordering IS the leftmost-first rule.
    Split(usize, usize),
    Jmp(usize),
    /// Record the current input offset into capture slot `0`.
    Save(usize),
    /// Start of input (`^`).
    AssertStart,
    /// End of input (`$`).
    AssertEnd,
    Match,
}

#[derive(Clone, Debug)]
enum Ast {
    Empty,
    Char(char),
    Any,
    Class { neg: bool, ranges: Vec<(char, char)> },
    Concat(Vec<Ast>),
    Alt(Vec<Ast>),
    /// `greedy` false means the lazy form (`*?`, `+?`, `??`, `{n,m}?`).
    Repeat { node: Box<Ast>, min: usize, max: Option<usize>, greedy: bool },
    Group { index: usize, node: Box<Ast> },
    AssertStart,
    AssertEnd,
}

struct Parser<'a> {
    chars: Vec<char>,
    pos: usize,
    group_count: usize,
    src: &'a str,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Parser { chars: src.chars().collect(), pos: 0, group_count: 0, src }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// alternation := concat ('|' concat)*
    fn parse_alt(&mut self) -> Result<Ast, String> {
        let mut branches = vec![self.parse_concat()?];
        while self.eat('|') {
            branches.push(self.parse_concat()?);
        }
        if branches.len() == 1 {
            Ok(branches.pop().expect("one branch"))
        } else {
            Ok(Ast::Alt(branches))
        }
    }

    fn parse_concat(&mut self) -> Result<Ast, String> {
        let mut items: Vec<Ast> = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.parse_atom()?;
            let atom = self.parse_quantifier(atom)?;
            items.push(atom);
        }
        match items.len() {
            0 => Ok(Ast::Empty),
            1 => Ok(items.pop().expect("one item")),
            _ => Ok(Ast::Concat(items)),
        }
    }

    fn parse_quantifier(&mut self, atom: Ast) -> Result<Ast, String> {
        let (min, max) = match self.peek() {
            Some('*') => {
                self.pos += 1;
                (0, None)
            }
            Some('+') => {
                self.pos += 1;
                (1, None)
            }
            Some('?') => {
                self.pos += 1;
                (0, Some(1))
            }
            Some('{') => {
                // Only treat `{` as a quantifier when it really parses as one;
                // otherwise it is a literal brace, which is what a model writing
                // `\d{` half-finished would produce.
                match self.try_parse_counted() {
                    Some(bounds) => bounds?,
                    None => return Ok(atom),
                }
            }
            _ => return Ok(atom),
        };
        // A trailing `?` makes the quantifier lazy.
        let greedy = !self.eat('?');
        if let Some(m) = max {
            if m < min {
                return Err(format!(
                    "E2203 invalid repetition {{{min},{m}}}: max is less than min in {:?}",
                    self.src
                ));
            }
        }
        Ok(Ast::Repeat { node: Box::new(atom), min, max, greedy })
    }

    /// `{n}` / `{n,}` / `{n,m}`. Returns None (rewinding) when the braces are not
    /// a counted repetition at all.
    #[allow(clippy::type_complexity)]
    fn try_parse_counted(&mut self) -> Option<Result<(usize, Option<usize>), String>> {
        let start = self.pos;
        self.pos += 1; // consume '{'
        let mut lo = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                lo.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        if lo.is_empty() {
            self.pos = start;
            return None;
        }
        let min: usize = match lo.parse() {
            Ok(v) => v,
            Err(_) => {
                self.pos = start;
                return Some(Err(format!("E2203 repetition count too large in {:?}", self.src)));
            }
        };
        if self.eat('}') {
            return Some(Ok((min, Some(min))));
        }
        if !self.eat(',') {
            self.pos = start;
            return None;
        }
        let mut hi = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                hi.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        if !self.eat('}') {
            self.pos = start;
            return None;
        }
        if hi.is_empty() {
            Some(Ok((min, None)))
        } else {
            match hi.parse::<usize>() {
                Ok(v) => Some(Ok((min, Some(v)))),
                Err(_) => {
                    Some(Err(format!("E2203 repetition count too large in {:?}", self.src)))
                }
            }
        }
    }

    fn parse_atom(&mut self) -> Result<Ast, String> {
        match self.bump() {
            None => Ok(Ast::Empty),
            Some('.') => Ok(Ast::Any),
            Some('^') => Ok(Ast::AssertStart),
            Some('$') => Ok(Ast::AssertEnd),
            Some('[') => self.parse_class(),
            Some('(') => {
                // `(?...)` — the refusal point for lookaround, and where
                // non-capturing groups are accepted.
                if self.eat('?') {
                    match self.peek() {
                        Some(':') => {
                            self.pos += 1;
                            let inner = self.parse_alt()?;
                            if !self.eat(')') {
                                return Err(format!("E2203 unclosed group in {:?}", self.src));
                            }
                            return Ok(inner);
                        }
                        Some('=') | Some('!') => {
                            return Err(format!(
                                "E2203 lookahead is not supported: it requires backtracking, \
                                 which this engine refuses so that matching stays linear-time \
                                 (pattern {:?})",
                                self.src
                            ));
                        }
                        Some('<') => {
                            return Err(format!(
                                "E2203 lookbehind is not supported: it requires backtracking, \
                                 which this engine refuses so that matching stays linear-time \
                                 (pattern {:?})",
                                self.src
                            ));
                        }
                        _ => {
                            return Err(format!(
                                "E2203 unsupported group flags in {:?}",
                                self.src
                            ))
                        }
                    }
                }
                self.group_count += 1;
                if self.group_count > MAX_GROUPS {
                    return Err(format!(
                        "E2203 too many capture groups (max {MAX_GROUPS}) in {:?}",
                        self.src
                    ));
                }
                let index = self.group_count;
                let inner = self.parse_alt()?;
                if !self.eat(')') {
                    return Err(format!("E2203 unclosed group in {:?}", self.src));
                }
                Ok(Ast::Group { index, node: Box::new(inner) })
            }
            Some(')') => Err(format!("E2203 unmatched `)` in {:?}", self.src)),
            Some('\\') => self.parse_escape(),
            Some(c) => Ok(Ast::Char(c)),
        }
    }

    fn parse_escape(&mut self) -> Result<Ast, String> {
        match self.bump() {
            None => Err(format!("E2203 trailing backslash in {:?}", self.src)),
            // A BACKREFERENCE. Refused, not silently reinterpreted as a literal
            // digit — treating `\1` as `1` would match something the author did
            // not ask for, which is worse than refusing.
            Some(d) if d.is_ascii_digit() => Err(format!(
                "E2203 backreference `\\{d}` is not supported: it requires backtracking, which \
                 this engine refuses so that matching stays linear-time (pattern {:?})",
                self.src
            )),
            Some('d') => Ok(Ast::Class { neg: false, ranges: vec![('0', '9')] }),
            Some('D') => Ok(Ast::Class { neg: true, ranges: vec![('0', '9')] }),
            Some('w') => Ok(Ast::Class {
                neg: false,
                ranges: vec![('a', 'z'), ('A', 'Z'), ('0', '9'), ('_', '_')],
            }),
            Some('W') => Ok(Ast::Class {
                neg: true,
                ranges: vec![('a', 'z'), ('A', 'Z'), ('0', '9'), ('_', '_')],
            }),
            Some('s') => Ok(Ast::Class {
                neg: false,
                ranges: vec![(' ', ' '), ('\t', '\t'), ('\n', '\n'), ('\r', '\r')],
            }),
            Some('S') => Ok(Ast::Class {
                neg: true,
                ranges: vec![(' ', ' '), ('\t', '\t'), ('\n', '\n'), ('\r', '\r')],
            }),
            Some('n') => Ok(Ast::Char('\n')),
            Some('t') => Ok(Ast::Char('\t')),
            Some('r') => Ok(Ast::Char('\r')),
            Some(c) => Ok(Ast::Char(c)),
        }
    }

    fn parse_class(&mut self) -> Result<Ast, String> {
        let neg = self.eat('^');
        let mut ranges: Vec<(char, char)> = Vec::new();
        let mut first = true;
        loop {
            let c = match self.peek() {
                None => return Err(format!("E2203 unclosed character class in {:?}", self.src)),
                // `]` as the FIRST member is a literal, per POSIX/PCRE.
                Some(']') if !first => {
                    self.pos += 1;
                    break;
                }
                Some(c) => {
                    self.pos += 1;
                    c
                }
            };
            first = false;
            let lo = if c == '\\' {
                match self.bump() {
                    None => return Err(format!("E2203 trailing backslash in {:?}", self.src)),
                    Some('d') => {
                        ranges.push(('0', '9'));
                        continue;
                    }
                    Some('w') => {
                        ranges.extend([('a', 'z'), ('A', 'Z'), ('0', '9'), ('_', '_')]);
                        continue;
                    }
                    Some('s') => {
                        ranges.extend([(' ', ' '), ('\t', '\t'), ('\n', '\n'), ('\r', '\r')]);
                        continue;
                    }
                    Some('n') => '\n',
                    Some('t') => '\t',
                    Some('r') => '\r',
                    Some(e) => e,
                }
            } else {
                c
            };
            // A range, unless the `-` is last (then it is a literal).
            if self.peek() == Some('-') && self.chars.get(self.pos + 1) != Some(&']') {
                self.pos += 1;
                let hi = match self.bump() {
                    None => return Err(format!("E2203 unclosed character class in {:?}", self.src)),
                    Some('\\') => self.bump().unwrap_or('\\'),
                    Some(h) => h,
                };
                if hi < lo {
                    return Err(format!(
                        "E2203 reversed range {lo}-{hi} in character class in {:?}",
                        self.src
                    ));
                }
                ranges.push((lo, hi));
            } else {
                ranges.push((lo, lo));
            }
        }
        Ok(Ast::Class { neg, ranges })
    }
}

struct Compiler {
    prog: Vec<Inst>,
}

impl Compiler {
    fn emit(&mut self, i: Inst) -> Result<usize, String> {
        if self.prog.len() >= MAX_PROGRAM {
            // The `{n,m}` blow-up guard. Counted repetition is compiled by
            // REPEATING the sub-program, so this is where `a{1,100000}` stops.
            return Err(format!(
                "E2203 pattern is too large once counted repetitions are expanded (limit \
                 {MAX_PROGRAM} instructions) — a bound like `{{1,100000}}` expands the program by \
                 that factor, which is a resource blow-up hiding inside a linear-time guarantee"
            ));
        }
        self.prog.push(i);
        Ok(self.prog.len() - 1)
    }

    fn compile(&mut self, ast: &Ast) -> Result<(), String> {
        match ast {
            Ast::Empty => Ok(()),
            Ast::Char(c) => self.emit(Inst::Char(*c)).map(|_| ()),
            Ast::Any => self.emit(Inst::Any).map(|_| ()),
            Ast::Class { neg, ranges } => self
                .emit(Inst::Class { neg: *neg, ranges: ranges.clone() })
                .map(|_| ()),
            Ast::AssertStart => self.emit(Inst::AssertStart).map(|_| ()),
            Ast::AssertEnd => self.emit(Inst::AssertEnd).map(|_| ()),
            Ast::Concat(items) => {
                for it in items {
                    self.compile(it)?;
                }
                Ok(())
            }
            Ast::Group { index, node } => {
                self.emit(Inst::Save(index * 2))?;
                self.compile(node)?;
                self.emit(Inst::Save(index * 2 + 1))?;
                Ok(())
            }
            Ast::Alt(branches) => {
                // Chain of Splits. Earlier branches get priority, which is what
                // makes `a|ab` match "a" in "ab" (leftmost-FIRST).
                let mut jumps: Vec<usize> = Vec::new();
                for (n, b) in branches.iter().enumerate() {
                    if n + 1 < branches.len() {
                        let split = self.emit(Inst::Split(0, 0))?;
                        let body = self.prog.len();
                        self.compile(b)?;
                        let jmp = self.emit(Inst::Jmp(0))?;
                        jumps.push(jmp);
                        let next = self.prog.len();
                        self.prog[split] = Inst::Split(body, next);
                    } else {
                        self.compile(b)?;
                    }
                }
                let end = self.prog.len();
                for j in jumps {
                    self.prog[j] = Inst::Jmp(end);
                }
                Ok(())
            }
            Ast::Repeat { node, min, max, greedy } => {
                match (min, max) {
                    (0, None) => {
                        // star
                        let split = self.emit(Inst::Split(0, 0))?;
                        let body = self.prog.len();
                        self.compile(node)?;
                        self.emit(Inst::Jmp(split))?;
                        let after = self.prog.len();
                        self.prog[split] = if *greedy {
                            Inst::Split(body, after)
                        } else {
                            Inst::Split(after, body)
                        };
                    }
                    (1, None) => {
                        // plus
                        let body = self.prog.len();
                        self.compile(node)?;
                        let split = self.emit(Inst::Split(0, 0))?;
                        let after = self.prog.len();
                        self.prog[split] = if *greedy {
                            Inst::Split(body, after)
                        } else {
                            Inst::Split(after, body)
                        };
                    }
                    (0, Some(1)) => {
                        // question mark
                        let split = self.emit(Inst::Split(0, 0))?;
                        let body = self.prog.len();
                        self.compile(node)?;
                        let after = self.prog.len();
                        self.prog[split] = if *greedy {
                            Inst::Split(body, after)
                        } else {
                            Inst::Split(after, body)
                        };
                    }
                    (lo, hi) => {
                        // Counted: emit `lo` mandatory copies, then either an
                        // unbounded star or `hi - lo` optional copies. THIS is
                        // the expansion `MAX_PROGRAM` bounds.
                        for _ in 0..*lo {
                            self.compile(node)?;
                        }
                        match hi {
                            None => {
                                let split = self.emit(Inst::Split(0, 0))?;
                                let body = self.prog.len();
                                self.compile(node)?;
                                self.emit(Inst::Jmp(split))?;
                                let after = self.prog.len();
                                self.prog[split] = if *greedy {
                                    Inst::Split(body, after)
                                } else {
                                    Inst::Split(after, body)
                                };
                            }
                            Some(h) => {
                                let optional = h.saturating_sub(*lo);
                                let mut splits = Vec::new();
                                for _ in 0..optional {
                                    let s = self.emit(Inst::Split(0, 0))?;
                                    let body = self.prog.len();
                                    self.compile(node)?;
                                    splits.push((s, body));
                                }
                                let after = self.prog.len();
                                for (s, body) in splits {
                                    self.prog[s] = if *greedy {
                                        Inst::Split(body, after)
                                    } else {
                                        Inst::Split(after, body)
                                    };
                                }
                            }
                        }
                    }
                }
                Ok(())
            }
        }
    }
}

/// A compiled pattern: the program plus how many capture groups it has.
#[derive(Clone, Debug)]
pub struct Program {
    prog: Vec<Inst>,
    pub groups: usize,
}

thread_local! {
    /// Compiled-pattern cache. These builtins take the pattern per CALL, so
    /// without this `re_find_all` in a loop recompiles every iteration — the
    /// compile cost would dominate the linear-time match it exists to protect.
    static CACHE: RefCell<HashMap<String, Program>> = RefCell::new(HashMap::new());
}

/// Compile `pattern`, or return an `E2203`-prefixed message.
pub fn compile(pattern: &str) -> Result<Program, String> {
    if let Some(p) = CACHE.with(|c| c.borrow().get(pattern).cloned()) {
        return Ok(p);
    }
    let mut parser = Parser::new(pattern);
    let ast = parser.parse_alt()?;
    if parser.pos < parser.chars.len() {
        return Err(format!("E2203 unexpected `)` or trailing input in {pattern:?}"));
    }
    let groups = parser.group_count;
    let mut c = Compiler { prog: Vec::new() };
    // Slot 0/1 are the whole match, so the top level is group 0.
    c.emit(Inst::Save(0))?;
    c.compile(&ast)?;
    c.emit(Inst::Save(1))?;
    c.emit(Inst::Match)?;
    let program = Program { prog: c.prog, groups };
    CACHE.with(|cache| {
        let mut m = cache.borrow_mut();
        // Bound the cache: a program generating patterns in a loop should not
        // grow it without limit.
        if m.len() > 256 {
            m.clear();
        }
        m.insert(pattern.to_string(), program.clone());
    });
    Ok(program)
}

/// Capture slots for one match: `[start0, end0, start1, end1, ...]`, `usize::MAX`
/// for a group that did not participate.
type Slots = Vec<usize>;

struct Thread {
    pc: usize,
    slots: Slots,
}

/// Run `prog` at `start`, returning capture slots for the leftmost-first match
/// beginning exactly there, or None.
///
/// This is the Pike VM proper: one pass over the input, a list of threads per
/// position, explored in priority order. `seen` dedupes by pc so the thread list
/// is bounded by program length — the property that makes it linear.
fn run_at(p: &Program, input: &[char], start: usize) -> Option<Slots> {
    let nslots = (p.groups + 1) * 2;
    let mut clist: Vec<Thread> = Vec::new();
    let mut seen = vec![false; p.prog.len()];

    fn add(
        p: &Program,
        list: &mut Vec<Thread>,
        seen: &mut Vec<bool>,
        pc: usize,
        pos: usize,
        input: &[char],
        slots: Slots,
    ) {
        if seen[pc] {
            return;
        }
        seen[pc] = true;
        match &p.prog[pc] {
            // Epsilon instructions are followed IMMEDIATELY, in priority order,
            // so the thread list only ever holds consuming instructions.
            Inst::Jmp(t) => add(p, list, seen, *t, pos, input, slots),
            Inst::Split(a, b) => {
                add(p, list, seen, *a, pos, input, slots.clone());
                add(p, list, seen, *b, pos, input, slots);
            }
            Inst::Save(n) => {
                let mut s = slots;
                if *n < s.len() {
                    s[*n] = pos;
                }
                add(p, list, seen, pc + 1, pos, input, s);
            }
            Inst::AssertStart => {
                if pos == 0 {
                    add(p, list, seen, pc + 1, pos, input, slots);
                }
            }
            Inst::AssertEnd => {
                if pos == input.len() {
                    add(p, list, seen, pc + 1, pos, input, slots);
                }
            }
            _ => list.push(Thread { pc, slots }),
        }
    }

    add(p, &mut clist, &mut seen, 0, start, input, vec![usize::MAX; nslots]);

    let mut matched: Option<Slots> = None;
    let mut pos = start;
    loop {
        let mut nlist: Vec<Thread> = Vec::new();
        let mut nseen = vec![false; p.prog.len()];
        for th in clist.into_iter() {
            match &p.prog[th.pc] {
                Inst::Match => {
                    // FIRST thread to match wins — priority order is what makes
                    // this leftmost-FIRST rather than leftmost-longest. Lower
                    // priority threads are discarded.
                    matched = Some(th.slots);
                    break;
                }
                Inst::Char(c) => {
                    if pos < input.len() && input[pos] == *c {
                        add(p, &mut nlist, &mut nseen, th.pc + 1, pos + 1, input, th.slots);
                    }
                }
                Inst::Any => {
                    // `.` excludes newline, matching PCRE's default.
                    if pos < input.len() && input[pos] != '\n' {
                        add(p, &mut nlist, &mut nseen, th.pc + 1, pos + 1, input, th.slots);
                    }
                }
                Inst::Class { neg, ranges } => {
                    if pos < input.len() {
                        let ch = input[pos];
                        let inside = ranges.iter().any(|(lo, hi)| ch >= *lo && ch <= *hi);
                        if inside != *neg {
                            add(p, &mut nlist, &mut nseen, th.pc + 1, pos + 1, input, th.slots);
                        }
                    }
                }
                // Epsilon instructions never reach the thread list.
                _ => {}
            }
        }
        // Reaching `Match` records the match and CUTS lower-priority threads (the
        // `break` above), but it must NOT stop the machine: threads already in
        // `nlist` came from HIGHER-priority positions in `clist` and are still
        // alive. Stopping here was a real bug — it made `(?:ab)+` on "ababc"
        // match "ab", because the Split's exit branch reached `Match` in the same
        // pass that the loop branch queued another "ab". Greedy repetition IS a
        // higher-priority thread outliving a recorded match, so the recorded
        // match is simply overwritten as longer ones are found.
        if nlist.is_empty() || pos >= input.len() {
            return matched;
        }
        clist = nlist;
        pos += 1;
    }
}

/// The leftmost match: try each start offset in order. Returns character-index
/// capture slots.
pub fn find_from(p: &Program, input: &[char], from: usize) -> Option<Slots> {
    let mut start = from;
    while start <= input.len() {
        if let Some(s) = run_at(p, input, start) {
            return Some(s);
        }
        start += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pat: &str, s: &str) -> Option<String> {
        let p = compile(pat).expect("compiles");
        let cs: Vec<char> = s.chars().collect();
        find_from(&p, &cs, 0).map(|sl| cs[sl[0]..sl[1]].iter().collect())
    }

    #[test]
    fn leftmost_first_not_longest() {
        // THE semantics test. Under leftmost-LONGEST this would be "ab".
        assert_eq!(m("a|ab", "ab").as_deref(), Some("a"));
        // And the other order gives the other answer, which is the point:
        // priority is the branch order, not the match length.
        assert_eq!(m("ab|a", "ab").as_deref(), Some("ab"));
    }

    #[test]
    fn greedy_and_lazy_differ() {
        assert_eq!(m("<.*>", "<a><b>").as_deref(), Some("<a><b>"));
        // Lazy is meaningless under leftmost-longest; here it stops early.
        assert_eq!(m("<.*?>", "<a><b>").as_deref(), Some("<a>"));
    }

    #[test]
    fn classes_anchors_and_counted() {
        assert_eq!(m(r"\d+", "ab123cd").as_deref(), Some("123"));
        assert_eq!(m("[^a-z]+", "abc123").as_deref(), Some("123"));
        assert_eq!(m("^abc$", "abc").as_deref(), Some("abc"));
        assert!(m("^abc$", "xabc").is_none());
        assert_eq!(m("a{2,3}", "aaaa").as_deref(), Some("aaa"));
        assert_eq!(m("a{2}", "aaaa").as_deref(), Some("aa"));
    }

    #[test]
    fn backreference_and_lookaround_are_refused() {
        for pat in [r"(a)\1", "(?=a)", "(?!a)", "(?<=a)", "(?<!a)"] {
            let e = compile(pat).expect_err("must refuse");
            assert!(e.contains("E2203"), "{pat}: {e}");
            assert!(
                e.contains("backtrack"),
                "the refusal should say WHY (backtracking): {pat}: {e}"
            );
        }
    }

    #[test]
    fn counted_repetition_blowup_is_refused_not_allocated() {
        // The DoS guard: linear in a program 100_000x the pattern is not linear
        // in any useful sense.
        let e = compile("a{1,100000}").expect_err("must refuse");
        assert!(e.contains("E2203"), "{e}");
        // But an ordinary bound still compiles.
        assert!(compile("a{1,50}").is_ok());
    }

    /// The pattern that makes a backtracking engine explode. Here it must return
    /// promptly — this test would hang, not fail, on a backtracking engine.
    #[test]
    fn catastrophic_pattern_is_linear() {
        let input: String = "a".repeat(40);
        let p = compile("(a+)+$").expect("compiles");
        let cs: Vec<char> = input.chars().collect();
        assert!(find_from(&p, &cs, 0).is_some());
        // And the non-matching case, which is the one that explodes classically.
        let bad: String = format!("{}b", "a".repeat(40));
        let cs2: Vec<char> = bad.chars().collect();
        assert!(find_from(&p, &cs2, 0).is_none());
    }

    #[test]
    fn groups_capture_positions() {
        let p = compile(r"(\d+)-(\d+)").expect("compiles");
        let cs: Vec<char> = "x 12-345 y".chars().collect();
        let s = find_from(&p, &cs, 0).expect("matches");
        let whole: String = cs[s[0]..s[1]].iter().collect();
        let g1: String = cs[s[2]..s[3]].iter().collect();
        let g2: String = cs[s[4]..s[5]].iter().collect();
        assert_eq!(whole, "12-345");
        assert_eq!(g1, "12");
        assert_eq!(g2, "345");
    }

    #[test]
    fn non_capturing_group_is_accepted() {
        assert_eq!(m("(?:ab)+", "ababc").as_deref(), Some("abab"));
    }

    #[test]
    fn malformed_patterns_are_errors_not_panics() {
        for pat in ["(", "[a", r"\", "a{2,1}", "[z-a]", ")"] {
            let e = compile(pat).expect_err("must refuse");
            assert!(e.contains("E2203"), "{pat}: {e}");
        }
    }
}
