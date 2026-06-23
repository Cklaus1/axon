/// Minimal web dashboard for axon-signal analytics.
///
/// Serves a single-page HTML/JS dashboard at http://localhost:<port>/ that
/// renders score trends, weekly reports, rework hotspots, and loop opportunities.
/// No external framework — vanilla JS fetches JSON from REST endpoints
/// which proxy the same analytics the CLI provides.
///
/// Endpoints:
///   GET  /            → HTML dashboard
///   GET  /api/score   → SessionScore[]
///   GET  /api/weekly  → WeeklyReport
///   GET  /api/rework  → ReworkHotspot[]
///   GET  /api/trends  → TrendsReport
///   GET  /api/loops      → loop candidate[]
///   GET  /api/benchmark  → BenchmarkReport
///   GET  /api/ab         → AbReport
use std::path::Path;

use anyhow::Result;
use tiny_http::{Header, Response, Server};

use axon_ledger::store::Store;

use crate::ab::compute_ab_report;
use crate::benchmark::compute_benchmark;
use crate::rework::find_rework_hotspots;
use crate::score::score_sessions;
use crate::trends::compute_trends;
use crate::weekly::generate_weekly;

pub fn run_dashboard(ledger_dir: &Path, port: u16) -> Result<()> {
    let addr = format!("0.0.0.0:{port}");
    let server = Server::http(&addr).map_err(|e| anyhow::anyhow!("Cannot bind {addr}: {e}"))?;
    eprintln!("[axon-signal] dashboard listening on http://localhost:{port}/");

    for request in server.incoming_requests() {
        let url = request.url().to_string();
        let method = request.method().to_string();

        if method != "GET" {
            let _ =
                request.respond(Response::from_string("Method Not Allowed").with_status_code(405));
            continue;
        }

        let response = match url.as_str() {
            "/" => Response::from_string(DASHBOARD_HTML).with_header(
                "Content-Type: text/html; charset=utf-8"
                    .parse::<Header>()
                    .unwrap(),
            ),
            "/api/score" => json_response(ledger_dir, |store| {
                score_sessions(&store).map(|s| serde_json::to_value(s).unwrap())
            }),
            "/api/weekly" => json_response(ledger_dir, |store| {
                let now = now_ms();
                let from = now.saturating_sub(7 * 86_400_000);
                generate_weekly(&store, from, now, 14).map(|r| serde_json::to_value(r).unwrap())
            }),
            "/api/rework" => json_response(ledger_dir, |store| {
                find_rework_hotspots(&store, 14).map(|h| serde_json::to_value(h).unwrap())
            }),
            "/api/trends" => json_response(ledger_dir, |store| {
                compute_trends(&store, 8).map(|r| serde_json::to_value(r).unwrap())
            }),
            "/api/loops" => json_response(ledger_dir, |store| {
                score_sessions(&store).map(|scores| {
                    let candidates: Vec<_> = scores
                        .iter()
                        .filter(|s| s.turns >= 40)
                        .map(|s| {
                            serde_json::json!({
                                "session_id": s.session_id,
                                "engineer": s.engineer,
                                "goal": s.goal,
                                "turns": s.turns,
                                "commits_linked": s.commits_linked,
                                "score": s.score,
                            })
                        })
                        .collect();
                    serde_json::to_value(candidates).unwrap()
                })
            }),
            "/api/benchmark" => json_response(ledger_dir, |store| {
                compute_benchmark(&store, None)
                    .map(|r| serde_json::to_value(r).unwrap_or(serde_json::json!({})))
            }),
            "/api/ab" => {
                let body = match Store::open(ledger_dir)
                    .map_err(anyhow::Error::from)
                    .and_then(|store| compute_ab_report(ledger_dir, &store))
                    .map(|r| serde_json::to_value(r).unwrap_or(serde_json::json!({})))
                {
                    Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_else(|_| "{}".to_string()),
                    Err(e) => format!(r#"{{"error":"{}"}}"#, e),
                };
                Response::from_string(body)
                    .with_header("Content-Type: application/json".parse::<Header>().unwrap())
                    .with_header("Access-Control-Allow-Origin: *".parse::<Header>().unwrap())
            }
            _ => Response::from_string(r#"{"error":"not found"}"#)
                .with_status_code(404)
                .with_header("Content-Type: application/json".parse::<Header>().unwrap()),
        };

        let _ = request.respond(response);
    }
    Ok(())
}

fn json_response<F>(ledger_dir: &Path, f: F) -> tiny_http::Response<std::io::Cursor<Vec<u8>>>
where
    F: FnOnce(Store) -> anyhow::Result<serde_json::Value>,
{
    let body = match Store::open(ledger_dir)
        .map_err(anyhow::Error::from)
        .and_then(f)
    {
        Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_else(|_| "{}".to_string()),
        Err(e) => format!(r#"{{"error":"{}"}}"#, e),
    };
    Response::from_string(body)
        .with_header("Content-Type: application/json".parse::<Header>().unwrap())
        .with_header("Access-Control-Allow-Origin: *".parse::<Header>().unwrap())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ──────────────────────────────────────────────
// Embedded dashboard HTML
// ──────────────────────────────────────────────
const DASHBOARD_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>axon-signal dashboard</title>
<style>
  :root { --bg:#0f1117; --card:#1a1d27; --accent:#7c6af7; --text:#e2e8f0; --muted:#64748b; --green:#22c55e; --red:#ef4444; --yellow:#f59e0b; }
  * { box-sizing:border-box; margin:0; padding:0; }
  body { background:var(--bg); color:var(--text); font-family:'SF Mono',monospace; padding:24px; }
  h1 { font-size:1.4rem; color:var(--accent); margin-bottom:24px; }
  h2 { font-size:1rem; color:var(--muted); text-transform:uppercase; letter-spacing:.08em; margin-bottom:12px; }
  .grid { display:grid; grid-template-columns:repeat(auto-fill,minmax(340px,1fr)); gap:16px; margin-bottom:24px; }
  .card { background:var(--card); border-radius:8px; padding:20px; }
  .stat { font-size:2.2rem; font-weight:700; color:var(--accent); }
  .label { font-size:.75rem; color:var(--muted); margin-top:4px; }
  table { width:100%; border-collapse:collapse; font-size:.8rem; }
  th { text-align:left; color:var(--muted); padding:6px 8px; border-bottom:1px solid #2a2d3a; }
  td { padding:6px 8px; border-bottom:1px solid #1e2130; }
  .badge { display:inline-block; padding:2px 8px; border-radius:4px; font-size:.7rem; }
  .badge-green { background:#166534; color:#bbf7d0; }
  .badge-yellow { background:#713f12; color:#fef08a; }
  .badge-red { background:#7f1d1d; color:#fecaca; }
  .bar { display:inline-block; background:var(--accent); height:8px; border-radius:2px; opacity:.7; }
  .trend-up { color:var(--green); }
  .trend-down { color:var(--red); }
  .trend-stable { color:var(--muted); }
  nav { display:flex; gap:12px; margin-bottom:20px; flex-wrap:wrap; }
  nav button { background:var(--card); color:var(--text); border:1px solid #2a2d3a; border-radius:6px; padding:6px 14px; cursor:pointer; font-family:inherit; font-size:.85rem; }
  nav button.active { background:var(--accent); border-color:var(--accent); color:#fff; }
  .pane { display:none; } .pane.active { display:block; }
  .warn { background:#422006; color:#fde68a; padding:12px 16px; border-radius:6px; margin:8px 0; font-size:.85rem; }
  .eng-row { margin-bottom:16px; }
  .eng-name { font-weight:600; margin-bottom:4px; }
  .week-bars { display:flex; gap:6px; align-items:flex-end; height:40px; margin:6px 0; }
  .week-bar { display:flex; flex-direction:column; align-items:center; gap:2px; }
  .week-bar-fill { background:var(--accent); width:28px; border-radius:2px 2px 0 0; opacity:.75; }
  .week-bar-label { font-size:.6rem; color:var(--muted); }
  .rec { font-size:.75rem; color:var(--muted); margin-top:4px; }
</style>
</head>
<body>
<h1>axon-signal</h1>
<nav>
  <button class="active" onclick="show('overview')">Overview</button>
  <button onclick="show('trends')">Trends</button>
  <button onclick="show('rework')">Rework</button>
  <button onclick="show('loops')">Loops</button>
  <button onclick="show('sessions')">Sessions</button>
</nav>

<div id="overview" class="pane active">
  <div class="grid">
    <div class="card"><div class="stat" id="avg-score">—</div><div class="label">Avg session score (7d)</div></div>
    <div class="card"><div class="stat" id="session-count">—</div><div class="label">Sessions this week</div></div>
    <div class="card"><div class="stat" id="coverage-pct">—</div><div class="label">Commit coverage %</div></div>
    <div class="card"><div class="stat" id="rework-count">—</div><div class="label">Rework hotspots</div></div>
  </div>
  <div class="card">
    <h2>Top Sessions (7d)</h2>
    <table><thead><tr><th>Score</th><th>Engineer</th><th>Goal</th><th>Turns</th><th>Commits</th></tr></thead>
    <tbody id="top-sessions"></tbody></table>
  </div>
</div>

<div id="trends" class="pane">
  <div id="trends-container"></div>
</div>

<div id="rework" class="pane">
  <div class="card">
    <h2>Rework Hotspots (14d)</h2>
    <div id="rework-list"></div>
  </div>
</div>

<div id="loops" class="pane">
  <div class="card">
    <h2>Loop Opportunities (sessions with 40+ turns)</h2>
    <table><thead><tr><th>Engineer</th><th>Goal</th><th>Turns</th><th>Commits</th><th>Score</th></tr></thead>
    <tbody id="loops-table"></tbody></table>
  </div>
</div>

<div id="sessions" class="pane">
  <div class="card">
    <h2>All Scored Sessions</h2>
    <table><thead><tr><th>Score</th><th>Tier</th><th>Engineer</th><th>Goal</th><th>Turns</th><th>Commits</th></tr></thead>
    <tbody id="sessions-table"></tbody></table>
  </div>
</div>

<script>
const panes = ['overview','trends','rework','loops','sessions'];
function show(id) {
  panes.forEach(p => {
    document.getElementById(p).classList.toggle('active', p===id);
    document.querySelectorAll('nav button').forEach((b,i) => b.classList.toggle('active', panes[i]===id));
  });
}

function tier_badge(tier) {
  if (!tier) return '';
  if (tier.includes('Gold')) return `<span class="badge badge-green">${tier}</span>`;
  if (tier.includes('Negative')) return `<span class="badge badge-red">${tier}</span>`;
  return `<span class="badge badge-yellow">${tier}</span>`;
}

function score_color(s) {
  if (s >= 75) return 'var(--green)';
  if (s >= 50) return 'var(--yellow)';
  return 'var(--red)';
}

async function loadOverview() {
  const [weekly, score, rework] = await Promise.all([
    fetch('/api/weekly').then(r=>r.json()),
    fetch('/api/score').then(r=>r.json()),
    fetch('/api/rework').then(r=>r.json()),
  ]);
  const week7 = score.filter(s => s.ts_ms > Date.now() - 7*86400*1000);
  const avg = week7.length ? Math.round(week7.reduce((a,s)=>a+s.score,0)/week7.length) : 0;
  document.getElementById('avg-score').textContent = avg;
  document.getElementById('avg-score').style.color = score_color(avg);
  document.getElementById('session-count').textContent = week7.length;
  const cov = weekly.commit_coverage_pct ?? 0;
  document.getElementById('coverage-pct').textContent = cov + '%';
  document.getElementById('rework-count').textContent = rework.length;

  const top = [...week7].sort((a,b)=>b.score-a.score).slice(0,10);
  document.getElementById('top-sessions').innerHTML = top.map(s => `
    <tr>
      <td style="color:${score_color(s.score)};font-weight:700">${s.score}</td>
      <td>${s.engineer.split('@')[0]}</td>
      <td>${(s.goal||'').slice(0,60)}</td>
      <td>${s.turns}</td>
      <td>${s.commits_linked}</td>
    </tr>`).join('');
}

async function loadTrends() {
  const data = await fetch('/api/trends').then(r=>r.json());
  const engs = data.engineers || [];
  const container = document.getElementById('trends-container');
  if (!engs.length) { container.innerHTML='<div class="card"><p style="color:var(--muted)">No trend data yet — ingest more sessions.</p></div>'; return; }

  const teamDir = data.team_direction;
  const teamArrow = teamDir==='Improving'?'↑ Improving':teamDir==='Declining'?'↓ Declining':'→ Stable';
  const teamColor = teamDir==='Improving'?'var(--green)':teamDir==='Declining'?'var(--red)':'var(--muted)';

  container.innerHTML = `
    <div class="card" style="margin-bottom:16px">
      <h2>Team Trend</h2>
      <span style="font-size:1.5rem;color:${teamColor}">${teamArrow}</span>
      <span style="color:var(--muted);margin-left:12px">this week ${Math.round(data.team_avg_this_week)}/100 · last week ${Math.round(data.team_avg_last_week)}/100</span>
    </div>
    <div class="grid">${engs.map(eng => {
      const dir = eng.direction;
      const arrow = dir==='Improving'?'↑':dir==='Declining'?'↓':dir==='Stable'?'→':'·';
      const color = dir==='Improving'?'var(--green)':dir==='Declining'?'var(--red)':'var(--muted)';
      const maxScore = Math.max(...eng.weeks.map(w=>w.avg_score),1);
      const bars = eng.weeks.map(w => `
        <div class="week-bar">
          <div class="week-bar-fill" style="height:${Math.round(w.avg_score/maxScore*32)}px" title="${w.week}: ${Math.round(w.avg_score)}/100"></div>
          <div class="week-bar-label">${w.week.slice(5)}</div>
        </div>`).join('');
      return `<div class="card">
        <div class="eng-name"><span style="color:${color}">${arrow}</span> ${eng.engineer.split('@')[0]}</div>
        <div style="font-size:.75rem;color:var(--muted)">${eng.total_sessions} sessions · avg ${Math.round(eng.overall_avg)}/100</div>
        <div class="week-bars">${bars}</div>
        <div class="rec">${eng.recommendation}</div>
      </div>`;
    }).join('')}</div>`;
}

async function loadRework() {
  const data = await fetch('/api/rework').then(r=>r.json());
  const el = document.getElementById('rework-list');
  if (!data.length) { el.innerHTML='<p style="color:var(--muted)">No rework hotspots detected.</p>'; return; }
  el.innerHTML = data.map(h => `
    <div class="warn">
      <strong>${h.file}</strong> — touched by ${h.session_count} session(s) in ${h.window_days}d<br>
      <span style="font-size:.75rem">${(h.goals||[]).map(g=>'"'+g.slice(0,60)+'"').join(' · ')}</span>
    </div>`).join('');
}

async function loadLoops() {
  const data = await fetch('/api/loops').then(r=>r.json());
  document.getElementById('loops-table').innerHTML = data.map(s=>`
    <tr>
      <td>${s.engineer.split('@')[0]}</td>
      <td>${(s.goal||'').slice(0,55)}</td>
      <td style="color:var(--yellow)">${s.turns}</td>
      <td>${s.commits_linked}</td>
      <td style="color:${score_color(s.score)}">${s.score}</td>
    </tr>`).join('') || '<tr><td colspan="5" style="color:var(--muted)">No high-turn sessions found.</td></tr>';
}

async function loadSessions() {
  const data = await fetch('/api/score').then(r=>r.json());
  const sorted = [...data].sort((a,b)=>b.ts_ms-a.ts_ms);
  document.getElementById('sessions-table').innerHTML = sorted.map(s=>`
    <tr>
      <td style="color:${score_color(s.score)};font-weight:700">${s.score}</td>
      <td>${tier_badge(s.tier)}</td>
      <td>${s.engineer.split('@')[0]}</td>
      <td>${(s.goal||'').slice(0,55)}</td>
      <td>${s.turns}</td>
      <td>${s.commits_linked}</td>
    </tr>`).join('') || '<tr><td colspan="6" style="color:var(--muted)">No sessions in ledger.</td></tr>';
}

// Load all panes on startup
Promise.all([loadOverview(), loadTrends(), loadRework(), loadLoops(), loadSessions()]);
</script>
</body>
</html>
"#;
