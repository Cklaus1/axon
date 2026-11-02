//! IG3 - the generator seam: where a candidate program comes from. The ONLY
//! impure dependency of the check->repair loop ([`crate::genloop`]).
//!
//! - `cmd:<path>` runs the operator's command as a process-group job
//!   ([`axon_os::run_bounded_job`]): the prompt on stdin, the candidate on
//!   stdout, the whole group killed at the deadline (`AXON_INTENT_TIMEOUT_MS`,
//!   default 60 s). A termination signal to `axon-intent` reaches the job
//!   (`main` forwards it, KG5).
//! - `ai:<model>` calls `axon-ai` directly (feature `ai`), under the same
//!   deadline.
//!
//! A generator only proposes text. Nothing it returns is trusted: the loop
//! type-checks, gates and runs every candidate before it can reach approval.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// The deadline for one generation call, from `AXON_INTENT_TIMEOUT_MS`.
pub const TIMEOUT_ENV: &str = "AXON_INTENT_TIMEOUT_MS";
/// Default deadline for one generation call.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
/// The most candidate text a `cmd:` generator may print (bytes).
pub const MAX_CANDIDATE_BYTES: usize = 256 * 1024;
/// A `cmd:` generator reports the tokens a call used with a stderr line
/// `axon-intent-tokens: <n>` (the last such line counts). Without it the call
/// reported none, and the per-task token budget cannot be held against it.
pub const TOKENS_LINE_PREFIX: &str = "axon-intent-tokens: ";

/// The tokens a `cmd:` generator reported on stderr (see
/// [`TOKENS_LINE_PREFIX`]).
fn reported_tokens(stderr: &str) -> Option<u64> {
    stderr
        .lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix(TOKENS_LINE_PREFIX))
        .and_then(|n| n.trim().parse::<u64>().ok())
}

/// One generator answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generation {
    /// The candidate text, verbatim (fences are the loop's to strip).
    pub text: String,
    /// Tokens the generator reported for this call, if it reports any.
    pub tokens: Option<u64>,
}

/// Why a generation call produced no candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenFailure {
    /// The deadline passed; a `cmd:` job's whole process group was killed.
    TimedOut { after_ms: u64 },
    /// The generator ran and failed (non-zero exit, killed by a signal, an
    /// oversized answer, a model error).
    Failed { reason: String },
    /// The generator could not be started.
    Unavailable { reason: String },
}

impl std::fmt::Display for GenFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GenFailure::TimedOut { after_ms } => {
                write!(f, "the generator timed out after {after_ms} ms")
            }
            GenFailure::Failed { reason } => write!(f, "the generator failed: {reason}"),
            GenFailure::Unavailable { reason } => {
                write!(f, "the generator is unavailable: {reason}")
            }
        }
    }
}

/// A source of candidate programs (IG3).
pub trait Generator {
    /// A stable identifier recorded in every transcript (`cmd:<path>`,
    /// `ai:<model>`).
    fn id(&self) -> String;
    /// One candidate for `prompt`.
    fn generate(&self, prompt: &str) -> Result<Generation, GenFailure>;
}

/// The deadline from `AXON_INTENT_TIMEOUT_MS` (milliseconds, > 0), else
/// [`DEFAULT_TIMEOUT`].
pub fn timeout_from_env() -> Duration {
    std::env::var(TIMEOUT_ENV)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|&ms| ms > 0)
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_TIMEOUT)
}

/// Parse a `--generator` value: `cmd:<path>` or `ai:<model>`.
pub fn parse_spec(spec: &str, timeout: Duration) -> Result<Box<dyn Generator>, String> {
    if let Some(path) = spec.strip_prefix("cmd:") {
        if path.is_empty() {
            return Err("`--generator cmd:` needs a path".into());
        }
        return Ok(Box::new(CmdGenerator::new(PathBuf::from(path), timeout)));
    }
    if let Some(model) = spec.strip_prefix("ai:") {
        if model.is_empty() {
            return Err("`--generator ai:` needs a model name".into());
        }
        return ai_generator(model, timeout);
    }
    Err(format!(
        "unknown generator `{spec}` (expected `cmd:<path>` or `ai:<model>`)"
    ))
}

#[cfg(feature = "ai")]
fn ai_generator(model: &str, timeout: Duration) -> Result<Box<dyn Generator>, String> {
    Ok(Box::new(AiGenerator {
        model: model.to_string(),
        timeout,
    }))
}

