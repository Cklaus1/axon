//! R50 S11: the pure-`i64` function tier. A fn qualifies when it passes
//! `fast_call_blocker`, every param and the return are declared `i64` (at
//! most 8 params), its compiled body uses only the ops [`Interp::pure_build`]
//! lowers, on its params and int literals, and every call in it is a proven
//! fast call to a qualifying fn (or itself). Its body then runs as register
//! code ([`FIns`]) on an explicit frame stack, without `Env`, `Value`, the op
//! loop or native recursion. Any instruction that would panic on the tree
//! (overflow, `/` or `%` by zero, `MIN / -1`, the depth limit) declines the
//! WHOLE outermost call: nothing it did is observable (no effects, no
//! bindings, no output), so the caller replays it on the generic path, which
//! produces the tree's panic in the tree's order.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u8)]
pub(super) enum FOp {
    /// `r[d] = r[a] op k` (checked; overflow declines).
    AddK,
    SubK,
    MulK,
    /// `r[d] = r[a] op r[b]` (checked).
    AddR,
    SubR,
    MulR,
    /// `r[d] = int_fast(OTHER[k], r[a], r[b])` for every other int operator
    /// (a zero divisor or `MIN / -1` declines).
    OtherR,
    /// `r[d] = (r[a] cmp r[b])`: 0 or 1 (`t` = cmp).
    CmpR,
    /// Jump to `t` when `!(r[a] cmp k)` (`b` = cmp).
    BrNotK,
    /// Jump to `t` when `r[a] == 0`.
    BrZero,
    /// Jump to `t`.
    Jmp,
    /// `r[d] = k`.
    LoadK,
    /// `r[d] = r[a]`.
    Mov,
    /// Call `fn_table[k]` (`t` registers) with its args in `r[a..]`; the
    /// result lands in `r[a]` (`d == a`).
    Call,
    /// Return `r[a]`.
    Ret,
}

/// One register instruction; jump targets `t` are absolute indices into the
/// fn's code.
#[derive(Clone, Copy, Debug)]
pub(super) struct FIns {
    pub(super) op: FOp,
    pub(super) d: u8,
    pub(super) a: u8,
    pub(super) b: u8,
    pub(super) t: u32,
    pub(super) k: i64,
}

const OTHER: [BinOp; 7] = [
    BinOp::Div,
    BinOp::Rem,
    BinOp::BitAnd,
    BinOp::BitOr,
    BinOp::BitXor,
    BinOp::Shl,
    BinOp::Shr,
];

/// Whether `a c b` holds, `c` a [`cmp_of`] mask: bit 0 for `a < b`, bit 1
/// for `a > b`, bit 2 for `a == b` (branch-free).
#[inline(always)]
fn cmp(c: u8, a: i64, b: i64) -> bool {
    let s = ((a > b) as u8) | (((a == b) as u8) << 1);
    (c >> s) & 1 != 0
}

/// [`cmp`]'s mask for a comparison operator.
fn cmp_of(op: &BinOp) -> Option<u8> {
    Some(match op {
        BinOp::Lt => 0b001,
        BinOp::LtEq => 0b101,
        BinOp::Gt => 0b010,
        BinOp::GtEq => 0b110,
        BinOp::Eq => 0b100,
        BinOp::NotEq => 0b011,
        _ => return None,
    })
}

/// A qualifying fn's register code. Registers: params `0..nparams`, then
/// the operand-stack slots by height. Validated at build: every register
/// index is below `nregs`, every jump target is inside `code`, every call
/// names a table fn, and the code ends in `Ret` or `Jmp`.
pub(in crate::interp) struct PureFn {
    pub(super) code: Box<[FIns]>,
    pub(super) nregs: usize,
}

/// The pure tier's state for one `fn_table` entry, in `Interp::pure_slots`
/// (beside the table, not in `FnEntry`, so the entry's type and drop glue
/// stay as before, cost only): its code once decided (`None`: it never
/// qualifies), and how many builds found it not ready yet.
#[derive(Default)]
pub(in crate::interp) struct PureSlot {
    pub(super) code: std::cell::OnceCell<Option<PureFn>>,
    tries: Cell<u32>,
}

