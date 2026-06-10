//! Goal-directed optimization for the interpreter (R0 slice 3 — extracted
//! verbatim from `interp.rs`, zero behavior change). The `goal_run` search
//! strategies (warm/random/multistart/tournament), the hill-climb cores
//! (i64 + f64, single + multi-dim), and the goal-introspection readers
//! (best_observed/history/clear/best_input(s)/goal_eval_holdout/goal_spec_of).
//! These are `impl Interp` methods moved into a second inherent-impl block;
//! `self.<method>` resolves across split impl blocks, and they reach the
//! parent's `call_fn`/`eval`/`goal_name_is_known` the same way. `use super::*`
//! pulls in Interp, Value, Flow, FnDef, GoalSpec, R, and the free helpers
//! (read_best_input, goal_continue_enabled, numeric_score, …).

use super::*;

impl<'p> Interp<'p> {
    pub(super) fn run_goal_warm(&self, name: &str, target: f64, max_evals: i64) -> Result<f64, Flow> {
        let _goal_guard = self.enter_goal(name);
        if !self.goal_name_is_known(name) {
            return Err(Self::unknown_goal_name(name));
        }
        if let Some(f) = self.fns.get(name) {
            let f = *f;
            let is_adaptive = f.attrs.iter().any(|a| a.name == "adaptive");
            let all_i64_params = !f.params.is_empty()
                && f.params.iter().all(|p| is_i64_type(&p.ty));
            let all_f64_params = !f.params.is_empty()
                && f.params.iter().all(|p| is_f64_type(&p.ty));
            let i64_ret = f.return_type.as_ref().map(is_i64_scored_ret).unwrap_or(false);
            let f64_ret = f.return_type.as_ref().map(is_f64_scored_ret).unwrap_or(false);
            if is_adaptive {
                if all_i64_params && i64_ret {
                    if f.params.len() == 1 {
                        // Single-arg: already has the on-disk continuation hook.
                        return self.hill_climb_i64(f, target, max_evals);
                    }
                    // Multi-arg: seed from in-memory best prior tuple.
                    let seed = self.best_input_index(name, target).and_then(|idx| {
                        self.provenance_inputs
                            .borrow()
                            .get(name)
                            .and_then(|v| v.get(idx).cloned())
                    });
                    return self.hill_climb_multi_i64_from(f, target, max_evals, seed);
                }
                if all_f64_params && f64_ret {
                    let seed = self.best_input_index(name, target).and_then(|idx| {
                        self.provenance_inputs_f64
                            .borrow()
                            .get(name)
                            .and_then(|v| v.get(idx).cloned())
                    });
                    return self.hill_climb_multi_f64_from(f, target, max_evals, seed);
                }
            }
        }
        Ok(self.best_observed(name, target, max_evals))
    }

    /// Random-search strategy for an `@[adaptive] fn(i64, …) -> i64`.
    /// Samples `n_samples` random i64 tuples uniformly in `[lo, hi)`
    /// (per-dim independent) and scores each. Returns the best score
    /// (closest to target). Provides a baseline against `goal_run`'s
    /// hill climb — useful for multi-modal objectives where the
    /// gradient strategy gets stuck in local optima, and for
    /// "is the optimizer actually doing anything?" sanity checks.
    /// Each call flows through `call_fn`, so provenance accumulates
    /// just like the hill-climb path; `goal_best_input` / `_inputs`
    /// can read the winner back.
    pub(super) fn run_goal_random(
        &self,
        name: &str,
        target: f64,
        n_samples: i64,
        lo: i64,
        hi: i64,
    ) -> Result<f64, Flow> {
        let _goal_guard = self.enter_goal(name);
        if hi <= lo {
            return panic(format!(
                "goal_run_random: hi ({hi}) must be greater than lo ({lo})"
            ));
        }
        let f = match self.fns.get(name) {
            Some(f) => *f,
            // Unknown fn but with provenance → retrospective lookup is fine.
            // Unknown fn and no provenance → typo (BUG_HUNT #19 / I-9).
            None if self.provenance.borrow().contains_key(name) => {
                return Ok(self.best_observed(name, target, n_samples));
            }
            None => return Err(Self::unknown_goal_name(name)),
        };
        let is_adaptive = f.attrs.iter().any(|a| a.name == "adaptive");
        let all_i64_params = !f.params.is_empty()
            && f.params.iter().all(|p| is_i64_type(&p.ty));
        let i64_ret = f.return_type.as_ref().map(is_i64_scored_ret).unwrap_or(false);
        if !is_adaptive || !all_i64_params || !i64_ret {
            return Ok(self.best_observed(name, target, n_samples));
        }
        let n_dims = f.params.len();
        // Start "unset" — first probe wins regardless of its distance,
        // then every subsequent probe must beat it. Initializing from
        // `best_observed` is wrong here because that returns `target`
        // when no provenance exists, locking `best_dist` at 0 and
        // preventing every later probe from being accepted.
        let mut best_score: f64 = f64::NAN;
        let mut best_dist: f64 = f64::INFINITY;
        let mut i: i64 = 0;
        while i < n_samples {
            // Uniform per-dim in `[lo, hi)` via the same `next_rand_u64`
            // helper the `random_i64` builtin uses — keeps RNG semantics
            // consistent across calls within one run.
            let mut probe: Vec<Value> = Vec::with_capacity(n_dims);
            let range = (hi as i128 - lo as i128) as u128;
            for _ in 0..n_dims {
                let v = lo + (next_rand_u64() as u128 % range.max(1)) as i64;
                probe.push(Value::Int(v));
            }
            let result = self.call_fn(f, probe)?;
            // numeric_score unwraps a soft wrapper (Uncertain<i64>/Temporal<i64>)
            // to its inner score, so an AI scorer returning an Uncertain is optimized.
            let score = match numeric_score(&result) {
                Some(s) => s,
                None => return panic(format!(
                    "@[adaptive] fn `{}` must return a number, got {}",
                    f.name,
                    result.type_name()
                )),
            };
            let d = (score - target).abs();
            if d < best_dist {
                best_dist = d;
                best_score = score;
                if best_dist <= f64::EPSILON {
                    return Ok(best_score);
                }
            }
            i += 1;
        }
        // If no probes ran (n_samples <= 0), fall back to "target" so the
        // caller's contract ("return the best") still makes sense.
        if best_score.is_nan() {
            best_score = target;
        }
        Ok(best_score)
    }

