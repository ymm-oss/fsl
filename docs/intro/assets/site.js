/* FSL intro site — shared engine (generic; pages provide JSON config + trace) */
(function () {
  "use strict";
  const NS = "http://www.w3.org/2000/svg";
  const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const wait = (ms) => new Promise((r) => setTimeout(r, reduce ? Math.min(ms, 120) : ms));
  const $ = (s, r = document) => r.querySelector(s);
  const $$ = (s, r = document) => Array.from(r.querySelectorAll(s));
  const readJSON = (id) => { const n = document.getElementById(id); return n ? JSON.parse(n.textContent) : null; };

  // user-facing strings (Japanese defaults; pages may override via <script id="i18n">)
  const DEFAULT_T = {
    heroBanner: "❌ <b>違反 (violated)</b> — ルール「{inv}」が破れました。<br>最短の反例トレース：<b>submit → {act}</b>（{step} 手）。承認を飛ばして出荷に到達しています。",
    heroVerdict: "❌ violated",
    boardViol: "❌ <b>ルール違反！</b> 承認なしで出荷されました（approved = false のまま Shipped）。手では見つけにくいこの抜け穴を、機械は総当たりで一発で見つけます。",
    boardShip: "✅ 承認を経て出荷。ルールは守られています。",
    boardCancel: "取消で終了。もう一度試すには「リセット」。",
    ctiBanner: "⚠ <b>反例候補 (CTI)</b> — もし「承認済なのに approved = false」という状態があれば、ship 一発でルールが破れます。<br>この状態は init から実際には到達しませんが、帰納法はそれを前提にしません。",
    ctiVerdict: "⚠ unknown_cti",
    provedBanner: "✅ <b>proved</b> — 補助不変条件「承認済 ⇒ approved」を1行足すと、帰納法が<b>無限の深さ</b>で証明を完了しました。",
    provedVerdict: "✓ proved",
  };
  let T = DEFAULT_T;
  const fmt = (s, o) => String(s).replace(/\{(\w+)\}/g, (_, k) => (o && k in o ? o[k] : ""));

  function svg(tag, attrs) {
    const e = document.createElementNS(NS, tag);
    for (const k in attrs) e.setAttribute(k, attrs[k]);
    return e;
  }

  /* ---------- page chrome: progress bar + scroll reveal ---------- */
  function initChrome() {
    const bar = $(".progress");
    if (bar) {
      const onScroll = () => {
        const h = document.documentElement;
        const p = h.scrollTop / (h.scrollHeight - h.clientHeight || 1);
        bar.style.width = (p * 100).toFixed(2) + "%";
      };
      addEventListener("scroll", onScroll, { passive: true });
      onScroll();
    }
    const io = new IntersectionObserver(
      (es) => es.forEach((e) => { if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); } }),
      { threshold: 0.12 }
    );
    $$(".reveal").forEach((n) => io.observe(n));
  }

  /* ---------- state diagram ---------- */
  const W = 116, H = 46, HW = 58, HH = 23;

  function edgeGeom(cfg, e) {
    const a = cfg.nodes.find((n) => n.id === e.from);
    const b = cfg.nodes.find((n) => n.id === e.to);
    if (e.curve) {
      const x1 = a.cx, y1 = a.cy - HH, x2 = b.cx, y2 = b.cy - HH;
      const cx = (x1 + x2) / 2, cy = Math.min(y1, y2) - 52;
      return { d: `M ${x1} ${y1} Q ${cx} ${cy} ${x2} ${y2}`, lx: cx, ly: cy + 14 };
    }
    if (a.cy === b.cy) { // horizontal
      const dir = b.cx > a.cx ? 1 : -1;
      const x1 = a.cx + dir * HW, x2 = b.cx - dir * HW;
      return { d: `M ${x1} ${a.cy} L ${x2} ${b.cy}`, lx: (x1 + x2) / 2, ly: a.cy - 10 };
    }
    // vertical
    const dir = b.cy > a.cy ? 1 : -1;
    const y1 = a.cy + dir * HH, y2 = b.cy - dir * HH;
    return { d: `M ${a.cx} ${y1} L ${b.cx} ${y2}`, lx: a.cx + 16, ly: (y1 + y2) / 2 };
  }

  function buildDiagram(host, cfg, opts = {}) {
    host.innerHTML = "";
    host.classList.add("diagram");
    const root = svg("svg", { viewBox: "0 0 760 250", role: "img" });

    const defs = svg("defs", {});
    [["ar", "var(--muted)"], ["arb", "var(--brand)"], ["ard", "var(--danger)"]].forEach(([id, fill]) => {
      const m = svg("marker", { id: id + (opts.ns || ""), markerWidth: "9", markerHeight: "9", refX: "7", refY: "3", orient: "auto", markerUnits: "strokeWidth" });
      const p = svg("path", { d: "M0,0 L7,3 L0,6 Z", fill });
      m.appendChild(p); defs.appendChild(m);
    });
    root.appendChild(defs);

    const edges = {}, nodes = {};
    cfg.edges.forEach((e) => {
      if (e.kind === "bug" && !opts.showBug) return;
      const g = edgeGeom(cfg, e);
      const path = svg("path", { d: g.d, class: "edge" + (e.kind === "bug" ? " bug" : ""), "marker-end": `url(#${e.kind === "bug" ? "ard" : "ar"}${opts.ns || ""})` });
      const label = svg("text", { x: g.lx, y: g.ly, class: "edge-label" + (e.kind === "bug" ? " bug" : ""), "text-anchor": "middle" });
      label.textContent = e.label;
      root.appendChild(path); root.appendChild(label);
      edges[e.id] = { e, path };
    });
    cfg.nodes.forEach((n) => {
      const g = svg("g", { class: "node", "data-id": n.id });
      const rect = svg("rect", { x: n.cx - HW, y: n.cy - HH, width: W, height: H, rx: 11 });
      const text = svg("text", { x: n.cx, y: n.cy + 5, "text-anchor": "middle" });
      text.textContent = n.label;
      g.appendChild(rect); g.appendChild(text); root.appendChild(g);
      nodes[n.id] = g;
    });
    host.appendChild(root);

    function clearNodes() { Object.values(nodes).forEach((g) => g.classList.remove("active", "violate", "done", "cti")); }
    function resetEdges() {
      Object.values(edges).forEach(({ e, path }) => {
        path.classList.remove("fire");
        path.setAttribute("marker-end", `url(#${e.kind === "bug" ? "ard" : "ar"}${opts.ns || ""})`);
      });
    }
    function fireEdge(id) {
      const it = edges[id]; if (!it) return;
      it.path.classList.add("fire");
      it.path.setAttribute("marker-end", `url(#${it.e.kind === "bug" ? "ard" : "arb"}${opts.ns || ""})`);
    }
    const findEdgeId = (action, from, to) => {
      const m = cfg.edges.find((e) => e.action === action && e.from === from && e.to === to);
      return m ? m.id : null;
    };

    const ro = opts.readout || null;
    function setReadout(state, viol) {
      if (!ro) return;
      ro.innerHTML = "";
      const mk = (label, val, cls) => {
        const c = document.createElement("span");
        c.className = "chip" + (cls ? " " + cls : "");
        c.innerHTML = `${label} <b>${val}</b>`;
        return c;
      };
      const labelOf = (id) => (cfg.nodes.find((n) => n.id === id) || {}).label || id;
      const lab = labelOf(state.status);
      ro.appendChild(mk("status =", lab === state.status ? state.status : lab + " (" + state.status + ")", viol ? "bad" : ""));
      if ("approved" in state)
        ro.appendChild(mk("approved =", String(state.approved), state.approved ? "good" : (viol ? "bad" : "")));
    }

    return {
      el: root, nodes, edges, cfg,
      setActive(id) { clearNodes(); if (nodes[id]) nodes[id].classList.add("active"); },
      addFireByState(action, from, to) { const id = findEdgeId(action, from, to); if (id) fireEdge(id); return id; },
      fireEdge, violate(id) { if (nodes[id]) nodes[id].classList.add("violate"); },
      done(id) { if (nodes[id]) nodes[id].classList.add("done"); },
      cti(id) { clearNodes(); if (nodes[id]) nodes[id].classList.add("cti"); },
      clearNodes, resetEdges, setReadout,
      reset() { clearNodes(); resetEdges(); if (ro) ro.innerHTML = ""; },
      async play(data, o = {}) {
        const speed = o.speed || 950;
        this.reset();
        const tr = data.trace;
        for (let i = 0; i < tr.length; i++) {
          const st = tr[i];
          const isViol = data.violated_at_step === st.step;
          if (st.action && st.changes && st.changes.status) {
            this.addFireByState(st.action.name, st.changes.status.from, st.changes.status.to);
          }
          if (o.hypothetical && i === 0) this.cti(st.state.status);
          else this.setActive(st.state.status);
          this.setReadout(st.state, isViol);
          if (isViol) this.violate(st.state.status);
          if (o.onStep) o.onStep(st, i);
          await wait(i === 0 ? speed * 0.6 : speed);
        }
        if (o.onEnd) o.onEnd(data);
      },
    };
  }

  /* ---------- concept page wiring ---------- */
  function initConcept() {
    const cfg = readJSON("diagram-config");
    const buggy = readJSON("trace-buggy");
    if (!cfg) return;

    // scene: structure (static diagram, happy path tinted)
    const intro = $("#diagram-intro");
    if (intro) {
      const d = buildDiagram(intro, cfg, { ns: "i" });
      d.setActive("Draft");
    }

    // scene: test vs all (two static diagrams)
    const testD = $("#diagram-test");
    if (testD) {
      const d = buildDiagram(testD, cfg, { ns: "t" });
      ["submit", "approve", "ship"].forEach((id) => d.fireEdge(id));
      d.done("Shipped");
    }
    const allD = $("#diagram-all");
    if (allD) {
      const d = buildDiagram(allD, cfg, { ns: "a", showBug: true });
      ["submit", "approve", "ship", "cancel1", "shipbug"].forEach((id) => d.fireEdge(id));
      d.violate("Shipped");
    }

    // scene: hero counterexample player
    const heroHost = $("#diagram-hero");
    if (heroHost && buggy) {
      const ro = $("#readout-hero");
      const d = buildDiagram(heroHost, cfg, { ns: "h", showBug: true, readout: ro });
      d.setActive("Draft");
      d.setReadout(buggy.trace[0].state, false);
      const btn = $("#play-hero");
      const banner = $("#banner-hero");
      const verdict = $("#verdict-hero");
      btn.addEventListener("click", async () => {
        btn.disabled = true;
        if (banner) { banner.className = "banner"; }
        if (verdict) verdict.innerHTML = "";
        await d.play(buggy, {
          speed: 1050,
          onEnd: (data) => {
            if (banner) {
              banner.className = "banner violated show";
              banner.innerHTML = fmt(T.heroBanner, {
                inv: data.invariant,
                act: data.last_action ? data.last_action.name : "?",
                step: data.violated_at_step,
              });
            }
            if (verdict) verdict.innerHTML = `<span class="badge violated">${T.heroVerdict}</span>`;
            btn.disabled = false;
          },
        });
      });
    }

    // scene: interactive "try to break it" board
    initBoard(cfg);
  }

  function initBoard(cfg) {
    const host = $("#diagram-board");
    if (!host) return;
    const ro = $("#readout-board");
    const banner = $("#banner-board");
    const modeEl = $("#board-mode"); // checkbox: checked = buggy
    const btns = {
      submit: $("#op-submit"), approve: $("#op-approve"), ship: $("#op-ship"),
      cancel: $("#op-cancel"), reset: $("#op-reset"),
    };
    let d = null;
    let state = { status: "Draft", approved: false };
    let dead = false;

    function rebuild() {
      const buggy = modeEl && modeEl.checked;
      d = buildDiagram(host, cfg, { ns: "b", showBug: buggy, readout: ro });
      state = { status: "Draft", approved: false };
      dead = false;
      if (banner) banner.className = "banner";
      render(null);
    }
    function allowed(op) {
      const buggy = modeEl && modeEl.checked;
      if (dead) return false;
      switch (op) {
        case "submit": return state.status === "Draft";
        case "approve": return state.status === "Submitted";
        case "ship": return buggy ? (state.status === "Submitted" || state.status === "Approved") : state.status === "Approved";
        case "cancel": return state.status !== "Shipped" && state.status !== "Cancelled";
      }
      return false;
    }
    function render(firedEdgeId) {
      d.resetEdges();
      if (firedEdgeId) d.fireEdge(firedEdgeId);
      const viol = state.status === "Shipped" && state.approved === false;
      d.setActive(state.status);
      if (viol) d.violate(state.status);
      else if (state.status === "Shipped") d.done(state.status);
      d.setReadout(state, viol);
      ["submit", "approve", "ship", "cancel"].forEach((op) => { btns[op].disabled = !allowed(op); });
      if (viol && banner) {
        banner.className = "banner violated show";
        banner.innerHTML = T.boardViol;
        dead = true; render2disable();
      } else if (state.status === "Shipped" && banner) {
        banner.className = "banner ok show";
        banner.innerHTML = T.boardShip;
        dead = true; render2disable();
      } else if (state.status === "Cancelled" && banner) {
        banner.className = "banner ok show";
        banner.innerHTML = T.boardCancel;
        dead = true; render2disable();
      }
    }
    function render2disable() { ["submit", "approve", "ship", "cancel"].forEach((op) => { btns[op].disabled = true; }); }

    function step(op) {
      if (!allowed(op)) return;
      let fired = null;
      if (op === "submit") { fired = "submit"; state.status = "Submitted"; }
      else if (op === "approve") { fired = "approve"; state.status = "Approved"; state.approved = true; }
      else if (op === "ship") { fired = state.status === "Submitted" ? "shipbug" : "ship"; state.status = "Shipped"; }
      else if (op === "cancel") { fired = "cancel1"; state.status = "Cancelled"; }
      render(fired);
    }

    btns.submit.addEventListener("click", () => step("submit"));
    btns.approve.addEventListener("click", () => step("approve"));
    btns.ship.addEventListener("click", () => step("ship"));
    btns.cancel.addEventListener("click", () => step("cancel"));
    btns.reset.addEventListener("click", rebuild);
    if (modeEl) modeEl.addEventListener("change", rebuild);
    rebuild();
  }

  /* ---------- guide page: BMC -> induction -> CTI -> proved ---------- */
  function initGuide() {
    const cfg = readJSON("diagram-config");
    const cti = readJSON("trace-cti");
    const host = $("#diagram-cti");
    if (!cfg || !cti || !host) return;
    const ro = $("#readout-cti");
    const d = buildDiagram(host, cfg, { ns: "g", showBug: false, readout: ro });
    d.cti("Approved");
    d.setReadout(cti.trace[0].state, true);
    const banner = $("#banner-cti"), verdict = $("#verdict-cti");
    const btnInd = $("#play-cti"), btnFix = $("#fix-cti"), aux = $("#aux-line");
    if (btnFix) btnFix.disabled = true;

    btnInd && btnInd.addEventListener("click", async () => {
      btnInd.disabled = true;
      if (banner) banner.className = "banner";
      if (verdict) verdict.innerHTML = "";
      if (aux) aux.style.display = "none";
      if (btnFix) btnFix.disabled = true;
      await d.play(cti, {
        speed: 1150, hypothetical: true,
        onEnd: () => {
          if (verdict) verdict.innerHTML = `<span class="badge cti">${T.ctiVerdict}</span>`;
          if (banner) { banner.className = "banner cti show"; banner.innerHTML = T.ctiBanner; }
          if (btnFix) btnFix.disabled = false;
          btnInd.disabled = false;
        },
      });
    });

    btnFix && btnFix.addEventListener("click", () => {
      d.reset();
      ["submit", "approve", "ship"].forEach((id) => d.fireEdge(id));
      d.done("Shipped");
      d.setReadout({ status: "Shipped", approved: true }, false);
      if (verdict) verdict.innerHTML = `<span class="badge proved">${T.provedVerdict}</span>`;
      if (banner) { banner.className = "banner ok show"; banner.innerHTML = T.provedBanner; }
      if (aux) aux.style.display = "block";
      btnFix.disabled = true;
    });
  }

  /* ---------- shared navigation (single source of truth) ----------
     Chapter order, titles, and the three nav surfaces (top bar, docs
     sidebar, footer) are generated here so pages cannot drift out of
     sync. Each page only declares data-page="<stem>" + <html lang>.
     Format per chapter: [shortLabel, sidebarTitle, sidebarDescription]. */
  const CHAPTERS = [
    { id: "concept",            en: ["Concept", "What is FSL?", "Concepts and counterexamples"],          ja: ["概念", "FSLって、なに？", "概念と反例の直感"] },
    { id: "when-to-use",        en: ["When to use", "When to Use FSL", "Fit, gates, and scope"],          ja: ["使いどころ", "FSLを使うべきか", "効くドメインと判断ゲート"] },
    { id: "guide",              en: ["Workflow", "Workflow", "Commands and repair loop"],                 ja: ["使い方", "仕組みと使い方", "検証ループとコマンド"] },
    { id: "mechanism",          en: ["Mechanisms", "Mechanisms", "BMC, induction, refinement"],           ja: ["仕組み", "仕組み詳細", "BMC・帰納法・詳細化"] },
    { id: "business-layer",     en: ["Business", "Business Layer", "Processes, controls, KPIs"],           ja: ["業務層", "業務層", "プロセス・統制・KPI"] },
    { id: "requirements-layer", en: ["Requirements", "Requirements Layer", "IDs, acceptance, forbidden"], ja: ["要件層", "要件層", "要件ID・受け入れ・禁止"] },
    { id: "design-layer",       en: ["Design", "Design Layer", "Internal state, refinement, compose"],    ja: ["設計層", "設計層", "内部状態・詳細化・合成"] },
    { id: "syntax",             en: ["Syntax", "Syntax Guide", "Types, actions, properties"],             ja: ["文法", "文法・構文", "型・式・操作・性質"] },
    { id: "analysis",           en: ["Analyze", "Structural Analysis", "TSG, graph projections, findings"], ja: ["構造分析", "構造分析", "TSG・グラフ投影・所見"] },
    { id: "db",                 en: ["fsl-db", "DB / Multi-env Compatibility", "Schema, artifacts, environments"], ja: ["fsl-db", "DB/複数環境互換性", "スキーマ・成果物・環境"] },
    { id: "ai",                 en: ["fsl-ai", "AI Contracts & Agents", "Tool authority, agents, replay"], ja: ["fsl-ai", "AI contract / agent", "tool権限・agent構造・replay"] },
  ];
  const NAV_T = {
    en: { brand: "Manual", index: "English Manual", kicker: "FSL Manual", other: "日本語", otherRead: "日本語で読む",
          tagline: 'FSL — AI-Native Formal Specification Language. This manual is static HTML under <code>docs/intro/</code>.' },
    ja: { brand: "Manual", index: "日本語マニュアル", kicker: "FSL Manual", other: "English", otherRead: "Read in English",
          tagline: 'FSL — AI向け形式仕様言語。このマニュアルは <code>docs/intro/</code> の静的HTMLで構成されています。' },
  };
  function initNav() {
    const lang = (document.documentElement.lang || "en").slice(0, 2) === "ja" ? "ja" : "en";
    const other = lang === "ja" ? "en" : "ja";
    const page = document.body.dataset.page || "index"; // file stem, e.g. "concept" or "index"
    const t = NAV_T[lang];
    const href = (stem, l) => `${stem}.${l}.html`;
    const meta = (c) => (lang === "ja" ? c.ja : c.en);
    const cur = (id) => (id === page ? ' aria-current="page"' : "");
    const langToggle =
      `<span class="lang">` +
      (lang === "ja"
        ? `<a href="${href(page, "en")}">English</a><a href="${href(page, "ja")}" class="active">日本語</a>`
        : `<a href="${href(page, "ja")}">日本語</a><a href="${href(page, "en")}" class="active">English</a>`) +
      `</span>`;

    // Top bar is global chrome only: brand + language. Chapter navigation
    // lives in the sidebar (content pages), the body grid (home), and the
    // footer — so the sticky bar stays light and never overflows on mobile.
    const top = $("header.topbar[data-nav]");
    if (top) {
      top.innerHTML =
        `<a class="brand" href="${href("index", lang)}"><b>FSL</b> ${t.brand}</a>` +
        `<span class="spacer"></span>` + langToggle;
    }

    const side = $("aside.docs-sidebar[data-nav]");
    if (side) {
      const items = CHAPTERS.map((c, i) => {
        const m = meta(c);
        const num = String(i + 1).padStart(2, "0");
        return `<a class="chapter-link" href="${href(c.id, lang)}"${cur(c.id)}><span class="num">${num}</span><span><strong>${m[1]}</strong><span>${m[2]}</span></span></a>`;
      }).join("");
      side.innerHTML =
        `<a class="docs-sidebar-title" href="${href("index", lang)}"><span>${t.kicker}</span><strong>${t.index}</strong></a>` +
        `<nav class="docs-chapters">${items}</nav>` +
        `<p class="docs-note"><a href="${href(page, other)}">${t.otherRead}</a></p>`;
    }

    const foot = $("footer[data-nav]");
    if (foot) {
      const links = CHAPTERS.map((c) => `<a href="${href(c.id, lang)}">${meta(c)[0]}</a>`).join(" · ");
      foot.innerHTML =
        `<p>${t.tagline}</p>` +
        `<p><a href="${href("index", lang)}">Index</a> · ${links} · ` +
        `<a href="${href(page, other)}">${t.other}</a> · ` +
        `<a href="https://github.com/ymm-oss/fsl" target="_blank" rel="noopener">GitHub</a></p>`;
    }
  }

  document.addEventListener("DOMContentLoaded", () => {
    initNav();
    T = Object.assign({}, DEFAULT_T, readJSON("i18n") || {});
    initChrome();
    if ($("#diagram-hero") || $("#diagram-board")) initConcept();
    if ($("#diagram-cti")) initGuide();
  });
})();
