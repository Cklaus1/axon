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
</style>
</head>
<body>
<h1>Axon Goal Approval Flow</h1>

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

<!-- Pane 4: Red Team -->
<div class="pane" id="p4">
  <div class="pane-header">
    <span class="pane-title">Red Team Check</span>
    <span class="pane-step">Step 4</span>
  </div>
  <div class="pane-body">
    <button class="btn" id="btn-redteam" onclick="runRedteam()" disabled style="background:#6e40c9">Run Redteam</button>
    <div class="status" id="s4"></div>
    <div class="code-area" id="redteam-out" style="display:none"></div>
  </div>
</div>

<!-- Pane 5: Deploy -->
<div class="pane" id="p5">
  <div class="pane-header">
    <span class="pane-title">Deploy</span>
    <span class="pane-step">Step 5</span>
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

<!-- Pane 6: Trace -->
<div class="pane" id="p6">
  <div class="pane-header">
    <span class="pane-title">Trace</span>
    <span class="pane-step">Step 6</span>
  </div>
  <div class="pane-body">
    <button class="btn" id="btn-trace" onclick="showTrace()" style="background:#21262d;color:#c9d1d9;border:1px solid #30363d">Show Trace</button>
    <div class="status" id="s6"></div>
    <div class="code-area" id="trace-out" style="display:none"></div>
  </div>
</div>

<script>
let axContent = '';
let running = false;
// Track which steps have succeeded so we can restore the right button states
const done = {compiled: false, reviewed: false, approved: false, redteamed: false};

const ALL_BTNS = ['btn-compile','btn-review','btn-approve','btn-redteam','btn-deploy','btn-trace'];

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
  document.getElementById('btn-redteam').disabled = !done.approved;
  document.getElementById('btn-deploy').disabled = !done.redteamed;
  // trace is always available
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
    if (j.error) { fail('s2', j.error); return; }
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

async function runRedteam() {
  if (running || !axContent) { fail('s4', 'approve AST first'); return; }
  lockAll(); spin('s4');
  try {
    const j = await post('/api/redteam', {content: axContent});
    show('redteam-out', JSON.stringify(j, null, 2));
    const passed = j.ok !== false && !j.error;
    if (passed) { ok('s4', 'redteam passed'); }
    else { fail('s4', 'redteam flagged issues — review before deploying'); }
    done.redteamed = true;
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
    const deployed = j.deployed === true || (j.ok !== false && !j.error);
    if (deployed) { ok('s5', 'deployed'); }
    else { fail('s5', j.failed_reason || j.error || 'deploy blocked by gate'); }
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
</script>
</body>
</html>
"#;