    /// Multi-start hill climb. Picks `n_starts` random starting points
    /// uniformly in `[lo, hi)` (per-dim) and runs the existing
    /// coordinate-descent hill-climb (with Powell joint step) from each
    /// with `evals_per_start` budget. Returns the best score across all
    /// starts. Standard recipe for escaping local optima on multi-modal
    /// objectives — keeps gradient-style refinement once a basin is
    /// found while still sampling broadly. Provenance accumulates as a
    /// side effect (so goal_best_input reads the winning probe back).
    pub(super) fn run_goal_multistart(
        &self,
        name: &str,
        target: f64,
        n_starts: i64,
        evals_per_start: i64,
        lo: i64,
        hi: i64,
    ) -> Result<f64, Flow> {
        let _goal_guard = self.enter_goal(name);
        if hi <= lo {
            return panic(format!(
                "goal_run_multistart: hi ({hi}) must be greater than lo ({lo})"
            ));
        }
        let f = match self.fns.get(name) {
            Some(f) => *f,
            None if self.provenance.borrow().contains_key(name) => {
                return Ok(self.best_observed(name, target, 0));
            }
            None => return Err(Self::unknown_goal_name(name)),
        };
        let is_adaptive = f.attrs.iter().any(|a| a.name == "adaptive");
        let all_i64_params = !f.params.is_empty()
            && f.params.iter().all(|p| is_i64_type(&p.ty));
        let i64_ret = f.return_type.as_ref().map(is_i64_scored_ret).unwrap_or(false);
        if !is_adaptive || !all_i64_params || !i64_ret {
            return Ok(self.best_observed(name, target, 0));
        }
        let n_dims = f.params.len();
        let mut best_score: f64 = f64::NAN;
        let mut best_dist: f64 = f64::INFINITY;
        let range = (hi as i128 - lo as i128) as u128;
        let mut s: i64 = 0;
        while s < n_starts {
            let start: Vec<i64> = (0..n_dims)
                .map(|_| lo + (next_rand_u64() as u128 % range.max(1)) as i64)
                .collect();
            let score = if n_dims == 1 {
                // Single-arg: bypass the multi-i64 path's per-dim wiring
                // (which expects Vec<i64>) and call the 1-D climber
                // directly, but its API doesn't take a start point —
                // use multi-i64_from with a 1-elem vec instead.
                self.hill_climb_multi_i64_from(f, target, evals_per_start, Some(start))?
            } else {
                self.hill_climb_multi_i64_from(f, target, evals_per_start, Some(start))?
            };
            let d = (score - target).abs();
            if d < best_dist {
                best_dist = d;
                best_score = score;
                if best_dist <= f64::EPSILON {
                    return Ok(best_score);
                }
            }
            s += 1;
        }
        if best_score.is_nan() {
            best_score = target;
        }
        Ok(best_score)
    }

