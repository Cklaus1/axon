pub const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Axon Goal Approval Flow</title>
<style>
*{box-sizing:border-box;margin:0;padding:0}
body{font-family:system-ui,sans-serif;background:#0f1117;color:#c9d1d9;min-height:100vh;padding:1.5rem}
h1{font-size:1.4rem;color:#58a6ff;margin-bottom:1.5rem}
.pane{border:1px solid #30363d;border-radius:8px;margin-bottom:1.2rem;overflow:hidden}
.pane-header{display:flex;align-items:center;justify-content:space-between;padding:.6rem 1rem;background:#161b22;border-bottom:1px solid #30363d}
.pane-title{font-size:.95rem;font-weight:600;color:#e6edf3}
.pane-step{font-size:.75rem;color:#8b949e;background:#21262d;padding:.1rem .5rem;border-radius:12px}
.pane-body{padding:1rem}
textarea,.code-area{width:100%;background:#0d1117;color:#c9d1d9;border:1px solid #30363d;border-radius:6px;padding:.6rem .8rem;font-family:ui-monospace,monospace;font-size:.82rem;resize:vertical;outline:none}
textarea{min-height:160px}
.code-area{min-height:120px;white-space:pre-wrap;word-break:break-all;overflow:auto}
.btn{margin-top:.6rem;padding:.4rem 1rem;background:#238636;color:#fff;border:none;border-radius:6px;cursor:pointer;font-size:.85rem;font-weight:600}
.btn:hover{background:#2ea043}
.btn:disabled{background:#21262d;color:#484f58;cursor:default}
.status{margin-top:.5rem;font-size:.8rem}
.ok{color:#3fb950}
.err{color:#f85149}
.warn{color:#d29922}
.spinner{display:inline-block;width:12px;height:12px;border:2px solid #30363d;border-top-color:#58a6ff;border-radius:50%;animation:spin .7s linear infinite;margin-right:.4rem}
@keyframes spin{to{transform:rotate(360deg)}}
.nav-tab{display:inline-block;padding:.3rem .7rem;background:#21262d;color:#c9d1d9;border:1px solid #30363d;border-radius:6px;text-decoration:none;font-size:.8rem;font-weight:600;cursor:pointer}
.nav-tab:hover{background:#30363d;color:#e6edf3}
</style>
</head>
<body>
<h1>Axon Goal Approval Flow</h1>

<nav style="margin-bottom:1.2rem;display:flex;flex-wrap:wrap;gap:.4rem;align-items:center">
  <button onclick="document.getElementById('p1').scrollIntoView({behavior:'smooth'})" class="nav-tab">1 Intent</button>
  <button onclick="document.getElementById('p2').scrollIntoView({behavior:'smooth'})" class="nav-tab">2 Review</button>
  <button onclick="document.getElementById('p3').scrollIntoView({behavior:'smooth'})" class="nav-tab">3 Approve</button>
  <button onclick="document.getElementById('p4').scrollIntoView({behavior:'smooth'})" class="nav-tab">4 Improve</button>
  <button onclick="document.getElementById('p5').scrollIntoView({behavior:'smooth'})" class="nav-tab">5 Redteam</button>
  <button onclick="document.getElementById('p6').scrollIntoView({behavior:'smooth'})" class="nav-tab">6 Deploy</button>
  <button onclick="document.getElementById('p7').scrollIntoView({behavior:'smooth'})" class="nav-tab">7 Trace</button>
  <button onclick="showPane('safety')" class="nav-tab" style="background:#1a2a3a;border-color:#1f6feb">&#x1F6E1; Safety</button>
</nav>

<!-- Pane 1: Intent -->
<div class="pane" id="p1">
  <div class="pane-header">
    <span class="pane-title">Intent</span>
    <span class="pane-step">Step 1</span>
  </div>
  <div class="pane-body">
    <textarea id="intent-input" placeholder="Describe your goal in plain prose…
# Goal: optimize throughput
Constraints: budget ≤ $0.10
Budget: 50 calls"></textarea>
    <button class="btn" id="btn-compile" onclick="compileIntent()">Compile Intent</button>
    <div class="status" id="s1"></div>
    <div class="code-area" id="intent-out" style="display:none"></div>
  </div>
</div>

<!-- Pane 2: AST Review -->
<div class="pane" id="p2">
  <div class="pane-header">
    <span class="pane-title">AST Review</span>
    <span class="pane-step">Step 2</span>
  </div>
  <div class="pane-body">
    <div class="code-area" id="ax-content" style="min-height:80px;color:#8b949e">Compile intent first…</div>
    <button class="btn" id="btn-review" onclick="reviewAst()" disabled>Review AST</button>
    <div class="status" id="s2"></div>
    <div class="code-area" id="review-out" style="display:none"></div>
  </div>
</div>

<!-- Pane 3: Approve -->
<div class="pane" id="p3">
  <div class="pane-header">
    <span class="pane-title">Approve AST</span>
    <span class="pane-step">Step 3</span>
  </div>
  <div class="pane-body">
    <p style="font-size:.85rem;color:#8b949e;margin-bottom:.6rem">The typed AST is the legal artifact. Approval is required before deploy.</p>
    <button class="btn" id="btn-approve" onclick="approveAst()" disabled style="background:#1f6feb">Approve AST</button>
    <div class="status" id="s3"></div>
  </div>
</div>

<!-- Pane 4: Improve -->
<div class="pane" id="p4">
  <div class="pane-header">
    <span class="pane-title">Improve</span>
    <span class="pane-step">Step 4</span>
  </div>
  <div class="pane-body">
    <p style="font-size:.85rem;color:#8b949e;margin-bottom:.6rem">Run the goal optimizer — the system searches for the best variant and shows the score trajectory.</p>
    <button class="btn" id="btn-improve" onclick="runImprove()" disabled style="background:#1f6feb">Run Improve Cycle</button>
    <div class="status" id="s-improve"></div>
    <div class="code-area" id="improve-out" style="display:none"></div>
  </div>
</div>

<!-- Pane 5: Red Team -->
<div class="pane" id="p5">
  <div class="pane-header">
    <span class="pane-title">Red Team Check</span>
    <span class="pane-step">Step 5</span>
  </div>
  <div class="pane-body">
    <button class="btn" id="btn-redteam" onclick="runRedteam()" disabled style="background:#6e40c9">Run Redteam</button>
    <div class="status" id="s4"></div>
    <div class="code-area" id="redteam-out" style="display:none"></div>
  </div>
</div>

<!-- Pane 6: Deploy -->
<div class="pane" id="p6">
  <div class="pane-header">
    <span class="pane-title">Deploy</span>
    <span class="pane-step">Step 6</span>
  </div>
  <div class="pane-body">
    <label style="font-size:.82rem;color:#8b949e">Risk level:
      <select id="risk-sel" style="margin-left:.4rem;background:#161b22;color:#c9d1d9;border:1px solid #30363d;border-radius:4px;padding:.2rem .4rem">
        <option value="">derive from AST</option>
        <option value="low">low</option>
        <option value="medium">medium</option>
        <option value="high">high</option>
        <option value="critical">critical</option>
      </select>
    </label>
    <button class="btn" id="btn-deploy" onclick="runDeploy()" disabled>Deploy</button>
    <div class="status" id="s5"></div>
    <div class="code-area" id="deploy-out" style="display:none"></div>
  </div>
</div>

<!-- Pane 7: Trace -->
<div class="pane" id="p7">
  <div class="pane-header">
    <span class="pane-title">Trace</span>
    <span class="pane-step">Step 7</span>
  </div>
  <div class="pane-body">
    <button class="btn" id="btn-trace" onclick="showTrace()" style="background:#21262d;color:#c9d1d9;border:1px solid #30363d">Show Trace</button>
    <div class="status" id="s6"></div>
    <div class="code-area" id="trace-out" style="display:none"></div>
  </div>
</div>

<!-- Pane 8: Safety Dashboard (R26 attestation · R27 kill-switch · R28 audit ledger) -->
<div class="pane" id="pane-safety" style="display:none">
  <div class="pane-header">
    <span class="pane-title">&#x1F6E1; Safety Dashboard</span>
    <span class="pane-step">R26 &middot; R27 &middot; R28</span>
  </div>
  <div class="pane-body">

    <!-- Attestation (R26) -->
    <div style="margin-bottom:1rem;padding:.8rem;background:#0d1117;border:1px solid #30363d;border-radius:6px">
      <div style="display:flex;align-items:center;gap:.8rem;margin-bottom:.5rem">
        <span style="font-weight:600;font-size:.9rem">Attestation</span>
        <span id="attest-badge" style="font-size:1.3rem">&#x2014;</span>
        <span id="attest-status" style="font-size:.8rem;color:#8b949e">not checked</span>
      </div>
      <button class="btn" onclick="runAttest()">Attest Kernel</button>
      <div class="code-area" id="attest-out" style="display:none;margin-top:.5rem;min-height:60px"></div>
    </div>

    <!-- Kill-switch (R27) -->
    <div style="margin-bottom:1rem;padding:.8rem;background:#0d1117;border:1px solid #30363d;border-radius:6px">
      <div style="font-weight:600;font-size:.9rem;margin-bottom:.4rem">Kill-Switch (R27)</div>
      <div style="font-size:.82rem;color:#8b949e;margin-bottom:.6rem">Trip the corrigibility latch for a running job. Irreversible.</div>
      <input id="kill-run-id" placeholder="run_id (leave blank for current)" style="background:#0d1117;color:#c9d1d9;border:1px solid #30363d;border-radius:4px;padding:.3rem .6rem;font-size:.82rem;width:240px;outline:none">
      <button id="btn-kill" onclick="tripKill()" style="margin-left:.5rem;padding:.4rem 1rem;background:#b91c1c;color:#fff;border:none;border-radius:6px;cursor:pointer;font-size:.85rem;font-weight:600">&#x26A0; Kill Running Job</button>
      <div class="status" id="kill-status" style="margin-top:.4rem"></div>
    </div>

    <!-- Coalition bound (R27) -->
    <div style="margin-bottom:1rem;padding:.8rem;background:#0d1117;border:1px solid #30363d;border-radius:6px">
      <div style="font-weight:600;font-size:.9rem;margin-bottom:.4rem">Coalition Bound (R27)</div>
      <div id="coalition-display" style="font-size:.85rem;color:#8b949e">coalition: &#x2014; / 3 principals</div>
    </div>

    <!-- Audit Ledger (R28) -->
    <div id="safety-ledger" style="margin-bottom:1rem;padding:.8rem;background:#0d1117;border:1px solid #30363d;border-radius:6px">
      <div style="font-weight:600;font-size:.9rem;margin-bottom:.4rem">Audit Ledger &#x2014; last 10 entries (R28)</div>
      <div id="ledger-msg" style="font-size:.8rem;color:#8b949e;margin-bottom:.5rem"></div>
      <table id="ledger-table" style="width:100%;border-collapse:collapse;font-size:.8rem;display:none">
        <thead><tr style="color:#8b949e">
          <th style="text-align:left;padding:.3rem .5rem;border-bottom:1px solid #30363d">seq</th>
          <th style="text-align:left;padding:.3rem .5rem;border-bottom:1px solid #30363d">ts</th>
          <th style="text-align:left;padding:.3rem .5rem;border-bottom:1px solid #30363d">principal</th>
          <th style="text-align:left;padding:.3rem .5rem;border-bottom:1px solid #30363d">effect</th>
          <th style="text-align:left;padding:.3rem .5rem;border-bottom:1px solid #30363d">operation</th>
        </tr></thead>
        <tbody id="ledger-body"></tbody>
      </table>
      <button class="btn" onclick="refreshLedger()" style="background:#21262d;color:#c9d1d9;border:1px solid #30363d;margin-top:.5rem">Refresh Ledger</button>
    </div>

    <!-- Aggregate refresh -->
    <button class="btn" onclick="refreshSafetyStatus()" style="background:#1f6feb">&#x21BA; Refresh All</button>
    <div class="status" id="safety-status-msg" style="margin-top:.5rem"></div>
  </div>
</div>

<script>
let axContent = '';
let running = false;
// Track which steps have succeeded so we can restore the right button states
const done = {compiled: false, reviewed: false, approved: false, improved: false, redteamed: false};

const ALL_BTNS = ['btn-compile','btn-review','btn-approve','btn-improve','btn-redteam','btn-deploy','btn-trace'];

function lockAll() {
  running = true;
  ALL_BTNS.forEach(id => { document.getElementById(id).disabled = true; });
}

function unlockByState() {
  running = false;
  // btn-compile is always enabled
  document.getElementById('btn-compile').disabled = false;
  document.getElementById('btn-review').disabled = !done.compiled;
  document.getElementById('btn-approve').disabled = !done.reviewed;
  document.getElementById('btn-improve').disabled = !done.approved;
  document.getElementById('btn-redteam').disabled = !done.improved;
  document.getElementById('btn-deploy').disabled = !done.redteamed;
  document.getElementById('btn-trace').disabled = false;
}

function spin(id) {
  document.getElementById(id).innerHTML = '<span class="spinner"></span>running…';
}
function ok(id, msg) {
  document.getElementById(id).innerHTML = '<span class="ok">✓ ' + msg + '</span>';
}
function fail(id, msg) {
  document.getElementById(id).innerHTML = '<span class="err">✗ ' + msg + '</span>';
}
function show(id, txt) {
  const el = document.getElementById(id);
  el.style.display = '';
  el.textContent = typeof txt === 'object' ? JSON.stringify(txt, null, 2) : txt;
}

async function post(path, body) {
  const r = await fetch(path, {
    method: 'POST',
    headers: {'Content-Type': 'application/json'},
    body: JSON.stringify(body),
  });
  return r.json();
}

async function compileIntent() {
  if (running) return;
  const content = document.getElementById('intent-input').value.trim();
  if (!content) { fail('s1', 'enter intent first'); return; }
  lockAll(); spin('s1');
  try {
    const j = await post('/api/intent/compile', {content});
    if (j.error) { fail('s1', j.error); return; }
    const axText = j.ax_content || j.stdout || JSON.stringify(j, null, 2);
    axContent = axText;
    show('intent-out', JSON.stringify(j, null, 2));
    show('ax-content', axText);
    ok('s1', 'compiled');
    done.compiled = true;
  } catch(e) { fail('s1', e.message); }
  finally { unlockByState(); }
}

async function reviewAst() {
  if (running || !axContent) { fail('s2', 'compile intent first'); return; }
  lockAll(); spin('s2');
  try {
    const j = await post('/api/ast/review', {content: axContent});
    show('review-out', JSON.stringify(j, null, 2));
    // AUDIT T50 (P4-PROD-09). This gated on `j.error`, which the
    // `axon-ast-review/1` schema never emits — it reports `errors`, an ARRAY.
    // So a program that fails to type-check set done.reviewed = true and the
    // flow advanced to Approve. Verified against the running server: a review
    // of `let x: i64 = "not an int"` returns
    // `{"errors":["[E0102] type mismatch ..."]}` with no `error` field at all.
    const errs = Array.isArray(j.errors) ? j.errors : [];
    if (j.error || errs.length > 0) {
      fail('s2', j.error || (errs.length + ' error(s): ' + errs[0]));
      return;
    }
    ok('s2', 'reviewed');
    done.reviewed = true;
  } catch(e) { fail('s2', e.message); }
  finally { unlockByState(); }
}

async function approveAst() {
  if (running || !axContent) { fail('s3', 'review AST first'); return; }
  lockAll(); spin('s3');
  try {
    const j = await post('/api/ast/approve', {content: axContent});
    if (j.ok) {
      ok('s3', 'approved — AST signed');
      done.approved = true;
    } else {
      fail('s3', j.error || 'approval failed');
    }
  } catch(e) { fail('s3', e.message); }
  finally { unlockByState(); }
}

async function runImprove() {
  if (running || !axContent) { fail('s-improve', 'approve AST first'); return; }
  lockAll(); spin('s-improve');
  try {
    const j = await post('/api/goal/improve', {content: axContent});
    let summary = '';
    if (j.best_score !== undefined && j.best_score !== null) {
      summary += 'Best score: ' + j.best_score + '\n';
    }
    if (j.run_output) { summary += '\nRun output:\n' + j.run_output; }
    if (j.trajectory && j.trajectory.length > 0) {
      summary += '\nScore trajectory:\n';
      j.trajectory.forEach(t => {
        const arr = t.trend === 'improving' ? '↑' : t.trend === 'regressing' ? '↓' : '→';
        summary += '  ' + t.fn + ': ' + t.first + ' → ' + t.last + ' ' + arr + ' (' + t.evals + ' evals)\n';
      });
    }
    show('improve-out', summary || JSON.stringify(j, null, 2));
    if (j.ok !== false && !j.error) {
      const scoreLabel = (j.best_score !== undefined && j.best_score !== null) ? ' — best score: ' + j.best_score : '';
      ok('s-improve', 'optimization complete' + scoreLabel);
      done.improved = true;
    } else {
      fail('s-improve', j.error || 'optimizer failed');
    }
  } catch(e) { fail('s-improve', e.message); }
  finally { unlockByState(); }
}

async function runRedteam() {
  if (running || !axContent) { fail('s4', 'run improve first'); return; }
  lockAll(); spin('s4');
  try {
    const j = await post('/api/redteam', {content: axContent});
    const caught = j.caught === true;
    const reason = j.message || '';
    const display = (caught && reason ? '⚠ REDTEAM CAUGHT: ' + reason + '\n\n' : '') + JSON.stringify(j, null, 2);
    show('redteam-out', display);
    // AUDIT T50 (P4-PROD-09). `done.redteamed = true` used to sit OUTSIDE this
    // chain, so it was set even when the redteam CAUGHT something — unlocking
    // Deploy on exactly the run that was supposed to block it. That directly
    // falsified the documented Acid-Test-4 gating claim.
    if (caught) {
      fail('s4', 'REDTEAM CAUGHT — ' + (reason || 'adversarial issue detected'));
    } else if (j.ok !== false && !j.error) {
      ok('s4', 'redteam passed — no adversarial issues found');
      done.redteamed = true;
    } else {
      fail('s4', j.error || 'redteam check error');
    }
  } catch(e) { fail('s4', e.message); }
  finally { unlockByState(); }
}

async function runDeploy() {
  if (running || !axContent) { fail('s5', 'complete prior steps first'); return; }
  lockAll(); spin('s5');
  try {
    const risk = document.getElementById('risk-sel').value;
    const body = {content: axContent};
    if (risk) body.risk = risk;
    const j = await post('/api/deploy', body);
    show('deploy-out', JSON.stringify(j, null, 2));
    // AUDIT T50 (P4-PROD-09). `j.ok !== false && !j.error` treated any response
    // without an `error` field as a success, and `axon-deploy/1` reports
    // `status` ("deployed" / "blocked" / "blocked_approval"), never `error`. A
    // deploy refused by a gate therefore rendered as "deployed".
    const deployed = j.deployed === true || j.status === 'deployed';
    if (deployed) {
      ok('s5', 'deployed' + (j.approved === false ? ' (NOT approved)' : ''));
    } else {
      const why = j.gate ? ('gate: ' + j.gate) : (j.failed_reason || j.reason || j.status || j.error);
      fail('s5', why || 'deploy blocked by gate');
    }
  } catch(e) { fail('s5', e.message); }
  finally { unlockByState(); }
}

async function showTrace() {
  if (running) return;
  lockAll(); spin('s6');
  try {
    const r = await fetch('/api/trace');
    const j = await r.json();
    show('trace-out', JSON.stringify(j, null, 2));
    ok('s6', 'trace loaded');
  } catch(e) { fail('s6', e.message); }
  finally { unlockByState(); }
}

// ── Safety Dashboard (Pane 8) ─────────────────────────────────────────────

function showPane(name) {
  const el = document.getElementById('pane-' + name);
  if (!el) return;
  const hidden = el.style.display === 'none' || el.style.display === '';
  if (hidden) {
    el.style.display = 'block';
    refreshSafetyStatus();
  }
  el.scrollIntoView({behavior: 'smooth', block: 'start'});
}

async function runAttest() {
  document.getElementById('attest-status').textContent = 'checking…';
  document.getElementById('attest-badge').textContent = '—';
  try {
    const r = await fetch('/api/safety/attest', {method: 'POST', headers: {'Content-Type': 'application/json'}, body: '{}'});
    const j = await r.json();
    const passed = j.attested === true || j.ok === true;
    document.getElementById('attest-badge').textContent = passed ? '🔒' : '🔴';
    document.getElementById('attest-status').textContent = passed
      ? 'attested (' + (j.mode || 'live') + ')'
      : 'failed — ' + (j.error || 'attestation rejected');
    const out = document.getElementById('attest-out');
    out.style.display = '';
    out.textContent = JSON.stringify(j, null, 2);
  } catch(e) {
    document.getElementById('attest-badge').textContent = '🔴';
    document.getElementById('attest-status').textContent = 'error: ' + e.message;
  }
}

async function tripKill() {
  const runId = document.getElementById('kill-run-id').value.trim() || 'current';
  const ks = document.getElementById('kill-status');
  ks.innerHTML = '<span class="warn">sending kill signal…</span>';
  try {
    const r = await fetch('/api/safety/kill', {
      method: 'POST',
      headers: {'Content-Type': 'application/json'},
      body: JSON.stringify({run_id: runId}),
    });
    const j = await r.json();
    if (j.ok) {
      ks.innerHTML = '<span class="ok">✓ Kill latch tripped for run_id: ' + j.run_id + '</span>';
    } else {
      ks.innerHTML = '<span class="err">✗ ' + (j.error || 'kill failed') + '</span>';
    }
  } catch(e) {
    ks.innerHTML = '<span class="err">✗ ' + e.message + '</span>';
  }
}

async function refreshLedger() {
  const msg = document.getElementById('ledger-msg');
  msg.textContent = 'loading…';
  try {
    const r = await fetch('/api/safety/ledger');
    const j = await r.json();
    if (!j.ok) {
      msg.textContent = j.reason || j.error || 'R28 not available';
      document.getElementById('ledger-table').style.display = 'none';
      return;
    }
    const entries = j.entries || [];
    msg.textContent = entries.length + ' entries from ' + (j.ledger_path || 'ledger');
    const tbody = document.getElementById('ledger-body');
    tbody.innerHTML = '';
    entries.forEach(e => {
      const tr = document.createElement('tr');
      tr.innerHTML = [e.seq, e.ts, e.principal, e.effect, e.operation]
        .map(v => '<td style="padding:.3rem .5rem;border-bottom:1px solid #21262d">' + (v !== undefined && v !== null ? String(v) : '—') + '</td>')
        .join('');
      tbody.appendChild(tr);
    });
    document.getElementById('ledger-table').style.display = entries.length ? '' : 'none';
  } catch(e) {
    document.getElementById('ledger-msg').textContent = 'error: ' + e.message;
  }
}

async function refreshSafetyStatus() {
  const msg = document.getElementById('safety-status-msg');
  msg.innerHTML = '<span class="spinner"></span>checking…';
  try {
    const r = await fetch('/api/safety/status');
    const j = await r.json();
    if (j.ok) {
      const parts = [
        'attested: ' + (j.attested ? '✓' : '✗'),
        'killable: ' + (j.killable ? '✓' : '✗'),
        'ledger: ' + (j.ledger_ok ? '✓' : 'R28 pending'),
        'coalition: ' + (j.coalition_ok ? '✓' : '✗'),
      ];
      msg.innerHTML = '<span class="ok">' + parts.join(' &middot; ') + '</span>';
      if (j.coalition_principals !== undefined) {
        document.getElementById('coalition-display').textContent =
          'coalition: ' + j.coalition_principals + ' / ' + (j.coalition_max || 3) + ' principals';
      }
    } else {
      msg.innerHTML = '<span class="err">status check failed</span>';
    }
  } catch(e) {
    msg.innerHTML = '<span class="err">' + e.message + '</span>';
  }
}
</script>
</body>
</html>
"#;