/// A suspended caller in [`run`]: its fn-table index, resume pc, and
/// register base.
#[derive(Clone, Copy)]
pub(in crate::interp) struct PFrame {
    f: u32,
    pc: u32,
    base: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum K {
    I,
    B,
}

/// [`Interp::pure_try`]'s outcome.
pub(super) enum PureOut {
    /// The call's value; it ran, unobservably but for the `tier:` clear.
    Done(i64),
    /// The callee has no pure code, an argument is not an int, or a replay
    /// is running: the generic path.
    No,
    /// The callee's code would panic somewhere: replay the call on the
    /// generic path with nested pure entry off.
    Declined,
}

/// `None`: never qualifies. `Some(None)`: not yet (a body not compiled, a
/// call not proven). `Some(Some(_))`: its code, calls' register counts not
/// yet patched.
type Built = Option<Option<PureFn>>;

fn is_i64(t: &crate::ast::AxonType) -> bool {
    matches!(t, crate::ast::AxonType::Named(n) if n == "i64")
}

/// The pure code of `fn_table[i]`, once built.
#[inline(always)]
fn pure_code(slots: &[PureSlot], i: u32) -> Option<&PureFn> {
    slots.get(i as usize)?.code.get()?.as_ref()
}

/// Write the arguments `args` into the registers `r` (for
/// [`Interp::pure_try`]); `false` at the first that is not an int.
#[inline(always)]
fn int_args(r: &mut [i64], args: impl Iterator<Item = Option<i64>>) -> bool {
    for (slot, a) in r.iter_mut().zip(args) {
        match a {
            Some(n) => *slot = n,
            None => return false,
        }
    }
    true
}

impl<'p> Interp<'p> {
    /// Whether `fn_table[me]` qualifies on its own body; its code when it
    /// does. A callee not decided yet is appended to `group` (it is built
    /// with `me`, so mutual recursion qualifies) and assumed to qualify.
    fn pure_shape(&self, me: u32, group: &mut Vec<u32>) -> Built {
        let entry = &self.fn_table[me as usize];
        if compile::fast_call_blocker(entry, self.refine_preds.is_empty()).is_some()
            || !entry.def.params.iter().all(|p| is_i64(&p.ty))
            || !entry.def.return_type.as_ref().is_some_and(is_i64)
            || entry.params.len() > 8
        {
            return None;
        }
        let body = entry.compiled.as_ref()?.get();
        let Some(body) = body else {
            return Some(None);
        };
        let np = entry.params.len();
        let reg_of = |v: &Var<'_>| -> Option<u8> {
            let i = v.slot as usize;
            (i < np && entry.params[i] == v.s && v.s != SYM_GOAL_MET).then_some(i as u8)
        };
        // Each callee: a proven fast call (proved here when `dispatch_named`
        // has cached the name) to a table fn taking `argc` params that is in
        // the group, already built, or not decided yet (then it joins).
        let mut callee_ok = |callee: &Var<'_>, idx: u32, proven: &Cell<bool>, argc: usize| {
            if !proven.get() && !self.fast_call_proven(callee, idx, proven) {
                return Some(None);
            }
            let ce = self.fn_table.get(idx as usize)?;
            if ce.params.len() != argc {
                return None;
            }
            if group.contains(&idx) {
                return Some(Some(()));
            }
            match self.pure_slots[idx as usize].code.get() {
                Some(Some(_)) => Some(Some(())),
                Some(None) => None,
                None => {
                    group.push(idx);
                    Some(Some(()))
                }
            }
        };
        let mut code: Vec<FIns> = Vec::new();
        // Op index -> code index, for the jump targets.
        let mut at: Vec<u32> = vec![0; body.ops.len() + 1];
        let mut label_h: HashMap<u32, Vec<K>> = HashMap::new();
        let mut kinds: Vec<K> = Vec::new();
        let mut live = true;
        let mut maxh = 0usize;
        let ins = |op, d: usize, a: usize, b: u8, t: u32, k: i64| FIns {
            op,
            d: d as u8,
            a: a as u8,
            b,
            t,
            k,
        };
        let sreg = |h: usize| np + h;
        // A forward jump to op `t` with the operand kinds `ks`: every edge
        // into a label agrees on them.
        let join = |i: usize, t: u32, ks: &Vec<K>, label_h: &mut HashMap<u32, Vec<K>>| {
            if (t as usize) <= i {
                return false;
            }
            match label_h.get(&t) {
                Some(k2) => k2 == ks,
                None => {
                    label_h.insert(t, ks.clone());
                    true
                }
            }
        };
        for (i, op) in body.ops.iter().enumerate() {
            at[i] = code.len() as u32;
            if let Some(ks) = label_h.get(&(i as u32)) {
                if live && kinds != *ks {
                    return None;
                }
                kinds = ks.clone();
                live = true;
            }
            if !live {
                // Unreachable op: emit nothing but keep scanning labels.
                continue;
            }
            match op {
                Op::BranchReturn {
                    op,
                    l,
                    r,
                    val,
                    target,
                } => {
                    let c = cmp_of(op)?;
                    let a = reg_of(l)?;
                    if !join(i, *target, &kinds, &mut label_h) {
                        return None;
                    }
                    code.push(ins(FOp::BrNotK, 0, a as usize, c, *target, *r));
                    match val {
                        Opnd::Int(n) => {
                            let d = sreg(kinds.len());
                            maxh = maxh.max(kinds.len() + 1);
                            code.push(ins(FOp::LoadK, d, 0, 0, 0, *n));
                            code.push(ins(FOp::Ret, 0, d, 0, 0, 0));
                        }
                        Opnd::Local(v) => {
                            let a = reg_of(v)?;
                            code.push(ins(FOp::Ret, 0, a as usize, 0, 0, 0));
                        }
                        _ => return None,
                    }
                    // The then-arm's own ops are dead: `val` is an int.
                    live = false;
                }
                Op::CallFastLocalInt {
                    callee,
                    entry: idx,
                    op,
                    l,
                    r,
                    proven,
                } => {
                    if callee_ok(callee, *idx, proven, 1)?.is_none() {
                        return Some(None);
                    }
                    let a = reg_of(l)?;
                    let d = sreg(kinds.len());
                    let fop = match op {
                        BinOp::Add => FOp::AddK,
                        BinOp::Sub => FOp::SubK,
                        BinOp::Mul => FOp::MulK,
                        _ => return None,
                    };
                    code.push(ins(fop, d, a as usize, 0, 0, *r));
                    code.push(ins(FOp::Call, d, d, 0, 0, *idx as i64));
                    kinds.push(K::I);
                    maxh = maxh.max(kinds.len());
                }
                Op::CallFast {
                    callee,
                    entry: idx,
                    argc,
                    args,
                    proven,
                } => {
                    if callee_ok(callee, *idx, proven, *argc as usize)?.is_none() {
                        return Some(None);
                    }
                    let argc = *argc as usize;
                    let base;
                    if args.is_empty() {
                        // All on the stack.
                        if kinds.len() < argc || kinds[kinds.len() - argc..].contains(&K::B) {
                            return None;
                        }
                        kinds.truncate(kinds.len() - argc);
                        base = sreg(kinds.len());
                    } else {
                        if args.len() != argc {
                            return None;
                        }
                        base = sreg(kinds.len());
                        for (j, a) in args.iter().enumerate() {
                            match a {
                                Opnd::Int(n) => code.push(ins(FOp::LoadK, base + j, 0, 0, 0, *n)),
                                Opnd::Local(v) => {
                                    let r = reg_of(v)?;
                                    code.push(ins(FOp::Mov, base + j, r as usize, 0, 0, 0));
                                }
                                _ => return None,
                            }
                        }
                        maxh = maxh.max(kinds.len() + argc);
                    }
                    code.push(ins(FOp::Call, base, base, 0, 0, *idx as i64));
                    kinds.push(K::I);
                    maxh = maxh.max(kinds.len());
                }
                Op::BinStack(op) => {
                    let n = kinds.len();
                    if n < 2 || kinds[n - 2] != K::I || kinds[n - 1] != K::I {
                        return None;
                    }
                    let (d, a, b) = (sreg(n - 2), sreg(n - 2), sreg(n - 1));
                    kinds.truncate(n - 2);
                    let k = bin(&mut code, op, d, a, b)?;
                    kinds.push(k);
                }
                Op::Bin { op, l, r } => {
                    // Operands: inline (local/int) or the stack.
                    let mut n = kinds.len();
                    let take = |o: &Opnd<'_>, code: &mut Vec<FIns>, tmp: usize| -> Option<usize> {
                        match o {
                            Opnd::Local(v) => Some(reg_of(v)? as usize),
                            Opnd::Int(k) => {
                                code.push(ins(FOp::LoadK, tmp, 0, 0, 0, *k));
                                Some(tmp)
                            }
                            _ => None,
                        }
                    };
                    let (a, b);
                    match (l, r) {
                        (Opnd::Stack, Opnd::Stack) => {
                            if n < 2 || kinds[n - 2] != K::I || kinds[n - 1] != K::I {
                                return None;
                            }
                            a = sreg(n - 2);
                            b = sreg(n - 1);
                            n -= 2;
                        }
                        (Opnd::Stack, r) => {
                            if n < 1 || kinds[n - 1] != K::I {
                                return None;
                            }
                            a = sreg(n - 1);
                            b = take(r, &mut code, sreg(n))?;
                            n -= 1;
                        }
                        (l, r) => {
                            a = take(l, &mut code, sreg(n))?;
                            b = take(r, &mut code, sreg(n + 1))?;
                        }
                    }
                    maxh = maxh.max(n + 2);
                    kinds.truncate(n);
                    let k = bin(&mut code, op, sreg(n), a, b)?;
                    kinds.push(k);
                    maxh = maxh.max(kinds.len());
                }
                Op::Load(v) => {
                    let a = reg_of(v)?;
                    code.push(ins(FOp::Mov, sreg(kinds.len()), a as usize, 0, 0, 0));
                    kinds.push(K::I);
                    maxh = maxh.max(kinds.len());
                }
                Op::Const(Value::Int(n)) => {
                    code.push(ins(FOp::LoadK, sreg(kinds.len()), 0, 0, 0, *n));
                    kinds.push(K::I);
                    maxh = maxh.max(kinds.len());
                }
                Op::BranchLocalInt {
                    op,
                    l,
                    r,
                    target,
                    push: false,
                    ..
                } => {
                    let c = cmp_of(op)?;
                    let a = reg_of(l)?;
                    if !join(i, *target, &kinds, &mut label_h) {
                        return None;
                    }
                    code.push(ins(FOp::BrNotK, 0, a as usize, c, *target, *r));
                }
                Op::BranchFalse {
                    target,
                    push: false,
                    ..
                } => {
                    if kinds.pop()? != K::B || !join(i, *target, &kinds, &mut label_h) {
                        return None;
                    }
                    code.push(ins(FOp::BrZero, 0, sreg(kinds.len()), 0, *target, 0));
                }
                Op::Jump(t) => {
                    if !join(i, *t, &kinds, &mut label_h) {
                        return None;
                    }
                    code.push(ins(FOp::Jmp, 0, 0, 0, *t, 0));
                    live = false;
                }
                Op::Return => {
                    if kinds.pop()? != K::I {
                        return None;
                    }
                    code.push(ins(FOp::Ret, 0, sreg(kinds.len()), 0, 0, 0));
                    live = false;
                }
                _ => return None,
            }
        }
        // The end of the ops: the body's value.
        let end = body.ops.len();
        at[end] = code.len() as u32;
        if let Some(ks) = label_h.get(&(end as u32)) {
            if live && kinds != *ks {
                return None;
            }
            kinds = ks.clone();
            live = true;
        }
        if live {
            if kinds != [K::I] {
                return None;
            }
            code.push(ins(FOp::Ret, 0, sreg(0), 0, 0, 0));
        }
        let nregs = np + maxh + 2;
        if nregs > 255 {
            return None;
        }
        // Jump targets: op indices to code indices.
        for c in code.iter_mut() {
            if matches!(c.op, FOp::BrNotK | FOp::BrZero | FOp::Jmp) {
                c.t = at[c.t as usize];
            }
        }
        Some(Some(PureFn {
            code: code.into_boxed_slice(),
            nregs,
        }))
    }

    /// Decide `fn_table[me]` together with every undecided fn its calls
    /// reach (the group): each member that cannot qualify on its own body is
    /// marked so, and the rest rebuilt without it, until every member
    /// qualifies; then each call gets its callee's register count and every
    /// member its code. `false`, with only never-qualifying members marked,
    /// when a member is not ready yet (a body not compiled, a call not
    /// proven).
    fn pure_build_group(&self, me: u32) -> bool {
        loop {
            let mut group = vec![me];
            let mut codes: Vec<PureFn> = Vec::new();
            let mut never = None;
            while let Some(&g) = group.get(codes.len()) {
                match self.pure_shape(g, &mut group) {
                    None => {
                        never = Some(g);
                        break;
                    }
                    Some(None) => return false,
                    Some(Some(pf)) => codes.push(pf),
                }
            }
            if let Some(g) = never {
                let _ = self.pure_slots[g as usize].code.set(None);
                if g == me {
                    return true;
                }
                continue;
            }
            let nregs: Vec<usize> = codes.iter().map(|c| c.nregs).collect();
            let mut bad = None;
            for (j, pf) in codes.iter_mut().enumerate() {
                for c in pf.code.iter_mut().filter(|c| c.op == FOp::Call) {
                    let idx = c.k as u32;
                    let n = match group.iter().position(|&g| g == idx) {
                        Some(at) => nregs[at],
                        None => pure_code(&self.pure_slots, idx).map_or(0, |f| f.nregs),
                    };
                    c.t = n as u32;
                }
                if bad.is_none() && !pf.valid(self.fn_table.len()) {
                    bad = Some(group[j]);
                }
            }
            if let Some(g) = bad {
                let _ = self.pure_slots[g as usize].code.set(None);
                if g == me {
                    return true;
                }
                continue;
            }
            for (g, pf) in group.into_iter().zip(codes) {
                if self.vm_trace {
                    let name = &self.fn_table[g as usize].def.name;
                    eprintln!("vm: purefn {} {} ins", name, pf.code.len());
                }
                let _ = self.pure_slots[g as usize].code.set(Some(pf));
            }
            return true;
        }
    }

    /// A one-int-argument fast call to `fn_table[idx]` on its pure code.
    /// Out of line, as [`Interp::pure_try_values`].
    #[inline(always)]
    pub(super) fn pure_try1(&self, idx: u32, a: i64) -> PureOut {
        if !self.pure_maybe(idx) {
            return PureOut::No;
        }
        self.pure_try1_out(idx, a)
    }

    #[inline(never)]
    fn pure_try1_out(&self, idx: u32, a: i64) -> PureOut {
        self.pure_try(idx, |r| {
            r[0] = a;
            true
        })
    }

    /// A fast call to `fn_table[idx]` with its arguments `args` on the
    /// operand stack, on its pure code. Out of line, so the guarded caller's
    /// frame (`nest_cost`) stays the generic call's.
    #[inline(always)]
    pub(super) fn pure_try_values(&self, idx: u32, args: &[Value]) -> PureOut {
        if !self.pure_maybe(idx) {
            return PureOut::No;
        }
        self.pure_try_values_out(idx, args)
    }

    #[inline(never)]
    fn pure_try_values_out(&self, idx: u32, args: &[Value]) -> PureOut {
        let np = self.fn_table[idx as usize].params.len();
        self.pure_try(idx, |r| {
            let ints = args.iter().map(|v| match v {
                Value::Int(n) => Some(*n),
                _ => None,
            });
            args.len() == np && int_args(r, ints)
        })
    }

    /// A fast call to `fn_table[idx]` with inline arguments `args` (locals
    /// of `env`, int literals), on its pure code. Out of line, as
    /// [`Interp::pure_try_values`].
    #[inline(always)]
    pub(super) fn pure_try_opnds(&self, idx: u32, args: &[Opnd<'_>], env: &Env) -> PureOut {
        if !self.pure_maybe(idx) {
            return PureOut::No;
        }
        self.pure_try_opnds_out(idx, args, env)
    }

    #[inline(never)]
    fn pure_try_opnds_out(&self, idx: u32, args: &[Opnd<'_>], env: &Env) -> PureOut {
        self.pure_try(idx, |r| {
            let ints = args.iter().map(|a| match a {
                Opnd::Local(var) => match env.get_var(var.s, var.slot) {
                    Some(Value::Int(n)) => Some(*n),
                    _ => None,
                },
                Opnd::Int(n) => Some(*n),
                _ => None,
            });
            int_args(r, ints)
        })
    }

    /// Whether a call to `fn_table[idx]` may take the pure tier: no declined
    /// call is being replayed and the fn is not known never to qualify.
    #[inline(always)]
    fn pure_maybe(&self, idx: u32) -> bool {
        !self.pure_replay.get() && !matches!(self.pure_slots[idx as usize].code.get(), Some(None))
    }

    /// A fast call to `fn_table[idx]` on its pure code; `args` writes the
    /// arguments into the first registers, `false` when one is not an int.
    #[inline(always)]
    fn pure_try(&self, idx: u32, args: impl FnOnce(&mut [i64]) -> bool) -> PureOut {
        if self.pure_replay.get() {
            return PureOut::No;
        }
        let Some(f) = self.pure_of(idx) else {
            return PureOut::No;
        };
        match self.pure_exec(idx, f, args) {
            Some(Some(n)) => {
                self.set_call_tier(None);
                PureOut::Done(n)
            }
            Some(None) => PureOut::Declined,
            None => PureOut::No,
        }
    }

    /// The pure code of `fn_table[idx]`, built on demand.
    #[inline(always)]
    pub(super) fn pure_of(&self, idx: u32) -> Option<&PureFn> {
        match self.pure_slots[idx as usize].code.get() {
            Some(p) => p.as_ref(),
            None => self.pure_init(idx),
        }
    }

    /// [`Interp::pure_of`] before `fn_table[idx]` is decided: build it (and
    /// its group). A fn still not ready after 64 tries never qualifies.
    #[inline(never)]
    fn pure_init(&self, idx: u32) -> Option<&PureFn> {
        let slot = &self.pure_slots[idx as usize];
        if !self.pure_build_group(idx) {
            let tries = slot.tries.get() + 1;
            slot.tries.set(tries);
            if tries >= 64 {
                let _ = slot.code.set(None);
            }
        }
        slot.code.get()?.as_ref()
    }

    /// Run `fn_table[idx]`'s code `f` on the arguments `args` writes.
    /// `None`: an argument is not an int (nothing ran). `Some(None)`: an
    /// instruction would panic on the tree, or the depth limit is reached;
    /// nothing observable happened.
    #[inline(never)]
    fn pure_exec(
        &self,
        idx: u32,
        f: &PureFn,
        args: impl FnOnce(&mut [i64]) -> bool,
    ) -> Option<Option<i64>> {
        let mut regs = self.pure_regs.take();
        if regs.len() < f.nregs {
            regs.resize(f.nregs, 0);
        }
        let out = if !args(&mut regs) {
            None
        } else {
            let depth0 = self.call_depth.get();
            // The outermost call is one level; the frames beyond it the
            // depth limit allows.
            Some(match self.max_depth.checked_sub(depth0 + 1) {
                None => None,
                Some(budget) => {
                    let mut frames = self.pure_frames.take();
                    frames.clear();
                    let v = run(
                        &self.pure_slots,
                        idx,
                        &f.code,
                        &mut regs,
                        &mut frames,
                        budget,
                    );
                    self.pure_frames.set(frames);
                    v
                }
            })
        };
        self.pure_regs.set(regs);
        out
    }
}

impl PureFn {
    /// Every register index is below `nregs`, every jump target is inside
    /// the code, every call names one of `nfns` table fns with a nonzero
    /// register count and `d == a`, and the code ends in `Ret` or `Jmp`.
    pub(super) fn valid(&self, nfns: usize) -> bool {
        let n = self.nregs;
        let len = self.code.len();
        self.code.iter().all(|c| {
            let r = |x: u8| (x as usize) < n;
            let jump = (c.t as usize) < len;
            match c.op {
                FOp::AddK | FOp::SubK | FOp::MulK | FOp::Mov => r(c.d) && r(c.a),
                FOp::AddR | FOp::SubR | FOp::MulR => r(c.d) && r(c.a) && r(c.b),
                FOp::OtherR => r(c.d) && r(c.a) && r(c.b) && (c.k as usize) < OTHER.len(),
                FOp::CmpR => r(c.d) && r(c.a) && r(c.b) && (1..7).contains(&c.t),
                FOp::BrNotK => r(c.a) && (1..7).contains(&c.b) && jump,
                FOp::BrZero => r(c.a) && jump,
                FOp::Jmp => jump,
                FOp::LoadK => r(c.d),
                FOp::Call => r(c.a) && c.d == c.a && (c.k as u64) < nfns as u64 && c.t > 0,
                FOp::Ret => r(c.a),
            }
        }) && matches!(self.code.last(), Some(c) if matches!(c.op, FOp::Ret | FOp::Jmp))
    }
}

/// `r[d] = r[a] op r[b]`; the kind of the result.
fn bin(code: &mut Vec<FIns>, op: &BinOp, d: usize, a: usize, b: usize) -> Option<K> {
    let mk = |op, t, k| FIns {
        op,
        d: d as u8,
        a: a as u8,
        b: b as u8,
        t,
        k,
    };
    if let Some(c) = cmp_of(op) {
        code.push(mk(FOp::CmpR, c as u32, 0));
        return Some(K::B);
    }
    code.push(match op {
        BinOp::Add => mk(FOp::AddR, 0, 0),
        BinOp::Sub => mk(FOp::SubR, 0, 0),
        BinOp::Mul => mk(FOp::MulR, 0, 0),
        _ => mk(FOp::OtherR, 0, OTHER.iter().position(|o| o == op)? as i64),
    });
    Some(K::I)
}

/// The machine, on `fn_table[me]`'s code `code0`. The running fn's registers
/// start at `base` in `regs`; a call's callee frame starts at its argument
/// registers (`Call`'s `a`: the top of the caller's operand stack, nothing
/// live above it), so arguments are passed without a copy and the result
/// lands where the caller's stack expects it. `None`: an instruction would
/// panic on the tree, or a call would pass the depth limit (`budget` frames
/// beyond the outermost).
#[inline(always)]
fn run(
    table: &[PureSlot],
    me: u32,
    code0: &[FIns],
    regs: &mut Vec<i64>,
    frames: &mut Vec<PFrame>,
    budget: usize,
) -> Option<i64> {
    let mut f = me;
    let mut code = code0;
    let mut pc = 0usize;
    let mut base = 0usize;
    let mut r = window(regs, base);
    loop {
        let i = *code.get(pc)?;
        pc += 1;
        let (d, a, b) = (i.d as usize, i.a as usize, i.b as usize);
        match i.op {
            FOp::AddK => r[d] = r[a].checked_add(i.k)?,
            FOp::SubK => r[d] = r[a].checked_sub(i.k)?,
            FOp::MulK => r[d] = r[a].checked_mul(i.k)?,
            FOp::AddR => r[d] = r[a].checked_add(r[b])?,
            FOp::SubR => r[d] = r[a].checked_sub(r[b])?,
            FOp::MulR => r[d] = r[a].checked_mul(r[b])?,
            FOp::OtherR => {
                r[d] = match int_fast(OTHER.get(i.k as usize)?, r[a], r[b])? {
                    Scalar::Int(n) => n,
                    _ => return None,
                };
            }
            FOp::CmpR => r[d] = cmp(i.t as u8, r[a], r[b]) as i64,
            FOp::BrNotK => {
                if !cmp(i.b, r[a], i.k) {
                    pc = i.t as usize;
                }
            }
            FOp::BrZero => {
                if r[a] == 0 {
                    pc = i.t as usize;
                }
            }
            FOp::Jmp => pc = i.t as usize,
            FOp::LoadK => r[d] = i.k,
            FOp::Mov => r[d] = r[a],
            FOp::Call => {
                if frames.len() >= budget {
                    return None;
                }
                frames.push(PFrame {
                    f,
                    pc: pc as u32,
                    base,
                });
                base += a;
                r = window(regs, base);
                if i.k as u32 != f {
                    f = i.k as u32;
                    code = code_at(table, f)?;
                }
                pc = 0;
            }
            FOp::Ret => {
                let v = r[a];
                match frames.pop() {
                    None => return Some(v),
                    Some(fr) => {
                        // The result lands on the callee's first register:
                        // the caller's `Call` `a`/`d`.
                        r[0] = v;
                        if fr.f != f {
                            f = fr.f;
                            code = code_at(table, f)?;
                        }
                        pc = fr.pc as usize;
                        base = fr.base;
                        r = window(regs, base);
                    }
                }
            }
        }
    }
}

/// The pure code of `table[f]`, for a call or return that switches fn.
#[inline(never)]
fn code_at(table: &[PureSlot], f: u32) -> Option<&[FIns]> {
    Some(&pure_code(table, f)?.code)
}

/// The 256 registers a frame at `base` can name (a `u8` index needs no
/// bounds check), growing `regs` to hold them.
#[inline(always)]
fn window(regs: &mut Vec<i64>, base: usize) -> &mut [i64; 256] {
    let end = base + 256;
    if regs.len() < end {
        grow(regs, end);
    }
    (&mut regs[base..end])
        .try_into()
        .expect("window is 256 registers")
}

#[cold]
#[inline(never)]
fn grow(regs: &mut Vec<i64>, end: usize) {
    regs.resize(end.max(regs.len() * 2), 0);
}