    /// Tournament / evolutionary strategy (PRD L889-893): sample a population in
    /// `[lo, hi)`, score each, keep the top-K elites, then breed the next
    /// generation by mutating around the elites (Gaussian-ish integer jitter
    /// that shrinks each generation), repeating until the budget is spent.
    /// Direction-agnostic: returns the score closest to `target`. Each eval
    /// flows through `call_fn`, so provenance accumulates exactly like the other
    /// strategies. `exploit` (the `bayesian` mapping) keeps a SINGLE elite and
    /// spends the budget refining its basin — the honest exploit-heavy
    /// approximation of a surrogate search (no GP model is built).
    pub(super) fn run_goal_tournament(
        &self,
        name: &str,
        target: f64,
        max_evals: i64,
        lo: i64,
        hi: i64,
        exploit: bool,
    ) -> Result<f64, Flow> {
        let _goal_guard = self.enter_goal(name);
        if hi <= lo {
            return panic(format!(
                "goal tournament: hi ({hi}) must be greater than lo ({lo})"
            ));
        }
        let f = match self.fns.get(name) {
            Some(f) => *f,
            None if self.provenance.borrow().contains_key(name) => {
                return Ok(self.best_observed(name, target, 0));
            }
            None => return Err(Self::unknown_goal_name(name)),
        };
        let is_adaptive = f.attrs.iter().any(|a| a.name == "adaptive");
        let all_i64_params = !f.params.is_empty() && f.params.iter().all(|p| is_i64_type(&p.ty));
        let i64_ret = f.return_type.as_ref().map(is_i64_scored_ret).unwrap_or(false);
        if !is_adaptive || !all_i64_params || !i64_ret {
            return Ok(self.best_observed(name, target, 0));
        }
        let n_dims = f.params.len();
        let range = (hi as i128 - lo as i128) as u128;

        let eval = |probe: &[i64]| -> Result<f64, Flow> {
            let args: Vec<Value> = probe.iter().map(|&x| Value::Int(x)).collect();
            let other = self.call_fn(f, args)?;
            match numeric_score(&other) {
                Some(s) => Ok(s),
                None => panic(format!(
                    "@[adaptive] fn `{}` must return a number, got {}",
                    f.name, other.type_name()
                )),
            }
        };

        // Population + elite sizing from the budget. exploit → 1 elite (refine).
        let total = max_evals.max(1);
        let pop: i64 = if exploit { 4 } else { 8 }.min(total.max(1));
        let elites: usize = if exploit { 1 } else { 3 };
        let mut best_score = f64::NAN;
        let mut best_dist = f64::INFINITY;

        // Generation 0: uniform random population.
        let mut population: Vec<Vec<i64>> = (0..pop)
            .map(|_| {
                (0..n_dims)
                    .map(|_| lo + (next_rand_u64() as u128 % range.max(1)) as i64)
                    .collect()
            })
            .collect();

        let mut spent: i64 = 0;
        let mut step = (range / 4).max(1) as i64;
        while spent < total {
            // Score the population.
            let mut scored: Vec<(f64, Vec<i64>)> = Vec::with_capacity(population.len());
            for probe in &population {
                if spent >= total {
                    break;
                }
                let s = eval(probe)?;
                spent += 1;
                let d = (s - target).abs();
                if d < best_dist {
                    best_dist = d;
                    best_score = s;
                    if best_dist <= f64::EPSILON {
                        return Ok(best_score);
                    }
                }
                scored.push((d, probe.clone()));
            }
            if scored.is_empty() {
                break;
            }
            // Keep the top-K closest to target (smallest distance).
            scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            scored.truncate(elites);

            // Breed the next generation by mutating around the elites; shrink
            // the mutation step each generation (coarse → fine).
            let mut next: Vec<Vec<i64>> = Vec::with_capacity(pop as usize);
            for (_, elite) in &scored {
                next.push(elite.clone()); // carry the elite forward (elitism)
            }
            while (next.len() as i64) < pop {
                let (_, parent) = &scored[(next_rand_u64() as usize) % scored.len()];
                let child: Vec<i64> = parent
                    .iter()
                    .map(|&g| {
                        let jitter = (next_rand_u64() as i64 % (2 * step + 1)) - step;
                        (g + jitter).clamp(lo, hi - 1)
                    })
                    .collect();
                next.push(child);
            }
            population = next;
            step = (step / 2).max(1);
        }

        if best_score.is_nan() {
            best_score = target;
        }
        Ok(best_score)
    }

    pub(super) fn run_goal(&self, name: &str, target: f64, max_evals: i64) -> Result<f64, Flow> {
        let _goal_guard = self.enter_goal(name);
        if !self.goal_name_is_known(name) {
            return Err(Self::unknown_goal_name(name));
        }
        if let Some(f) = self.fns.get(name) {
            let f = *f;
            let is_adaptive = f.attrs.iter().any(|a| a.name == "adaptive");
            let all_i64_params = !f.params.is_empty()
                && f.params.iter().all(|p| is_i64_type(&p.ty));
            let all_f64_params = !f.params.is_empty()
                && f.params.iter().all(|p| is_f64_type(&p.ty));
            let i64_ret = f.return_type.as_ref().map(is_i64_scored_ret).unwrap_or(false);
            let f64_ret = f.return_type.as_ref().map(is_f64_scored_ret).unwrap_or(false);
            if is_adaptive {
                if all_i64_params && i64_ret {
                    return if f.params.len() == 1 {
                        self.hill_climb_i64(f, target, max_evals)
                    } else {
                        self.hill_climb_multi_i64(f, target, max_evals)
                    };
                }
                if all_f64_params && f64_ret {
                    return self.hill_climb_multi_f64(f, target, max_evals);
                }
            }
        }
        Ok(self.best_observed(name, target, max_evals))
    }