#[cfg(not(feature = "ai"))]
fn ai_generator(_model: &str, _timeout: Duration) -> Result<Box<dyn Generator>, String> {
    Err("`--generator ai:` needs axon-intent built with `--features ai`".into())
}

/// `cmd:<path>`: the operator's command, prompt on stdin, candidate on stdout.
pub struct CmdGenerator {
    path: PathBuf,
    timeout: Duration,
}

impl CmdGenerator {
    pub fn new(path: PathBuf, timeout: Duration) -> Self {
        CmdGenerator { path, timeout }
    }
}

impl Generator for CmdGenerator {
    fn id(&self) -> String {
        format!("cmd:{}", self.path.display())
    }

    fn generate(&self, prompt: &str) -> Result<Generation, GenFailure> {
        let mut cmd = Command::new(&self.path);
        let run = axon_os::run_bounded_job(
            &mut cmd,
            self.timeout,
            Some(prompt.as_bytes().to_vec()),
            MAX_CANDIDATE_BYTES,
        )
        .map_err(|e| GenFailure::Unavailable {
            reason: format!("cannot run {}: {e}", self.path.display()),
        })?;
        if run.timed_out {
            return Err(GenFailure::TimedOut {
                after_ms: self.timeout.as_millis() as u64,
            });
        }
        let tail = run
            .stderr
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_string();
        match run.code {
            Some(0) if run.stdout_omitted > 0 => Err(GenFailure::Failed {
                reason: format!("its answer exceeds {MAX_CANDIDATE_BYTES} bytes"),
            }),
            Some(0) => Ok(Generation {
                text: run.stdout,
                tokens: reported_tokens(&run.stderr),
            }),
            Some(code) => Err(GenFailure::Failed {
                reason: format!("exit {code}: {tail}"),
            }),
            None => Err(GenFailure::Failed {
                reason: format!("killed by signal {:?}", run.killed_by_signal),
            }),
        }
    }
}

/// `ai:<model>`: one `axon-ai` completion, bounded by the deadline.
#[cfg(feature = "ai")]
pub struct AiGenerator {
    model: String,
    timeout: Duration,
}

#[cfg(feature = "ai")]
impl Generator for AiGenerator {
    fn id(&self) -> String {
        format!("ai:{}", self.model)
    }

    fn generate(&self, prompt: &str) -> Result<Generation, GenFailure> {
        let (tx, rx) = std::sync::mpsc::channel();
        let (prompt, model) = (prompt.to_string(), self.model.clone());
        // A call past the deadline is abandoned, not joined: its thread ends
        // when axon-ai's own HTTP timeout fires.
        std::thread::spawn(move || {
            let _ = tx.send(axon_ai::complete_with_model_usage(&prompt, &model));
        });
        match rx.recv_timeout(self.timeout) {
            Ok(Ok((text, tokens))) => Ok(Generation {
                text,
                tokens: u64::try_from(tokens).ok(),
            }),
            Ok(Err(e)) => Err(GenFailure::Failed { reason: e.message }),
            Err(_) => Err(GenFailure::TimedOut {
                after_ms: self.timeout.as_millis() as u64,
            }),
        }
    }
}

/// Replays a fixed list of candidates, one per call (tests and audits; no
/// process, no network). Past the end it fails.
#[derive(Default)]
pub struct ScriptedGenerator {
    candidates: Vec<String>,
    tokens_per_call: Option<u64>,
    next: std::cell::Cell<usize>,
    prompts: std::cell::RefCell<Vec<String>>,
}

impl ScriptedGenerator {
    pub fn new<S: Into<String>>(candidates: impl IntoIterator<Item = S>) -> Self {
        ScriptedGenerator {
            candidates: candidates.into_iter().map(Into::into).collect(),
            ..Default::default()
        }
    }

    /// Report `n` tokens for every call (as a model generator would).
    pub fn with_tokens(mut self, n: u64) -> Self {
        self.tokens_per_call = Some(n);
        self
    }

    /// Every prompt received, in order.
    pub fn prompts(&self) -> Vec<String> {
        self.prompts.borrow().clone()
    }
}

impl Generator for ScriptedGenerator {
    fn id(&self) -> String {
        "scripted".into()
    }

