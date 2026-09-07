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

  /* ---------- skip link + main landmark (first focus stop) ---------- */
  function initSkipLink() {
    const main = $("main");
    if (!main) return;
    if (!main.id) main.id = "main";
    // A skip link that does not move focus is inert: activating it sets
    // location.hash but leaves document.activeElement on <body>, so assistive
    // technology never receives the context change. The landmark has to be
    // programmatically focusable for the link below to mean anything. Set this
    // before the early return, so pages that ship their own static skip link
    // still get a focusable target.
    if (!main.hasAttribute("tabindex")) main.setAttribute("tabindex", "-1");
    if ($(".skip-link")) return;
    const lang = (document.documentElement.lang || "en").slice(0, 2) === "ja" ? "ja" : "en";
    const label = lang === "ja" ? "メインコンテンツへスキップ" : "Skip to main content";
    const link = document.createElement("a");
    link.className = "skip-link";
    link.href = "#main";
    link.textContent = label;
    document.body.insertBefore(link, document.body.firstChild);
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

  /* ---------- correctness backbone (sitewide orientation) ----------
     Renders on every manual/hub page (not home/playground). Each stage
     names intent, seam, evidence, limitation, and next action. The
     current page's stage is highlighted — not a linear guarantee. */
  const BACKBONE_STAGES = [
    {
      id: "business",
      stem: "business-layer",
      en: {
        label: "Business",
        seam: "policy intent",
        intent: "Actors, processes, policies, goals, and KPIs stakeholders can read.",
        contract: "Top of the chain — no upper seam.",
        evidence: "<code>fslc verify</code> on policies/goals (<code>verified</code> / <code>violated</code> + trace).",
        limitation: "Bounded depth; no implementation detail or code paths.",
        next: "Requirements via <code>implements</code> — see <a href=\"{href:requirements-layer}\">requirements layer</a>.",
      },
      ja: {
        label: "業務",
        seam: "方針の意図",
        intent: "関係者が読める actor・プロセス・policy・goal・KPI。",
        contract: "連鎖の頂点 — 上位の seam はない。",
        evidence: "policy/goal への <code>fslc verify</code>（<code>verified</code> / <code>violated</code> + trace）。",
        limitation: "有界深さ；実装詳細やコード経路は含まない。",
        next: "<code>implements</code> で要件層へ — <a href=\"{href:requirements-layer}\">要件層</a>を参照。",
      },
    },
    {
      id: "requirements",
      stem: "requirements-layer",
      en: {
        label: "Requirements",
        seam: "<code>implements</code>",
        intent: "Requirement IDs, inputs, guards, acceptance, forbidden flows, NFRs.",
        contract: "<code>implements Business from \"file\" { }</code> — assert <code>implements.result == \"refines\"</code> yourself (failed seam can still exit 0).",
        evidence: "<code>fslc verify</code> + JSON <code>implements</code> field; counterexamples cite REQ IDs.",
        limitation: "Does not prove design structure or runtime code.",
        next: "Design via <code>refinement</code> mapping — <a href=\"{href:design-layer}\">design layer</a>.",
      },
      ja: {
        label: "要件",
        seam: "<code>implements</code>",
        intent: "要件ID・入力・ガード・受け入れ・禁止フロー・NFR。",
        contract: "<code>implements Business from \"file\" { }</code> — <code>implements.result == \"refines\"</code> を自分で確認（失敗しても exit 0 のことがある）。",
        evidence: "<code>fslc verify</code> と JSON の <code>implements</code>；反例は REQ ID を引用。",
        limitation: "設計構造やランタイムコードは証明しない。",
        next: "<code>refinement</code> マッピングで設計層へ — <a href=\"{href:design-layer}\">設計層</a>。",
      },
    },
    {
      id: "design",
      stem: "design-layer",
      en: {
        label: "Design",
        seam: "<code>refine</code>",
        intent: "Kernel <code>spec</code>: internal state, actions, invariants, composition.",
        contract: "<code>fslc refine design.fsl requirements.fsl mapping.fsl</code> — safety descends; liveness does not unless <code>preserve progress</code>.",
        evidence: "<code>refines</code> / <code>refinement_failed</code> + design <code>verify</code>/<code>proved</code>.",
        limitation: "Refinement is not full-system proof; upper requirements must be verified separately.",
        next: "Implementation observation — <a href=\"{href:examples}#correctness-chain\">examples/e2e</a> or <a href=\"{href:guide}\">workflow guide</a>.",
      },
      ja: {
        label: "設計",
        seam: "<code>refine</code>",
        intent: "カーネル <code>spec</code>：内部状態・アクション・不変条件・合成。",
        contract: "<code>fslc refine design.fsl requirements.fsl mapping.fsl</code> — 安全性は下がる；応答性は <code>preserve progress</code> なしでは自動では下がらない。",
        evidence: "<code>refines</code> / <code>refinement_failed</code> と設計の <code>verify</code>/<code>proved</code>。",
        limitation: "詳細化はシステム全体の証明ではない；上位要件は別途検証が必要。",
        next: "実装観測へ — <a href=\"{href:examples}#correctness-chain\">examples/e2e</a> または <a href=\"{href:guide}\">ワークフロー</a>。",
      },
    },
    {
      id: "observe",
      stem: "examples",
      en: {
        label: "Implementation",
        seam: "Adapter / oracle",
        intent: "Runtime code or logs projected into the design spec's logical state.",
        contract: "Design spec is the oracle; <code>Adapter.observe()</code> must be honest.",
        evidence: "<code>fslc scenarios</code> enumerates cases and <code>testgen</code> emits a conformance harness; <code>replay</code> judges an observed trace <code>conformant</code> / <code>nonconformant</code>.",
        limitation: "Observed traces only — not all future paths; <code>leadsTo</code> cannot be judged from a finite log.",
        next: "Repair loop: counterexample → spec or code fix → re-run native <code>fslc</code>. Walkthrough: <a href=\"{href:examples}#correctness-chain\">examples/e2e</a>.",
      },
      ja: {
        label: "実装",
        seam: "Adapter / oracle",
        intent: "設計仕様の論理状態へ射影したランタイムコードまたはログ。",
        contract: "設計仕様が oracle；<code>Adapter.observe()</code> は正直であること。",
        evidence: "<code>fslc scenarios</code> はケースを列挙し <code>testgen</code> は適合ハーネスを生成します。<code>replay</code> が観測トレースを <code>conformant</code> / <code>nonconformant</code> と判定します。",
        limitation: "観測されたトレースのみ — 将来の全経路ではない；有限ログから <code>leadsTo</code> は判定できない。",
        next: "修復ループ：反例 → 仕様またはコード修正 → ネイティブ <code>fslc</code> を再実行。手順: <a href=\"{href:examples}#correctness-chain\">examples/e2e</a>。",
      },
    },
  ];
  /* Only layer walkthrough chapters get a highlighted stage; cross-cutting
     reference/hub pages stay neutral (no false lifecycle highlight). */
  const PAGE_BACKBONE_FOCUS = {
    "business-layer": "business",
    "requirements-layer": "requirements",
    "design-layer": "design",
    examples: "observe",
  };
  function backboneHref(stem, lang) {
    return `${stem}.${lang}.html`;
  }
  function backboneFill(text, lang) {
    return String(text).replace(/\{href:(\w[\w-]*)\}/g, (_, stem) => backboneHref(stem, lang));
  }
  function initHomeBackbone() {
    const mount = $("#home-backbone");
    if (!mount) return;
    const lang = (document.documentElement.lang || "en").slice(0, 2) === "ja" ? "ja" : "en";
    const t = lang === "ja"
      ? {
        aria: "四段階の正しさの背骨",
        doc: "書いた仕様文そのものが読めるドキュメントです（Document は段階ではありません）。",
        repair: "反例・テスト結果・リプレイ観測は各段で検証し、仕様または実装へ戻して再検証します。",
        outcome: "証拠が積み上がるほど信頼は高まりますが、確率や百分率では表しません。",
        hints: ["方針の verify", "implements + 反例", "refine + verify", "replay / testgen"],
      }
      : {
        aria: "Four-stage correctness backbone",
        doc: "The spec you write is the readable document — Document is a property, not a stage.",
        repair: "Counterexamples, test results, and replay observations verify at every stage and loop back into spec or code.",
        outcome: "Confidence grows as evidence accumulates — never as a percentage or certainty claim.",
        hints: ["Policy verify", "implements + counterexamples", "refine + verify", "replay / testgen"],
      };
    const items = BACKBONE_STAGES.map((st, i) => {
      const m = lang === "ja" ? st.ja : st.en;
      const href = backboneHref(st.stem, lang);
      const hint = t.hints[i] || "";
      const node =
        `<li class="home-stage" data-backbone-stage="${st.id}">` +
        `<a class="home-stage-label" href="${href}">${m.label}</a>` +
        `<span class="home-stage-seam">${m.seam}</span>` +
        `<p class="home-stage-evidence">${hint}</p></li>`;
      return i < BACKBONE_STAGES.length - 1
        ? node + `<li class="home-connector" aria-hidden="true">→</li>`
        : node;
    }).join("");
    mount.setAttribute("aria-label", t.aria);
    mount.innerHTML =
      `<p class="home-doc-note">${t.doc}</p>` +
      `<ol class="home-stage-rail" role="list">${items}</ol>` +
      `<p class="home-repair-loop" data-home-repair><span aria-hidden="true">↺</span> ${t.repair}</p>` +
      `<p class="home-outcome" data-home-outcome>${t.outcome}</p>`;
  }
  /* Restrained spine trace: draws the rail once when visible; static if reduced motion. */
  function initSpineTrace() {
    if (reduce) {
      document.body.classList.add("spine-trace-ready");
      return;
    }
    const rail = $(".home-stage-rail") || $(".backbone-rail");
    if (!rail) return;
    const io = new IntersectionObserver(
      (es) => es.forEach((e) => {
        if (e.isIntersecting) {
          document.body.classList.add("spine-trace-ready");
          io.disconnect();
        }
      }),
      { threshold: 0.15 }
    );
    io.observe(rail);
  }

  function initBackbone() {
    if (document.body.classList.contains("site-home") || document.body.classList.contains("playground-page")) return;
    const lang = (document.documentElement.lang || "en").slice(0, 2) === "ja" ? "ja" : "en";
    const page = document.body.dataset.page || "";
    const focus = PAGE_BACKBONE_FOCUS[page] || null;
    const t = lang === "ja"
      ? { aria: "正しさの背骨 — 連鎖する有界証拠", note: "各段階が確立する主張の種類は段階ごとに違います（帰納法は無制限、BMC と refine は有界）。強調はこの章の位置づけであり、全体証明ではありません。", rail: "四段階" }
      : { aria: "Correctness backbone — connected bounded evidence", note: "What each stage establishes differs in kind — induction is unbounded, BMC and refine are bounded at the depth you ran. A highlight marks this chapter’s place, not whole-system proof.", rail: "Four stages" };
    const stages = BACKBONE_STAGES.map((st) => {
      const m = lang === "ja" ? st.ja : st.en;
      const isCurrent = focus === st.id;
      const href = backboneHref(st.stem, lang);
      return (
        `<li class="backbone-node${isCurrent ? " is-current" : ""}" data-backbone-stage="${st.id}"` +
        (isCurrent ? ' aria-current="step"' : "") + `>` +
        `<a class="backbone-label" href="${href}">${m.label}</a>` +
        `<span class="backbone-seam">${m.seam}</span></li>`
      );
    }).join('<li class="backbone-connector" aria-hidden="true">→</li>');
    const nav = document.createElement("nav");
    nav.className = "correctness-backbone correctness-backbone--compact reveal";
    nav.id = "correctness-backbone";
    nav.setAttribute("data-backbone", "");
    nav.setAttribute("aria-label", t.aria);
    nav.innerHTML =
      `<p class="backbone-note">${t.note}</p>` +
      `<p class="backbone-rail-label">${t.rail}</p>` +
      `<ol class="backbone-rail" role="list">${stages}</ol>`;
    const crumb = $("main .breadcrumb[data-nav]");
    if (crumb && crumb.parentElement) {
      crumb.parentElement.insertBefore(nav, crumb.nextSibling);
    } else {
      const hubHero = $("#hub-content section.hero");
      const target = hubHero || $("main > section:first-child");
      if (target && target.parentElement) {
        target.parentElement.insertBefore(nav, target.nextSibling);
      }
    }
    if (nav.parentElement) {
      const io = new IntersectionObserver(
        (es) => es.forEach((e) => { if (e.isIntersecting) { e.target.classList.add("in"); io.unobserve(e.target); } }),
        { threshold: 0.12 }
      );
      io.observe(nav);
    }
  }

  /* ---------- shared navigation (single source of truth) ----------
     Chapter order, titles, and the nav surfaces (top bar, docs sidebar,
     breadcrumb, footer, category hubs) are generated here so pages cannot
     drift out of sync. Each page only declares data-page="<stem>" +
     <html lang> (hub pages additionally declare data-hub="true").
     Format per chapter: [shortLabel, sidebarTitle, sidebarDescription].
     categoryId is a foreign key into CATEGORIES — the only place category
     label/order/description lives, so there is exactly one nav-mapping
     source per concern. */
  const CATEGORIES = [
    { id: "get-started", order: 1,
      en: ["Get Started", "Orientation and fit before you write anything"],
      ja: ["はじめる", "書く前に方向感と適合を確認する"] },
    { id: "guides", order: 2,
      en: ["Guides", "Task-oriented walkthroughs, one per layer"],
      ja: ["ガイド", "各層のタスク志向の手順"] },
    { id: "reference", order: 3,
      en: ["Reference", "Lookup-oriented syntax and tool surface"],
      ja: ["リファレンス", "検索用途の構文・ツール仕様"] },
    { id: "examples-background", order: 4,
      en: ["Examples & Background", "Worked mechanisms and dialect deep-dives"],
      ja: ["実例と背景", "仕組みの実例とダイアレクト詳細"] },
  ];
  /* Hub journey copy: audience/problem → FSL contract → evidence readout → next route.
     Rendered by initHub() into #hub-content with data-journey markers for the
     sitewide structural content contract (tests/test_site_refresh_contract.py). */
  const HUB_JOURNEYS = {
    "get-started": {
      en: {
        audience: "You are new to FSL or deciding whether machine-checked specs belong in your workflow. The question is not syntax first — it is whether your domain breaks through interaction order, flags, or cross-layer drift.",
        contract: "FSL treats the spec you write as the document people read and the native Rust <code>fslc</code> verifies. The contract is JSON on stdout, stable exit codes, and replayable traces — not a separate model file that can drift.",
        evidence: "You will read <code>result</code>, <code>completeness</code>, <code>trace</code>, and exit codes from the native CLI. Bounded <code>verified</code> is evidence up to depth <em>K</em>; it is not infinite proof until induction or explicit closure says so.",
        next: "Move in order: intuition → fit gates → a five-minute counterexample run on the native verifier.",
        routes: [
          { stem: "concept", why: "Why specs can be machine-checked and what a counterexample means" },
          { stem: "when-to-use", why: "Three gates for fit and what FSL genuinely cannot express" },
          { stem: "quickstart", why: "Build native <code>fslc</code>, run <code>check</code>/<code>verify</code>, read JSON" },
        ],
      },
      ja: {
        audience: "FSL が初めて、または機械検査可能な仕様をワークフローに入れるべきかを判断している段階です。最初の問いは文法ではなく、操作順序・フラグ・層間ドリフトで破れるドメインかどうかです。",
        contract: "FSL では人が読む仕様文そのものが、ネイティブ Rust <code>fslc</code> が検証する正典です。契約は stdout の JSON、安定した終了コード、再生可能なトレースであり、別モデルファイルではありません。",
        evidence: "ネイティブ CLI の <code>result</code>、<code>completeness</code>、<code>trace</code>、終了コードを読みます。有界 <code>verified</code> は深さ <em>K</em> までの証拠であり、帰納法や explicit 閉包が示すまで無限証明ではありません。",
        next: "直感 → 適合ゲート → ネイティブ検証器での5分反例、の順で進めてください。",
        routes: [
          { stem: "concept", why: "仕様を機械検査できる理由と反例の意味" },
          { stem: "when-to-use", why: "適合の3ゲートと FSL が表現できないこと" },
          { stem: "quickstart", why: "ネイティブ <code>fslc</code> をビルドし <code>check</code>/<code>verify</code> で JSON を読む" },
        ],
      },
    },
    guides: {
      en: {
        audience: "You already know why FSL fits and want to run the verification loop on a real project — across business, requirements, and design layers when useful.",
        contract: "Native <code>fslc</code> commands share one JSON envelope. Layer dialects refine intent downward; <code>refine</code> and <code>chain</code> check that lower layers still mean what upper layers require.",
        evidence: "Distinguish bounded <code>verified</code> (no counterexample to depth <em>K</em>), <code>proved</code> (unbounded induction/explicit closure), <code>refinement_failed</code> (mapping drift), and <code>replay</code>/<code>testgen</code> observations (implementation traces, not full proofs).",
        next: "Start with the workflow chapter, then open the layer guide that matches where you are writing today.",
        routes: [
          { stem: "guide", why: "BMC → induction → refinement → replay/testgen without overclaiming" },
          { stem: "business-layer", why: "Processes, controls, KPIs in the consulting dialect" },
          { stem: "requirements-layer", why: "Requirement IDs, acceptance, forbidden flows" },
          { stem: "design-layer", why: "State machines, refinement mappings, composition" },
        ],
      },
      ja: {
        audience: "FSL の適合は分かっており、実プロジェクトで検証ループを回したい段階です。必要なら業務・要件・設計の各層まで広げます。",
        contract: "ネイティブ <code>fslc</code> コマンドは共通の JSON エンベロープを共有します。層ダイアレクトは意図を下げ、<code>refine</code> と <code>chain</code> は下位層が上位の要求を保っているかを検査します。",
        evidence: "有界 <code>verified</code>（深さ <em>K</em> まで反例なし）、<code>proved</code>（無制限帰納/explicit 閉包）、<code>refinement_failed</code>（マッピングのずれ）、<code>replay</code>/<code>testgen</code> の観測（実装トレースの証拠であり全証明ではない）を区別します。",
        next: "まずワークフロー章から入り、今日書いている層のガイドを開いてください。",
        routes: [
          { stem: "guide", why: "BMC → 帰納 → 詳細化 → replay/testgen を過大主張なく" },
          { stem: "business-layer", why: "コンサルティング方言でのプロセス・統制・KPI" },
          { stem: "requirements-layer", why: "要件ID・受け入れ・禁止フロー" },
          { stem: "design-layer", why: "状態機械・詳細化マッピング・合成" },
        ],
      },
    },
    reference: {
      en: {
        audience: "You need authoritative lookup while writing or reviewing specs — semantics, command flags, diagnostics, or vocabulary — without mistaking compatibility snapshots for the product surface.",
        contract: "Four sources, in authority order: (1) generated <code>docs/LANGUAGE.md</code> language reference, (2) generated <code>rust/fslc/cli-contract.json</code> native CLI map, (3) hand-authored exit/result interpretation, (4) frozen Python under <code>src/fslc/</code> labeled compatibility-only.",
        evidence: "Read <code>result</code> + exit code together. <code>errors</code> indexes violation shapes; generated <code>cli</code> cites <code>outcome.rs</code>. Do not treat frozen Python argparse as current command authority.",
        next: "Pick the canonical page for your question, then return to guides or examples to apply it.",
        pillars: [
          { marker: "language-md", stem: "language", label: "Language semantics", blurb: "Generated from <code>docs/LANGUAGE.md</code> — canonical meaning of constructs and verifier behavior." },
          { marker: "cli-contract", stem: "cli", label: "Native CLI surface", blurb: "Generated from <code>rust/fslc/cli-contract.json</code> — every native <code>fslc</code> subcommand and flag." },
          { marker: "exit-interpret", stem: "errors", label: "Results & exit codes", blurb: "How to read <code>result</code>/<code>kind</code>/exit codes from native JSON output." },
          { marker: "frozen-compat", stem: null, label: "Frozen Python (parity only)", blurb: "<code>src/fslc/</code> mirrors a compatibility subset for tests — not the distribution or site CLI authority." },
        ],
      },
      ja: {
        audience: "仕様を書く・レビューするときに、意味論・コマンドフラグ・診断・用語の正典を参照したい段階です。互換スナップショットを製品面と混同しないでください。",
        contract: "権威の順序は4つ：(1) 生成 <code>docs/LANGUAGE.md</code> 言語リファレンス、(2) 生成 <code>rust/fslc/cli-contract.json</code> ネイティブ CLI、(3) 手書きの終了コード/結果の読み方、(4) <code>src/fslc/</code> の凍結 Python（互換のみと明記）。",
        evidence: "<code>result</code> と終了コードをセットで読みます。<code>errors</code> が違反形状を索引し、生成 <code>cli</code> は <code>outcome.rs</code> を引用します。凍結 Python の argparse を現行 CLI 権威と見なしません。",
        next: "問いに対応する正典ページを選び、ガイドや実例に戻って適用してください。",
        pillars: [
          { marker: "language-md", stem: "language", label: "言語意味論", blurb: "<code>docs/LANGUAGE.md</code> から生成 — 構文と検証器の正典。" },
          { marker: "cli-contract", stem: "cli", label: "ネイティブ CLI", blurb: "<code>rust/fslc/cli-contract.json</code> から生成 — 全ネイティブ <code>fslc</code> サブコマンド。" },
          { marker: "exit-interpret", stem: "errors", label: "結果と終了コード", blurb: "ネイティブ JSON の <code>result</code>/<code>kind</code>/終了コードの読み方。" },
          { marker: "frozen-compat", stem: null, label: "凍結 Python（parity のみ）", blurb: "<code>src/fslc/</code> はテスト用互換サブセット — 配布面でもサイト CLI 権威でもありません。" },
        ],
      },
    },
    "examples-background": {
      en: {
        audience: "You learn best from worked mechanisms, dialect deep-dives, or a curated map of the repository examples — not from reading directory names alone.",
        contract: "Examples are evidence anchors: each names the native command, expected <code>result</code>, and what kind of assurance the run provides. Cross-layer chains show refinement and implementation observation, not automatic end-to-end proof.",
        evidence: "Inspect gallery <code>expected-result</code> comments, e2e refinement links, replay/testgen harnesses, and dialect replay logs. Choose the example that matches your verification goal before opening source.",
        next: "Open the example gallery to pick by goal, or a mechanism/dialect chapter for the theory behind the run.",
        routes: [
          { stem: "examples", why: "Choose an example by verification goal and inspect the right JSON fields" },
          { stem: "mechanism", why: "BMC, induction, refinement, and Monitor/replay theory" },
          { stem: "domain", why: "fsl-domain structural findings and replay" },
          { stem: "db", why: "dbsystem compatibility and observe/import" },
          { stem: "ai", why: "ai_component contracts and runtime replay" },
        ],
      },
      ja: {
        audience: "仕組みの実例、ダイアレクト詳細、リポジトリ実例の地図から学びたい段階です。ディレクトリ名だけでは不十分です。",
        contract: "実例は証拠のアンカーです。各例はネイティブコマンド、期待 <code>result</code>、その実行が提供する保証の種類を名指しします。層をまたぐ連鎖は詳細化と実装観測を示しますが、自動の端到端証明ではありません。",
        evidence: "ギャラリーの <code>expected-result</code> コメント、e2e の詳細化リンク、replay/testgen ハーネス、ダイアレクト replay ログを見ます。ソースを開く前に検証目標に合う例を選んでください。",
        next: "検証目標別に実例ギャラリーを開くか、実行の理論は仕組み/ダイアレクト章へ。",
        routes: [
          { stem: "examples", why: "検証目標で例を選び、読むべき JSON フィールドを確認" },
          { stem: "mechanism", why: "BMC・帰納・詳細化・Monitor/replay の理論" },
          { stem: "domain", why: "fsl-domain の構造所見と replay" },
          { stem: "db", why: "dbsystem 互換と observe/import" },
          { stem: "ai", why: "ai_component 契約とランタイム replay" },
        ],
      },
    },
  };
  const CHAPTERS = [
    { id: "concept",            categoryId: "get-started", en: ["Concept", "What is FSL?", "Concepts and counterexamples"],          ja: ["概念", "FSLって、なに？", "概念と反例の直感"] },
    { id: "when-to-use",        categoryId: "get-started", en: ["When to use", "When to Use FSL", "Fit, gates, and scope"],          ja: ["使いどころ", "FSLを使うべきか", "効くドメインと判断ゲート"] },
    { id: "quickstart",         categoryId: "get-started", en: ["Quickstart", "5-Minute Quickstart", "Install, verify, read a counterexample"], ja: ["クイックスタート", "5分クイックスタート", "導入・検証・反例の読み方"] },

    { id: "guide",              categoryId: "guides", en: ["Workflow", "Workflow", "Commands and repair loop"],                 ja: ["使い方", "仕組みと使い方", "検証ループとコマンド"] },
    { id: "business-layer",     categoryId: "guides", en: ["Business", "Business Layer", "Processes, controls, KPIs"],           ja: ["業務層", "業務層", "プロセス・統制・KPI"] },
    { id: "requirements-layer", categoryId: "guides", en: ["Requirements", "Requirements Layer", "IDs, acceptance, forbidden"], ja: ["要件層", "要件層", "要件ID・受け入れ・禁止"] },
    { id: "design-layer",       categoryId: "guides", en: ["Design", "Design Layer", "Internal state, refinement, compose"],    ja: ["設計層", "設計層", "内部状態・詳細化・合成"] },

    { id: "syntax",             categoryId: "reference", en: ["Syntax", "Syntax Primer", "A 30-minute reading path into FSL source"],  ja: ["文法", "文法入門", "FSLを読むための30分ガイド"] },
    { id: "analysis",           categoryId: "reference", en: ["Analyze", "Structural Analysis", "TSG, graph projections, findings"], ja: ["構造分析", "構造分析", "TSG・グラフ投影・所見"] },
    { id: "language",           categoryId: "reference", en: ["Language", "Language Reference", "Generated, exhaustive map of LANGUAGE.md"], ja: ["言語仕様", "言語リファレンス", "LANGUAGE.mdから生成される網羅リファレンス"] },
    { id: "cli",                categoryId: "reference", en: ["CLI", "CLI Reference", "Generated map of every fslc subcommand"], ja: ["CLI", "CLIリファレンス", "全fslcサブコマンドを生成"] },
    { id: "errors",             categoryId: "reference", en: ["Errors", "Errors & Exit Codes", "JSON envelope, exit codes, violation/CTI/vacuity readouts"], ja: ["エラー", "エラー・終了コード", "JSONエンベロープ・終了コード・違反/CTI/空虚性の読み方"] },
    { id: "glossary",           categoryId: "reference", en: ["Glossary", "Glossary", "Reserved words, ja/en, one-line definitions"], ja: ["用語集", "用語集", "予約語・和訳・英語・一行定義"] },

    { id: "mechanism",          categoryId: "examples-background", en: ["Mechanisms", "Mechanisms", "BMC, induction, refinement"],           ja: ["仕組み", "仕組み詳細", "BMC・帰納法・詳細化"] },
    { id: "domain",             categoryId: "examples-background", en: ["fsl-domain", "Domain / Async Effects", "DDD, effects, scaffolds"], ja: ["fsl-domain", "DDD / 非同期Effect", "DDD・effect・scaffold"] },
    { id: "db",                 categoryId: "examples-background", en: ["fsl-db", "DB / Multi-env Compatibility", "Schema, artifacts, environments"], ja: ["fsl-db", "DB/複数環境互換性", "スキーマ・成果物・環境"] },
    { id: "ai",                 categoryId: "examples-background", en: ["fsl-ai", "AI Contracts & Agents", "Tool authority, agents, replay"], ja: ["fsl-ai", "AI contract / agent", "tool権限・agent構造・replay"] },
    { id: "examples",           categoryId: "examples-background", en: ["Examples", "Example Gallery", "A guided map of examples/ and specs/"], ja: ["実例", "実例ギャラリー", "examples/・specs/ の案内"] },
    { id: "design-notes",       categoryId: "examples-background", en: ["Design Notes", "Design Notes", "Why it's built this way — authoritative DESIGN-*.md contracts"], ja: ["設計ノート", "設計ノート", "なぜこの設計か — 正本のDESIGN-*.md契約"] },
  ];
  const OFFICIAL_NAV = [
    { id: "index", en: "Home", ja: "ホーム" },
    { id: "get-started", en: "Get Started", ja: "はじめる" },
    { id: "guides", en: "Guides", ja: "ガイド" },
    { id: "reference", en: "Reference", ja: "リファレンス" },
    { id: "examples-background", en: "Examples", ja: "実例" },
  ];
  const NAV_T = {
    en: { index: "English site", kicker: "Documentation", manual: "Manual", other: "日本語", otherRead: "日本語で読む",
          tagline: 'FSL — AI-Native Formal Specification Language. Static HTML under <code>docs/intro/</code>.' },
    ja: { index: "日本語サイト", kicker: "ドキュメント", manual: "マニュアル", other: "English", otherRead: "Read in English",
          tagline: 'FSL — AI向け形式仕様言語。<code>docs/intro/</code> の静的HTMLで構成されています。' },
  };
  function initNav() {
    const lang = (document.documentElement.lang || "en").slice(0, 2) === "ja" ? "ja" : "en";
    const other = lang === "ja" ? "en" : "ja";
    const page = document.body.dataset.page || "index"; // file stem, e.g. "concept" or "index"
    const t = NAV_T[lang];
    const href = (stem, l) => `${stem}.${l}.html`;
    const meta = (c) => (lang === "ja" ? c.ja : c.en);
    const catMeta = (cat) => (lang === "ja" ? cat.ja : cat.en);
    const cur = (id) => (id === page ? ' aria-current="page"' : "");
    const chapter = CHAPTERS.find((c) => c.id === page);
    const langToggle =
      `<span class="lang">` +
      (lang === "ja"
        ? `<a href="${href(page, "en")}">English</a><a href="${href(page, "ja")}" class="active" aria-current="true">日本語</a>`
        : `<a href="${href(page, "ja")}">日本語</a><a href="${href(page, "en")}" class="active" aria-current="true">English</a>`) +
      `</span>`;

    const navLinks = OFFICIAL_NAV.map((item) => {
      const label = lang === "ja" ? item.ja : item.en;
      const active = page === item.id ? ' aria-current="page"' : "";
      return `<a href="${href(item.id, lang)}"${active}>${label}</a>`;
    }).join("");
    const top = $("header.topbar[data-nav]");
    if (top) {
      top.innerHTML =
        `<a class="brand" href="${href("index", lang)}"><b>FSL</b></a>` +
        `<nav class="topnav" aria-label="${lang === "ja" ? "公式ナビゲーション" : "Official navigation"}">${navLinks}</nav>` +
        `<span class="spacer"></span>` +
        `<a class="topnav-manual" href="${href("index", lang)}#manual">${t.manual}</a>` +
        langToggle;
    }

    // Sidebar: 4 fixed categories, always expanded (no accordion — the
    // sidebar's one job is "scan every chapter, in order, in one glance").
    // Each category is a real <h2> + aria-labelledby <nav>, so a screen
    // reader's landmark/heading rotor sees 4 distinctly named groups
    // instead of 4 anonymous "navigation" regions.
    const side = $("aside.docs-sidebar[data-nav]");
    if (side) {
      const groups = CATEGORIES.slice().sort((a, b) => a.order - b.order).map((cat) => {
        const cm = catMeta(cat);
        const items = CHAPTERS.filter((c) => c.categoryId === cat.id).map((c, i) => {
          const m = meta(c);
          const num = String(i + 1).padStart(2, "0");
          return `<a class="chapter-link" href="${href(c.id, lang)}"${cur(c.id)}><span class="num">${num}</span><span><strong>${m[1]}</strong><span>${m[2]}</span></span></a>`;
        }).join("");
        return (
          `<h2 id="${cat.id}-label" class="docs-category-label">${cm[0]}</h2>` +
          `<nav class="docs-chapters" aria-labelledby="${cat.id}-label">${items}</nav>`
        );
      }).join("");
      side.innerHTML =
        `<a class="docs-sidebar-title" href="${href("index", lang)}#manual"><span>${t.kicker}</span><strong>${t.manual}</strong></a>` +
        groups +
        `<p class="docs-note"><a href="${href(page, other)}">${t.otherRead}</a></p>`;
    }

    // Breadcrumb: chapter/reference pages only (hub pages have no
    // "chapter" segment; the category link doubles as the only
    // "back to category" affordance — see DESIGN-docs-site.md D2).
    const crumb = $("nav.breadcrumb[data-nav]");
    if (crumb) {
      // docs/DESIGN-docs-site.md requires <nav aria-label="Breadcrumb">; the static
      // hosts ship unlabeled, so the accessible name is set here or nowhere.
      crumb.setAttribute("aria-label", lang === "ja" ? "パンくずリスト" : "Breadcrumb");
      if (chapter) {
        const cat = CATEGORIES.find((c) => c.id === chapter.categoryId);
        const cm = catMeta(cat);
        const m = meta(chapter);
        crumb.innerHTML =
          `<a href="${href(cat.id, lang)}">${cm[0]}</a><span> / </span>` +
          `<span aria-current="page">${m[1]}</span>`;
      } else {
        crumb.remove();
      }
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

  /* ---------- category hub pages ----------
     A hub page (data-hub="true", data-page="<categoryId>") has no content
     of its own: it renders CATEGORIES metadata + a CHAPTERS filter into
     #hub-content, reusing the existing .chapter-card component (already
     used on index.html) so there is no second per-chapter nav-mapping
     source to keep in sync with CHAPTERS/CATEGORIES. */
  function initHub() {
    const mount = $("#hub-content");
    if (!mount) return;
    const lang = (document.documentElement.lang || "en").slice(0, 2) === "ja" ? "ja" : "en";
    const page = document.body.dataset.page;
    const cat = CATEGORIES.find((c) => c.id === page);
    const journey = HUB_JOURNEYS[page];
    if (!cat || !journey) return;
    const cm = lang === "ja" ? cat.ja : cat.en;
    const j = lang === "ja" ? journey.ja : journey.en;
    const items = CHAPTERS.filter((c) => c.categoryId === cat.id);
    const href = (stem, l) => `${stem}.${l}.html`;
    const meta = (c) => (lang === "ja" ? c.ja : c.en);
    const readLabel = lang === "ja" ? "読む" : "Read";
    const stepLabels = lang === "ja"
      ? ["誰のため / 問題", "FSL の契約", "結果の読み方", "次に進む"]
      : ["Audience / problem", "FSL contract", "Read the evidence", "Next action"];
    const journeyHtml =
      `<section class="hub-journey"><div class="wrap narrow">` +
      `<div class="journey-grid reveal">` +
      `<article class="journey-step" data-journey="audience"><p class="kicker">${stepLabels[0]}</p><p>${j.audience}</p></article>` +
      `<article class="journey-step" data-journey="contract"><p class="kicker">${stepLabels[1]}</p><p>${j.contract}</p></article>` +
      `<article class="journey-step" data-journey="evidence"><p class="kicker">${stepLabels[2]}</p><p>${j.evidence}</p></article>` +
      `<article class="journey-step" data-journey="next"><p class="kicker">${stepLabels[3]}</p><p>${j.next}</p></article>` +
      `</div></div></section>`;
    let routesHtml = "";
    if (j.routes && j.routes.length) {
      const routeCards = j.routes.map((r) => {
        const ch = CHAPTERS.find((c) => c.id === r.stem);
        const title = ch ? meta(ch)[1] : r.stem;
        return (
          `<article class="syntax-card"><h3><a href="${href(r.stem, lang)}">${title}</a></h3>` +
          `<p>${r.why}</p></article>`
        );
      }).join("");
      routesHtml =
        `<section><div class="wrap"><p class="kicker reveal">${lang === "ja" ? "推奨ルート" : "Recommended routes"}</p>` +
        `<div class="syntax-grid reveal">${routeCards}</div></div></section>`;
    }
    let pillarsHtml = "";
    if (j.pillars && j.pillars.length) {
      const pillarCards = j.pillars.map((p) => {
        const title = p.label;
        const inner = p.stem
          ? `<h3><a href="${href(p.stem, lang)}">${title}</a></h3>`
          : `<h3>${title}</h3>`;
        return `<article class="syntax-card" data-journey="${p.marker}">${inner}<p>${p.blurb}</p></article>`;
      }).join("");
      pillarsHtml =
        `<section style="background:var(--surface-2)"><div class="wrap">` +
        `<p class="kicker reveal">${lang === "ja" ? "正典の区別" : "Canonical sources"}</p>` +
        `<div class="syntax-grid reveal">${pillarCards}</div></div></section>`;
    }
    const cards = items.map((c, i) => {
      const m = meta(c);
      const num = String(i + 1).padStart(2, "0");
      return (
        `<article class="chapter-card">` +
        `<span class="num">${num}</span>` +
        `<div><h3>${m[1]}</h3><p>${m[2]}</p></div>` +
        `<a class="btn" href="${href(c.id, lang)}">${readLabel}</a>` +
        `</article>`
      );
    }).join("");
    mount.innerHTML =
      `<section class="hero"><div class="wrap narrow center">` +
      `<p class="kicker reveal">${lang === "ja" ? "カテゴリ" : "Category"}</p>` +
      `<h1 class="reveal">${cm[0]}</h1>` +
      `<p class="lead reveal">${cm[1]}</p>` +
      `</div></section>` +
      journeyHtml +
      pillarsHtml +
      routesHtml +
      `<section><div class="wrap">` +
      `<p class="kicker reveal">${lang === "ja" ? "このカテゴリの章" : "Chapters in this category"}</p>` +
      `<div class="manual-main reveal">${cards}</div></div></section>`;
    initChrome(); // re-scan newly injected .reveal nodes
  }

  /* ---------- index.html category cards ----------
     The homepage's 4 entry cards are rendered from CATEGORIES (never grows
     past 4 — see DESIGN-docs-site.md D6) plus a chapter count derived from
     CHAPTERS, instead of being hand-authored per index.*.html. This is the
     same "read the data, don't duplicate the list" rule that keeps the hub
     pages (initHub) from drifting relative to CHAPTERS/CATEGORIES. */
  function initIndexCategories() {
    const mount = $("#index-categories");
    if (!mount) return;
    const lang = (document.documentElement.lang || "en").slice(0, 2) === "ja" ? "ja" : "en";
    const countLabel = lang === "ja" ? (n) => `${n}章` : (n) => `${n} chapter${n === 1 ? "" : "s"}`;
    mount.innerHTML = CATEGORIES.slice().sort((a, b) => a.order - b.order).map((cat) => {
      const cm = lang === "ja" ? cat.ja : cat.en;
      const n = CHAPTERS.filter((c) => c.categoryId === cat.id).length;
      return (
        `<a class="card" href="${cat.id}.${lang}.html" style="display:block;text-decoration:none;color:inherit">` +
        `<p class="kicker" style="margin-bottom:8px">${countLabel(n)}</p>` +
        `<h3>${cm[0]}</h3><p>${cm[1]}</p>` +
        `</a>`
      );
    }).join("");
  }

  /* ---------- disclosure-tree controls (language.html / cli.html) ----------
     Progressive enhancement only: native <details>/<summary> already work
     with JS disabled. This just toggles `open` on every node inside the
     nearest .disclosure-tree so a reader isn't forced to click one at a
     time in a page that's deep by design (see DESIGN-docs-site.md D3). */
  function initTreeControls() {
    $$(".tree-controls").forEach((ctl) => {
      const tree = ctl.nextElementSibling;
      if (!tree || !tree.classList.contains("disclosure-tree")) return;
      const expand = $(".op-expand", ctl);
      const collapse = $(".op-collapse", ctl);
      if (expand) expand.addEventListener("click", () => {
        $$("details", tree).forEach((d) => { d.open = true; });
      });
      if (collapse) collapse.addEventListener("click", () => {
        $$("details", tree).forEach((d) => { d.open = false; });
      });
    });
  }

  document.addEventListener("DOMContentLoaded", () => {
    document.body.classList.add("site-refreshed");
    initSkipLink();
    initNav();
    T = Object.assign({}, DEFAULT_T, readJSON("i18n") || {});
    initChrome();
    if ($("#diagram-hero") || $("#diagram-board")) initConcept();
    if ($("#diagram-cti")) initGuide();
    if (document.body.dataset.hub === "true") initHub();
    initHomeBackbone();
    initBackbone();
    initSpineTrace();
    initIndexCategories();
    initTreeControls();
  });
})();