    /// Hill-climb an `@[adaptive] fn(i64) -> i64` toward `target`, accepting any
    /// improvement and halving the step on a stall. Direction-agnostic: returns
    /// the observed score closest to `target`. Each callback flows through
    /// [`Interp::call_fn`], so the provenance store accumulates as a side effect.
    pub(super) fn hill_climb_i64(&self, f: &FnDef, target: f64, max_evals: i64) -> Result<f64, Flow> {
        let eval_at = |x: i64| -> Result<f64, Flow> {
            let other = self.call_fn(f, vec![Value::Int(x)])?;
            match numeric_score(&other) {
                Some(s) => Ok(s),
                None => panic(format!(
                    "@[adaptive] fn `{}` must return a number, got {}",
                    f.name,
                    other.type_name()
                )),
            }
        };

        // Resume from the best prior input across runs when continuation is
        // enabled (`AXON_GOAL_CONTINUE`); otherwise start fresh at 0.
        let mut cur_input: i64 = if goal_continue_enabled() {
            read_best_input(&f.name, target).unwrap_or(0)
        } else {
            0
        };
        let initial = eval_at(cur_input)?;
        let mut best_score = initial;
        let mut best_dist = (initial - target).abs();
        let mut best_input = cur_input;
        let mut evals: i64 = 1;
        let unlimited = max_evals <= 0;
        // Early exit if the very first probe already hit target — no point
        // burning the rest of the budget on a perfect score (and downstream
        // tests / observability are cleaner when the trace doesn't include
        // redundant tail evals).
        if best_dist <= f64::EPSILON {
            return Ok(best_score);
        }
        // Coarse-then-fine: when the starting probe is small (e.g. 0), the
        // old `step = max(1, |x|/4)` formula locked us into a unit-step walk
        // and 50 evals never escaped the local neighborhood. Seed wide and
        // let the halving phase narrow toward the optimum.
        //
        // Two modes:
        //  - Fresh start (cur_input == 0 and no prior best on disk): seed
        //    `step ≈ max_evals * 4` so the first probes can leap across
        //    the whole plausible range and the halving cascade acts as a
        //    binary search toward the peak.
        //  - Continuation / nonzero start: assume the input is already in a
        //    good neighborhood, use the narrow `|x|/4` seed (with a floor
        //    of 1) so we fine-tune rather than overshoot — preserves the
        //    cross-run continuation semantics tested by `learn-goal.md`.
        let mut step: i64 = if cur_input == 0 {
            if unlimited { 4096 } else { std::cmp::max(16, max_evals.saturating_mul(4)) }
        } else {
            std::cmp::max(1, (cur_input.unsigned_abs() as i64) / 4)
        };

        while step >= 1 {
            if !unlimited && evals >= max_evals {
                break;
            }
            let mut improved = false;

            let up = cur_input.saturating_add(step);
            let up_score = eval_at(up)?;
            evals += 1;
            if (up_score - target).abs() < best_dist {
                best_dist = (up_score - target).abs();
                best_score = up_score;
                best_input = up;
                improved = true;
                // Hit target exactly — stop now so subsequent observability
                // (goal_history, goal_best_input) reflects a tight trace.
                if best_dist <= f64::EPSILON {
                    return Ok(best_score);
                }
            }
            if !unlimited && evals >= max_evals {
                break;
            }

            let dn = cur_input.saturating_sub(step);
            let dn_score = eval_at(dn)?;
            evals += 1;
            if (dn_score - target).abs() < best_dist {
                best_dist = (dn_score - target).abs();
                best_score = dn_score;
                best_input = dn;
                improved = true;
                if best_dist <= f64::EPSILON {
                    return Ok(best_score);
                }
            }

            if improved {
                cur_input = best_input;
            } else {
                step /= 2;
            }
        }
        Ok(best_score)
    }

    /// Coordinate-descent hill climb for an `@[adaptive] fn(i64, i64, ...) -> i64`.
    /// Cycles through each dim, halving-step searching that dim while holding
    /// the others fixed, then repeats the sweep until either no dim improves
    /// (its halving cascade bottomed out) or the budget is exhausted.
    /// Direction-agnostic: returns the observed score closest to `target`.
    /// Each callback flows through `Interp::call_fn`, so provenance (scores +
    /// full input tuples) accumulates as a side effect.
    pub(super) fn hill_climb_multi_i64(
        &self,
        f: &FnDef,
        target: f64,
        max_evals: i64,
    ) -> Result<f64, Flow> {
        self.hill_climb_multi_i64_from(f, target, max_evals, None)
    }

