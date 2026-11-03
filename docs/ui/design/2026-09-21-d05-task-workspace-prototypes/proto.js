/* D05 — shared rendering for variants A, B, C. Identical data (fixture.js),
   identical panel DOM; only the layout shells differ. No production code. */
(function () {
  "use strict";
  var F = window.FIXTURE;
  function esc(s) {
    return String(s).replace(/[&<>"]/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c];
    });
  }
  function fill(id, html) {
    var el = document.getElementById(id);
    if (el) { el.innerHTML = html; }
  }

  /* ---------- left navigation (§9.1) ---------- */
  function renderNav() {
    var p = F.project;
    var html =
      '<div class="nav-section">' +
      '<div class="nav-item" role="button" tabindex="0"><strong>' + esc(p.name) +
      '</strong><span class="meta ellipsis">' + esc(p.repo) + "</span></div></div>" +
      '<div class="nav-section" role="region" aria-label="Tasks">';
    F.tasks.forEach(function (t) {
      html +=
        '<a class="nav-item" href="#" aria-current="' + (t.selected ? "true" : "false") + '">' +
        '<span class="meta mono">' + t.id.replace("TASK-", "") + "</span>" +
        '<span class="grow ellipsis">' + esc(t.objective) + "</span>" +
        '<span class="badge state-' + t.state + '">' + t.state + "</span></a>";
    });
    html += "</div>" +
      '<div class="nav-section" role="region" aria-label="Attention">' +
      '<div class="meta" style="padding:0 12px 4px">Attention (' + F.attention.length + ")</div>";
    F.attention.forEach(function (a) {
      var stale = a.status === "READ_ONLY";
      html +=
        '<a class="nav-item" href="#" aria-current="false">' +
        '<span class="badge' + (stale ? " neutral" : " attention") + '">' + a.type.split(" ")[0] + "</span>" +
        '<span class="grow ellipsis meta">' + a.id + " · " + a.task + "</span></a>";
    });
    html += "</div>" +
      '<div class="nav-utility"><div class="conn" title="' + esc(F.connection.note) + '">' +
      '<span class="dot" aria-hidden="true"></span>' +
      '<span class="meta grow">' + esc(F.connection.state) + " · " + esc(F.connection.protocol) + "</span>" +
      '<span class="meta">' + esc(F.connection.observed) + "</span></div>" +
      '<div class="btn-row" style="margin-top:8px">' +
      '<a class="btn meta" href="a.html">A</a><a class="btn meta" href="b.html">B</a>' +
      '<a class="btn meta" href="c.html">C</a>' +
      '<label class="meta" for="density">Density</label>' +
      '<select id="density" class="btn">' +
      '<option value="compact">Compact</option><option value="default" selected>Default</option>' +
      '<option value="comfortable">Comfortable</option></select>' +
      '<label class="meta" for="textsize">Text</label>' +
      '<select id="textsize" class="btn">' +
      '<option value="normal" selected>100%</option><option value="large">150%</option></select>' +
      "</div></div>";
    fill("nav", html);
    var d = document.getElementById("density");
    var tx = document.getElementById("textsize");
    if (d) { d.addEventListener("change", function () { document.documentElement.dataset.density = d.value; }); }
    if (tx) { tx.addEventListener("change", function () { document.documentElement.dataset.text = tx.value; }); }
  }

  /* ---------- attention item / decision detail (§13.4, §13.5) ---------- */
  function renderAttentionItem(a) {
    var html = '<div class="group"><div class="group-title">' +
      esc(a.type) + " " + esc(a.id) + " · " + esc(a.task) +
      ' — <span class="meta">' + esc(a.validity) + "</span></div><div style='padding:12px'>";
    if (a.operation) {
      html += '<p class="mono">' + esc(a.operation) + "</p>";
    }
    html += '<p>' + esc(a.why) + "</p>";
    if (a.reason === "NO_PROGRESS") {
      html += "<p class='meta' style='margin-top:8px'>Reason: " + esc(a.reason) +
        " — further work stopped. Recommendation shown separately from the choice: " +
        esc(a.recommendation) + ".</p>";
      a.choices.forEach(function (c, i) {
        html += '<label class="choice"><input type="radio" name="hdr-choice" value="' + c.id + '"' +
          (i === 0 ? "" : "") + '><span><strong>' + c.label + "</strong>" +
          '<span class="why">' + esc(c.why) + "</span></span></label>";
      });
      html += '<div class="btn-row" style="margin-top:12px">' +
        '<button class="btn primary" data-action="record-decision">Record decision</button>' +
        '<span class="meta">A recorded decision is shown as “Recorded”, then “Applied” once confirmed. No timer applies a choice.</span></div>';
    } else {
      html += '<div class="btn-row" style="margin-top:12px">' +
        '<button class="btn" disabled title="Request expired — read-only">Allow once</button>' +
        '<button class="btn" disabled title="Request expired — read-only">Deny operation</button>' +
        '<button class="btn">Open current request</button>' +
        '<span class="meta">' + esc(a.note) + "</span></div>";
    }
    html += "</div></div>";
    return html;
  }

  /* ---------- task panels (§9.1 tabs; content identical in A/B/C) ---------- */
  function badge(state) { return '<span class="badge state-' + state + '">' + state + "</span>"; }

  function renderOverview() {
    var t = F.task;
    var html =
      '<div class="callout info"><strong>' + esc(t.lifecycle) + "</strong> — " +
      esc(t.headlineReason) + '. <span class="meta">' + esc(t.nextCommand) + "</span></div>" +
      '<div class="group"><div class="group-title">Execution / Attempts</div><div style="padding:8px 12px">' +
      '<p class="meta" style="margin-bottom:8px">' + esc(t.execution.observation) + "</p>";
    t.attempts.forEach(function (a) {
      html += '<div class="row">' + badge(a.outcome === "SUCCEEDED" ? "COMPLETED" : "FAILED") +
        '<span class="grow">Attempt ' + a.n + " — " + esc(a.note) + "</span>" +
        '<span class="meta">' + esc(a.ended) + "</span>" +
        '<button class="btn meta" data-action="open-diagnostics">View diagnostics</button></div>';
    });
    html += "</div></div>" +
      '<div class="group"><div class="group-title">Blockers</div><div style="padding:12px">' +
      "<p>" + esc(t.childBlocker.text) + "</p>" +
      '<div class="btn-row" style="margin-top:8px"><button class="btn" data-action="open-decision">Open decision ' +
      esc(F.attention[0].id) + "</button></div></div></div>" +
      '<div class="group"><div class="group-title">Scope</div><div style="padding:8px 12px" class="btn-row">' +
      t.scope.map(function (s) { return '<span class="badge neutral mono">' + esc(s) + "</span>"; }).join("") +
      "</div></div>";
    return html;
  }

  function renderPlan() {
    var p = F.task.plan;
    return '<div class="group"><div class="group-title">Accepted revision — ' + esc(p.acceptedRevision) +
      " (head " + esc(p.head) + ")</div><div style='padding:12px'>" +
      "<p>Accepted plan head of the current branch. Provenance: " + esc(p.provenance) + ".</p>" +
      '<p class="meta" style="margin-top:8px">Plan list is the default view (§9.3); the graph is an analysis view, not the only way to read the plan.</p>' +
      "</div></div>" +
      '<div class="group"><div class="group-title">Proposed change set ' + esc(p.proposed.id) + " — proposal, not the acting plan</div>" +
      "<div style='padding:12px'><p>" + esc(p.proposed.delta) + " — " + esc(p.proposed.summary) + "</p>" +
      '<p class="meta" style="margin-top:8px">Risk: ' + esc(p.proposed.risk) + ". " +
      esc(p.proposed.validation) + ". A proposal never renders as an already-accepted revision.</p>" +
      '<div class="btn-row" style="margin-top:12px"><button class="btn">Review change set</button></div>' +
      "</div></div>";
  }

  function renderChanges() {
    var c = F.task.changes;
    var html = '<p class="meta" style="margin-bottom:8px">Diff against recorded base revision ' +
      esc(c.baseRevision) + ' — never silently compared against current main (§9.2).</p>';
    c.files.forEach(function (f) {
      html += '<div class="diff" style="margin-bottom:12px"><div class="file">' + esc(f.path) + "</div><pre>";
      f.hunks.forEach(function (h) {
        html += '<span class="' + h.t + '">' + esc(h.l) + "</span>";
      });
      html += "</pre></div>";
    });
    html += '<p class="meta">Unified diff shown; side-by-side appears at sufficient width per user preference (§14.5).</p>';
    return html;
  }

  function renderEvidence() {
    var t = F.task;
    var html = '<table class="data"><thead><tr><th>Criterion</th><th>Verdict</th>' +
      "<th>Snapshot</th><th>Producer</th><th>When</th><th>Links</th></tr></thead><tbody>";
    t.criteria.forEach(function (c) {
      html += "<tr><td>" + c.id + " — " + esc(c.text) + "</td>" +
        '<td class="verdict-' + c.verdict + '"><strong>' + c.verdict + "</strong>" +
        (c.note ? '<br><span class="meta">' + esc(c.note) + "</span>" : "") + "</td>" +
        "<td class='mono'>" + (c.snapshot ? esc(c.snapshot) : "—") + "</td>" +
        "<td>" + (c.producer ? esc(c.producer) : "—") + "</td>" +
        "<td>" + (c.when ? esc(c.when) : "—") + "</td>" +
        "<td>" + (c.links.length ? c.links.map(esc).join(", ") : "none yet") + "</td></tr>";
    });
    html += "</tbody></table>" +
      '<div class="callout" style="margin-top:16px"><strong>Staleness note.</strong> ' +
      esc(t.staleEvidence) + "</div>" +
      '<p class="meta">Evidence is organized by acceptance criterion, not by file type (§13.6). ' +
      "A VERIFIED verdict belongs to its recorded snapshot/revision and is never rewritten by later changes.</p>";
    return html;
  }

  function renderHistory() {
    var html = '<ul class="timeline">';
    F.task.history.forEach(function (h) {
      html += '<li><span class="seq">' + esc(h.seq) + '</span><span class="grow">' + esc(h.text) +
        '</span><span class="meta">' + esc(h.at) + "</span></li>";
    });
    html += "</ul>";
    return html;
  }

  function renderAttempts() {
    var t = F.task;
    var html = "";
    t.attempts.forEach(function (a) {
      html += '<div class="row"><strong>Attempt ' + a.n + "</strong>" +
        '<span class="badge ' + (a.outcome === "SUCCEEDED" ? "positive" : "state-FAILED") + '">' +
        a.outcome + '</span><span class="grow meta">' + esc(a.note) + "</span>" +
        '<span class="meta">' + esc(a.ended) + "</span></div>";
    });
    html += '<p class="meta" style="padding:4px 12px">An error in one Attempt does not recolor the whole Task (§10); Task lifecycle stays VERIFYING.</p>';
    return html;
  }

  function renderLogs() {
    var html = '<div class="log-follow"><span class="meta">Follow: on</span>' +
      '<button class="btn meta">Stop follow</button><button class="btn meta">Jump to newest</button>' +
      '<span class="meta">Filter: all sources · Attempt 2</span></div>' +
      '<div class="log" role="log" aria-label="Attempt 2 logs" tabindex="0">';
    F.task.logs.forEach(function (l) {
      html += '<div><span class="src">[' + esc(l.src) + "]</span> " + esc(l.line) + "</div>";
    });
    html += "</div><p class='meta' style='padding:4px 8px'>Read-only command output — " +
      "visibly distinct from a writable terminal, which would require its own IPC/authority contract (§14.5).</p>";
    return html;
  }

  function mountPanels(map) {
    Object.keys(map).forEach(function (id) { fill(id, map[id]()); });
  }

  function wireTabs(tabsEl, panelRoot) {
    tabsEl.addEventListener("click", function (e) {
      var btn = e.target.closest(".tab");
      if (!btn) { return; }
      tabsEl.querySelectorAll(".tab").forEach(function (t) { t.setAttribute("aria-selected", "false"); });
      btn.setAttribute("aria-selected", "true");
      panelRoot.querySelectorAll("section[data-panel]").forEach(function (s) {
        s.setAttribute("data-active", s.getAttribute("data-panel") === btn.dataset.panel ? "true" : "false");
      });
    });
  }

  window.PROTO = {
    esc: esc, fill: fill, badge: badge,
    mountPanels: mountPanels,
    wireTabs: wireTabs,
    renderNav: renderNav,
    renderAttentionItem: renderAttentionItem,
    renderOverview: renderOverview,
    renderPlan: renderPlan,
    renderChanges: renderChanges,
    renderEvidence: renderEvidence,
    renderHistory: renderHistory,
    renderAttempts: renderAttempts,
    renderLogs: renderLogs
  };
})();