    fn generate(&self, prompt: &str) -> Result<Generation, GenFailure> {
        self.prompts.borrow_mut().push(prompt.to_string());
        let i = self.next.get();
        self.next.set(i + 1);
        self.candidates
            .get(i)
            .map(|c| Generation {
                text: c.clone(),
                tokens: self.tokens_per_call,
            })
            .ok_or_else(|| GenFailure::Failed {
                reason: format!("the script has {} candidates", self.candidates.len()),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(name: &str, body: &str) -> (PathBuf, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("axon-intent-gen-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gen.sh");
        // The bytes reach `gen.sh` only through a child `cat`, so this process
        // never holds a write fd on it. An in-process write (even to a temp
        // name renamed into place: same inode) can be inherited by a sibling
        // test thread's forked child until its exec, and the exec of
        // `gen.sh` then fails ETXTBSY ("Text file busy").
        let mut child = std::process::Command::new("sh")
            .args(["-c", "cat > \"$0\""])
            .arg(&path)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        {
            use std::io::Write;
            let mut stdin = child.stdin.take().unwrap();
            stdin
                .write_all(body.replace("$DIR", &dir.display().to_string()).as_bytes())
                .unwrap();
        }
        assert!(child.wait().unwrap().success(), "write {}", path.display());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        (dir, path)
    }

    fn alive(pid: i32) -> bool {
        std::fs::read(format!("/proc/{pid}/cmdline"))
            .map(|c| !c.is_empty())
            .unwrap_or(false)
    }

    #[test]
    fn a_cmd_generator_reads_the_prompt_on_stdin_and_answers_on_stdout() {
        let (dir, path) = script(
            "echo",
            "#!/bin/sh\nread first\nprintf 'got:%s\\n' \"$first\"\n",
        );
        let g = CmdGenerator::new(path.clone(), Duration::from_secs(20));
        let out = g.generate("hello prompt\nsecond line\n").unwrap();
        assert_eq!(out.text, "got:hello prompt\n");
        assert_eq!(g.id(), format!("cmd:{}", path.display()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failing_cmd_generator_is_a_failure_not_a_candidate() {
        let (dir, path) = script(
            "fail",
            "#!/bin/sh\necho partial\necho 'no key' >&2\nexit 3\n",
        );
        let g = CmdGenerator::new(path, Duration::from_secs(20));
        match g.generate("p") {
            Err(GenFailure::Failed { reason }) => {
                assert!(
                    reason.contains("exit 3") && reason.contains("no key"),
                    "{reason}"
                )
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_hanging_cmd_generator_is_killed_with_its_group_at_the_deadline() {
        // ATTACK: a hanging generator outlives its deadline
        // The KG2 fixture shape: the script records its pid and a background
        // `sleep`'s, then waits on it; both must be gone after the deadline.
        let (dir, path) = script(
            "hang",
            "#!/bin/sh\necho $$ > $DIR/gen.pid\nsleep 4733 &\necho $! > $DIR/gc.pid\nwait\n",
        );
        let g = CmdGenerator::new(path, Duration::from_millis(1500));
        let start = std::time::Instant::now();
        let r = g.generate("p");
        let pid = |f: &str| -> i32 {
            std::fs::read_to_string(dir.join(f))
                .unwrap()
                .trim()
                .parse()
                .unwrap()
        };
        let (gen_pid, gc_pid) = (pid("gen.pid"), pid("gc.pid"));
        let gone = (0..100).any(|_| {
            if !alive(gen_pid) && !alive(gc_pid) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
            false
        });
        assert!(
            matches!(r, Err(GenFailure::TimedOut { after_ms: 1500 }))
                && gone
                && start.elapsed() < Duration::from_secs(15),
            "ATTACK: a hanging generator outlives its deadline: {r:?}, gone={gone}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn generator_specs_parse() {
        assert!(parse_spec("cmd:/bin/true", DEFAULT_TIMEOUT).is_ok());
        assert!(parse_spec("cmd:", DEFAULT_TIMEOUT).is_err());
        assert!(parse_spec("ai:", DEFAULT_TIMEOUT).is_err());
        assert!(parse_spec("gpt", DEFAULT_TIMEOUT)
            .err()
            .unwrap()
            .contains("cmd:<path>"));
    }

    #[test]
    fn a_scripted_generator_replays_then_fails() {
        let g = ScriptedGenerator::new(["a", "b"]);
        assert_eq!(g.generate("p1").unwrap().text, "a");
        assert_eq!(g.generate("p2").unwrap().text, "b");
        assert!(g.generate("p3").is_err());
        assert_eq!(g.prompts(), vec!["p1", "p2", "p3"]);
    }
}