    pub(super) fn hill_climb_multi_i64_from(
        &self,
        f: &FnDef,
        target: f64,
        max_evals: i64,
        start: Option<Vec<i64>>,
    ) -> Result<f64, Flow> {
        let n_dims = f.params.len();
        let unlimited = max_evals <= 0;
        // `start = None` means fresh: cur = [0; n_dims]. `start = Some(v)`
        // (used by `goal_continue`) seeds at a known-good prior probe.
        // Length-mismatches fall back to zero so a stale store can't crash
        // the optimizer.
        let mut cur: Vec<i64> = match start {
            Some(v) if v.len() == n_dims => v,
            _ => vec![0; n_dims],
        };
        let eval_at = |xs: &[i64]| -> Result<f64, Flow> {
            let args = xs.iter().map(|&x| Value::Int(x)).collect();
            let other = self.call_fn(f, args)?;
            match numeric_score(&other) {
                Some(s) => Ok(s),
                None => panic(format!(
                    "@[adaptive] fn `{}` must return a number, got {}",
                    f.name,
                    other.type_name()
                )),
            }
        };

        let mut best_score = eval_at(&cur)?;
        let mut best_dist = (best_score - target).abs();
        let mut evals: i64 = 1;
        if best_dist <= f64::EPSILON {
            return Ok(best_score);
        }

        // Per-dim step seeding. Mirrors the 1-D formula: each dim gets a
        // wide first probe so it can sweep an interval before the halving
        // cascade narrows. We divide the budget across dims so a single
        // sweep stays inside the user's eval cap.
        let per_dim_budget = if unlimited { 0 } else { std::cmp::max(4, max_evals / (n_dims as i64).max(1)) };
        let seed_step: i64 = if unlimited { 4096 } else { std::cmp::max(16, per_dim_budget.saturating_mul(4)) };

        let mut steps: Vec<i64> = vec![seed_step; n_dims];
        // Per-dim sweep cap rotates dims fairly so no single dim monopolizes
        // the budget. Less critical here than in the f64 path (i64 halving
        // bottoms out in ~log2(seed_step) ≈ 12 steps, well inside a fair
        // share), but keeps the algorithm symmetric and forward-compatible.
        let per_dim_sweep_cap = if unlimited {
            i64::MAX
        } else {
            std::cmp::max(4, per_dim_budget)
        };

        // Sweep until no dim improves or budget hits.
        loop {
            let mut any_improvement = false;
            let cur_at_sweep_start = cur.clone();
            for d in 0..n_dims {
                if !unlimited && evals >= max_evals {
                    return Ok(best_score);
                }
                let dim_evals_at_start = evals;
                while steps[d] >= 1 {
                    if !unlimited && evals >= max_evals {
                        return Ok(best_score);
                    }
                    if !unlimited && evals - dim_evals_at_start >= per_dim_sweep_cap {
                        break;
                    }
                    let mut improved = false;

                    let mut probe = cur.clone();
                    probe[d] = cur[d].saturating_add(steps[d]);
                    let up_score = eval_at(&probe)?;
                    evals += 1;
                    if (up_score - target).abs() < best_dist {
                        best_dist = (up_score - target).abs();
                        best_score = up_score;
                        cur[d] = probe[d];
                        improved = true;
                        any_improvement = true;
                        if best_dist <= f64::EPSILON {
                            return Ok(best_score);
                        }
                    }
                    if !unlimited && evals >= max_evals {
                        return Ok(best_score);
                    }

                    let mut probe = cur.clone();
                    probe[d] = cur[d].saturating_sub(steps[d]);
                    let dn_score = eval_at(&probe)?;
                    evals += 1;
                    if (dn_score - target).abs() < best_dist {
                        best_dist = (dn_score - target).abs();
                        best_score = dn_score;
                        cur[d] = probe[d];
                        improved = true;
                        any_improvement = true;
                        if best_dist <= f64::EPSILON {
                            return Ok(best_score);
                        }
                    }

                    if !improved {
                        steps[d] /= 2;
                    }
                }
            }
            if !any_improvement {
                return Ok(best_score);
            }

            // Powell's-method joint-direction step. Same heuristic as the
            // f64 path: extrapolate along `delta = cur - cur_at_sweep_start`
            // with k = 1, 2, 4, … while the score keeps improving. On a
            // multi-arg objective where the per-dim sweeps each found
            // improvements in correlated directions, the joint step can
            // jump straight to a good neighbourhood that cyclic CD would
            // crawl toward across many sweeps.
            let mut delta = vec![0_i64; n_dims];
            let mut delta_nonzero = false;
            for d in 0..n_dims {
                delta[d] = cur[d].saturating_sub(cur_at_sweep_start[d]);
                if delta[d] != 0 {
                    delta_nonzero = true;
                }
            }
            if delta_nonzero {
                let mut k: i64 = 1;
                loop {
                    if !unlimited && evals >= max_evals {
                        return Ok(best_score);
                    }
                    let probe: Vec<i64> = (0..n_dims)
                        .map(|d| cur[d].saturating_add(k.saturating_mul(delta[d])))
                        .collect();
                    let probe_score = eval_at(&probe)?;
                    evals += 1;
                    let probe_dist = (probe_score - target).abs();
                    if probe_dist < best_dist {
                        best_dist = probe_dist;
                        best_score = probe_score;
                        cur = probe;
                        if best_dist <= f64::EPSILON {
                            return Ok(best_score);
                        }
                        k = k.saturating_mul(2);
                        if k > 1 << 30 {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }

            // Re-arm each dim's step for another sweep — a higher-dim move
            // may have opened a new gradient in dim 0.
            for s in steps.iter_mut() {
                *s = seed_step;
            }
        }
    }

    /// Coordinate-descent hill climb for an `@[adaptive] fn(f64, …) -> f64`.
    /// Mirror of `hill_climb_multi_i64` over the continuous domain: per-dim
    /// halving step starting wide, cycle through dims until a full sweep
    /// produces no improvement (each dim's step bottomed out below the
    /// resolution). The resolution floor is `1e-9` — tight enough for
    /// learned weights, well above f64 precision noise.
    pub(super) fn hill_climb_multi_f64(
        &self,
        f: &FnDef,
        target: f64,
        max_evals: i64,
    ) -> Result<f64, Flow> {
        self.hill_climb_multi_f64_from(f, target, max_evals, None)
    }

    pub(super) fn hill_climb_multi_f64_from(
        &self,
        f: &FnDef,
        target: f64,
        max_evals: i64,
        start: Option<Vec<f64>>,
    ) -> Result<f64, Flow> {
        let n_dims = f.params.len();
        let unlimited = max_evals <= 0;
        let mut cur: Vec<f64> = match start {
            Some(v) if v.len() == n_dims => v,
            _ => vec![0.0; n_dims],
        };
        let eval_at = |xs: &[f64]| -> Result<f64, Flow> {
            let args = xs.iter().map(|&x| Value::Float(x)).collect();
            let result = self.call_fn(f, args)?;
            match numeric_score(&result) {
                Some(s) => Ok(s),
                None => panic(format!(
                    "@[adaptive] fn `{}` must return a number, got {}",
                    f.name,
                    result.type_name()
                )),
            }
        };

        let mut best_score = eval_at(&cur)?;
        let mut best_dist = (best_score - target).abs();
        let mut evals: i64 = 1;
        if best_dist <= f64::EPSILON {
            return Ok(best_score);
        }

        // Seed step: wide enough to leap across a few-hundred-unit window
        // and let halving zero in. Mirrors the i64 path but scales the
        // budget partition by dim count.
        let per_dim_budget = if unlimited { 0 } else { std::cmp::max(4, max_evals / (n_dims as i64).max(1)) };
        let seed_step: f64 = if unlimited { 1024.0 } else { (per_dim_budget as f64 * 4.0).max(16.0) };
        let resolution: f64 = 1e-9;

        let mut steps: Vec<f64> = vec![seed_step; n_dims];

        // Cap each dim's evals per sweep so no single dim monopolizes the
        // budget. Without this, the inner halving cascade on dim 0 (37+
        // halvings × 2 evals = ~74 evals to fully bottom out at the f64
        // resolution floor) could eat a small total budget before dim 1
        // gets a single probe. `per_dim_sweep_cap` rotates dims fairly;
        // multiple sweeps still let any single dim fully converge, just
        // not in one greedy pass.
        let per_dim_sweep_cap = if unlimited {
            i64::MAX
        } else {
            std::cmp::max(4, per_dim_budget)
        };

        loop {
            let mut any_improvement = false;
            let cur_at_sweep_start = cur.clone();
            for d in 0..n_dims {
                if !unlimited && evals >= max_evals {
                    return Ok(best_score);
                }
                let dim_evals_at_start = evals;
                while steps[d] >= resolution {
                    if !unlimited && evals >= max_evals {
                        return Ok(best_score);
                    }
                    if !unlimited && evals - dim_evals_at_start >= per_dim_sweep_cap {
                        break;
                    }
                    let mut improved = false;

                    let mut probe = cur.clone();
                    probe[d] = cur[d] + steps[d];
                    let up_score = eval_at(&probe)?;
                    evals += 1;
                    if (up_score - target).abs() < best_dist {
                        best_dist = (up_score - target).abs();
                        best_score = up_score;
                        cur[d] = probe[d];
                        improved = true;
                        any_improvement = true;
                        if best_dist <= f64::EPSILON {
                            return Ok(best_score);
                        }
                    }
                    if !unlimited && evals >= max_evals {
                        return Ok(best_score);
                    }

                    let mut probe = cur.clone();
                    probe[d] = cur[d] - steps[d];
                    let dn_score = eval_at(&probe)?;
                    evals += 1;
                    if (dn_score - target).abs() < best_dist {
                        best_dist = (dn_score - target).abs();
                        best_score = dn_score;
                        cur[d] = probe[d];
                        improved = true;
                        any_improvement = true;
                        if best_dist <= f64::EPSILON {
                            return Ok(best_score);
                        }
                    }

                    if !improved {
                        steps[d] *= 0.5;
                    }
                }
            }
            if !any_improvement {
                return Ok(best_score);
            }

            // Powell's-method-style joint-direction step. After a sweep
            // that improved each dim individually, the net displacement
            // `delta = cur - cur_at_sweep_start` often points toward the
            // joint optimum for correlated dims (slope/intercept in a
            // linear-regression objective being the classic case). Probe
            // along that direction with k = 1, 2, 4, … while score keeps
            // improving — a geometric line search. Cuts the constant-
            // factor convergence rate of cyclic CD on quadratic objectives.
            let mut delta = vec![0.0_f64; n_dims];
            let mut delta_norm_sq = 0.0_f64;
            for d in 0..n_dims {
                delta[d] = cur[d] - cur_at_sweep_start[d];
                delta_norm_sq += delta[d] * delta[d];
            }
            if delta_norm_sq > resolution * resolution {
                let mut k: f64 = 1.0;
                loop {
                    if !unlimited && evals >= max_evals {
                        return Ok(best_score);
                    }
                    let probe: Vec<f64> = (0..n_dims)
                        .map(|d| cur[d] + k * delta[d])
                        .collect();
                    let probe_score = eval_at(&probe)?;
                    evals += 1;
                    let probe_dist = (probe_score - target).abs();
                    if probe_dist < best_dist {
                        best_dist = probe_dist;
                        best_score = probe_score;
                        cur = probe;
                        if best_dist <= f64::EPSILON {
                            return Ok(best_score);
                        }
                        k *= 2.0;
                        if k > 1e6 {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }

            for s in steps.iter_mut() {
                *s = seed_step;
            }
        }
    }

    /// Closest recorded score to `target` for `name` (or `target` if none).
    /// `max_evals > 0` keeps only the most recent N records.
    pub(super) fn best_observed(&self, name: &str, target: f64, max_evals: i64) -> f64 {
        if name.is_empty() {
            return target;
        }
        let store = self.provenance.borrow();
        let Some(scores) = store.get(name).filter(|s| !s.is_empty()) else {
            return target;
        };
        let slice: &[f64] = if max_evals > 0 && (max_evals as usize) < scores.len() {
            &scores[scores.len() - max_evals as usize..]
        } else {
            &scores[..]
        };
        let mut best = slice[0];
        let mut best_dist = (best - target).abs();
        for &s in &slice[1..] {
            let d = (s - target).abs();
            if d < best_dist {
                best = s;
                best_dist = d;
            }
        }
        best
    }

    /// Full `(input, score)` trace for an `@[adaptive]` fn. Walks the
    /// in-memory provenance store in call order. Each entry's input is the
    /// FIRST i64 dim — multi-arg fns expose all dims via `goal_best_inputs`
    /// instead. Empty when nothing was recorded or the fn had no i64 args.
    pub(super) fn history(&self, name: &str) -> Value {
        let mut out: Vec<Value> = Vec::new();
        if name.is_empty() {
            return Value::Array(out);
        }
        let scores_store = self.provenance.borrow();
        let inputs_store = self.provenance_inputs.borrow();
        if let (Some(scores), Some(inputs)) = (scores_store.get(name), inputs_store.get(name)) {
            let n = scores.len().min(inputs.len());
            out.reserve(n);
            for i in 0..n {
                if let Some(&first) = inputs[i].first() {
                    out.push(Value::Tuple(vec![
                        Value::Int(first),
                        Value::Float(scores[i]),
                    ]));
                }
            }
        }
        Value::Array(out)
    }

    /// Drop the recorded `(input, score)` history for an `@[adaptive]` fn so
    /// a follow-up experiment starts from a clean slate. Returns the number
    /// of records evicted (0 when `name` was absent or already empty).
    pub(super) fn clear(&self, name: &str) -> i64 {
        if name.is_empty() {
            return 0;
        }
        let evicted = self
            .provenance
            .borrow_mut()
            .remove(name)
            .map(|v| v.len() as i64)
            .unwrap_or(0);
        self.provenance_inputs.borrow_mut().remove(name);
        evicted
    }

    /// Index of the provenance entry whose score is closest to `target`,
    /// among entries with at least one recorded input (i64 or f64). None
    /// when nothing was recorded for `name`. The "has at least one input"
    /// check accepts either store so f64-only fns aren't filtered out.
    pub(super) fn best_input_index(&self, name: &str, target: f64) -> Option<usize> {
        if name.is_empty() {
            return None;
        }
        let scores_store = self.provenance.borrow();
        let inputs_store = self.provenance_inputs.borrow();
        let inputs_f64_store = self.provenance_inputs_f64.borrow();
        let scores = scores_store.get(name).filter(|s| !s.is_empty())?;
        let n = scores.len();
        let mut best_idx: Option<usize> = None;
        let mut best_dist = f64::INFINITY;
        for (i, &score) in scores.iter().enumerate().take(n) {
            let has_i64 = inputs_store
                .get(name)
                .and_then(|v| v.get(i))
                .is_some_and(|t| !t.is_empty());
            let has_f64 = inputs_f64_store
                .get(name)
                .and_then(|v| v.get(i))
                .is_some_and(|t| !t.is_empty());
            if has_i64 || has_f64 {
                let d = (score - target).abs();
                if d < best_dist {
                    best_dist = d;
                    best_idx = Some(i);
                }
            }
        }
        best_idx
    }

    /// Input that produced the score closest to `target` for an `@[adaptive]`
    /// fn. Returns the FIRST i64 dim — for multi-arg fns, use
    /// `goal_best_inputs` for the full tuple. Falls back to 0 when no inputs
    /// were recorded (the fn was never invoked, or it had no i64 args).
    pub(super) fn best_input(&self, name: &str, target: f64) -> i64 {
        self.best_input_index(name, target)
            .and_then(|i| self.provenance_inputs.borrow().get(name)?.get(i)?.first().copied())
            .unwrap_or(0)
    }

    /// Held-out evaluation of an `@[adaptive]` metric: snapshot provenance,
    /// call the fn on `input`, restore provenance, return the numeric score.
    /// The provenance must remain unmodified so that `goal_run` is unbiased.
    pub(super) fn goal_eval_holdout(&self, name: &str, input: i64) -> Result<f64, Flow> {
        let Some(f) = self.fns.get(name) else {
            return Err(Flow::Panic(format!(
                "goal_eval: `{name}` is not a defined function"
            )));
        };
        // Snapshot the three provenance stores for this fn.
        let snap_scores = self.provenance.borrow().get(name).cloned();
        let snap_inputs = self.provenance_inputs.borrow().get(name).cloned();
        let snap_inputs_f64 = self.provenance_inputs_f64.borrow().get(name).cloned();
        // Call the metric (this records into the stores).
        let result = self.call_fn(f, vec![Value::Int(input)]);
        // Restore — the held-out eval must not bias future goal_run.
        match snap_scores {
            Some(v) => { self.provenance.borrow_mut().insert(name.to_string(), v); }
            None => { self.provenance.borrow_mut().remove(name); }
        }
        match snap_inputs {
            Some(v) => { self.provenance_inputs.borrow_mut().insert(name.to_string(), v); }
            None => { self.provenance_inputs.borrow_mut().remove(name); }
        }
        match snap_inputs_f64 {
            Some(v) => { self.provenance_inputs_f64.borrow_mut().insert(name.to_string(), v); }
            None => { self.provenance_inputs_f64.borrow_mut().remove(name); }
        }
        let metric_result = result?;
        let score = match numeric_score(&metric_result) {
            Some(s) => s,
            None => return Err(Flow::Panic(format!(
                "goal_eval: metric `{name}` must return a number, got {}",
                metric_result.type_name()
            ))),
        };
        Ok(score)
    }

    /// Parse the `@[goal(metric:…, target:…, max_evals:…, holdout:… | test_set:[…])]`
    /// attr on `f`. Returns `(metric, target, max_evals, holdout_set)` where
    /// `holdout_set` is the list of held-out evaluation points (empty when none).
    /// `holdout: N` (single point) and `test_set: [a, b, c]` (a set) both feed it;
    /// `test_set` is the R5 multi-point form, the parser renders `[a,b,c]` as the
    /// flat string `"a,b,c"` which we split here.
    pub(super) fn goal_spec_of(&self, f: &FnDef) -> Option<GoalSpec> {
        let goal_attr = f.attrs.iter().find(|a| a.name == "goal")?;
        let mut metric: Option<String> = None;
        let mut target: Option<f64> = None;
        let mut max_evals: i64 = 50;
        let mut holdout_set: Vec<i64> = Vec::new();
        let mut strategy = GoalStrategy::HillClimb;
        let mut lo: i64 = 0;
        let mut hi: i64 = 100;
        for arg in &goal_attr.args {
            if let Some((k, v)) = arg.split_once(':') {
                let k = k.trim().to_lowercase();
                let v = v.trim();
                match k.as_str() {
                    "metric" => metric = Some(v.to_string()),
                    "target" => {
                        if let Ok(n) = v.parse::<i64>() {
                            target = Some(n as f64);
                        } else if let Ok(fv) = v.parse::<f64>() {
                            target = Some(fv);
                        }
                    }
                    "max_evals" => {
                        if let Ok(n) = v.parse::<i64>() { max_evals = n; }
                    }
                    "holdout" => {
                        if let Ok(n) = v.parse::<i64>() { holdout_set.push(n); }
                    }
                    // R5: a multi-point held-out test set `test_set: [a, b, c]`,
                    // rendered by the parser as `"a,b,c"`. Each parseable int is
                    // a held-out point; the goal is met only if the metric clears
                    // `target` on the WORST (min) of them (no overfitting to one).
                    "test_set" => {
                        for part in v.split(',') {
                            if let Ok(n) = part.trim().parse::<i64>() {
                                holdout_set.push(n);
                            }
                        }
                    }
                    // R5 (PRD L889-899): the search strategy. `hill_climb`
                    // (default), `random`, `multistart`, `tournament`, and
                    // `bayesian` are the closed set; an unknown name is E1505
                    // (validated by the checker). `random`/`multistart`/
                    // `tournament`/`bayesian` search the `[lo, hi)` box (default
                    // 0..100), overridable via `lo:`/`hi:`.
                    "strategy" => {
                        strategy = GoalStrategy::parse(v).unwrap_or(GoalStrategy::HillClimb);
                    }
                    "lo" => { if let Ok(n) = v.parse::<i64>() { lo = n; } }
                    "hi" => { if let Ok(n) = v.parse::<i64>() { hi = n; } }
                    _ => {}
                }
            }
        }
        metric.and_then(|m| {
            target.map(|t| GoalSpec {
                metric: m,
                target: t,
                max_evals,
                holdout_set,
                strategy,
                lo,
                hi,
            })
        })
    }

    /// All i64 input dims that produced the score closest to `target` for an
    /// `@[adaptive]` fn. Returns an empty slice when nothing was recorded.
    pub(super) fn best_inputs(&self, name: &str, target: f64) -> Value {
        let Some(idx) = self.best_input_index(name, target) else {
            return Value::Array(Vec::new());
        };
        let inputs_store = self.provenance_inputs.borrow();
        let dims = inputs_store
            .get(name)
            .and_then(|v| v.get(idx))
            .cloned()
            .unwrap_or_default();
        Value::Array(dims.into_iter().map(Value::Int).collect())
    }

    /// f64-flavored counterpart of `best_inputs`: returns the f64-prefix
    /// input tuple of the entry whose score is closest to `target`.
    pub(super) fn best_inputs_f64(&self, name: &str, target: f64) -> Value {
        let Some(idx) = self.best_input_index(name, target) else {
            return Value::Array(Vec::new());
        };
        let inputs_store = self.provenance_inputs_f64.borrow();
        let dims = inputs_store
            .get(name)
            .and_then(|v| v.get(idx))
            .cloned()
            .unwrap_or_default();
        Value::Array(dims.into_iter().map(Value::Float).collect())
    }

}
