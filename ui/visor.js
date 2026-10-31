// Her memory in 3D, read only: a snapshot of what she has in mind right now (`memory_graph`),
// redrawn when she learns something. Nothing here writes to her memory.
//
// - Santuario: her core (who she is, her fixed rules) on the altar, and what she consolidated
//   around it, on a temple floor of rings.
// - Hélice: what she lived, in order, rising in a spiral: the oldest at the bottom, the present at
//   the top, what is scheduled above it. Terra (her base history) can sit below as a faint coil.
// - Arboleda: people, things and concepts. The most connected are trees; a click on one unfolds
//   its roots (the memories it is linked to), and so on.
// - Constelación: all of it at once, by sectors (see `constelacion`).
"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const $ = (id) => document.getElementById(id);

const MOD_COLORS = { yo: "#9fd24a", personas: "#f3c06b", cosas: "#6fb7e8", saber: "#b59bff" };
const modTitle = (m) => tOr(`visor.mod.${m}`, m);
const EXTRA_COLOR = "#c7c7c7";
const edgeName = (k) => tOr(`visor.edge.${k}`, k);
const EDGE_COLORS = { causa: "#f0b44c", precede: "#3fd6c8", relacion: "#8d978f", contradice: "#e0605a", menciona: "#5d6862" };
const CONCEPT_KINDS = new Set(["persona", "objeto", "lugar", "rasgo", "hecho", "evento"]);

const st = {
  g: null,
  byId: new Map(),
  adj: new Map(),
  view: "helice",
  terra: false,
  selected: null,
  hover: null,
  expanded: new Set(),
  search: "",
  cam: { yaw: 0.6, pitch: 0.38, dist: 9, tx: 0, ty: 0, tz: 0 },
  auto: true,
  scene: null,
  projected: [],
};

try {
  const v = localStorage.getItem("kaltsit.visor.vista");
  if (["santuario", "helice", "arboleda", "constelacion"].includes(v)) st.view = v;
} catch {}

const cv = $("cv");
const ctx = cv.getContext("2d");

function colorOf(n) {
  if (n.terra) return "#8f8a7a";
  return MOD_COLORS[n.m] || EXTRA_COLOR;
}

function rgba(hex, a) {
  const v = parseInt(hex.slice(1), 16);
  return `rgba(${(v >> 16) & 255},${(v >> 8) & 255},${v & 255},${a.toFixed(3)})`;
}

// ── language: it comes with the state of the app and can change while this window is open ──

function paintStamp() {
  if (!st.stamp) return;
  $("stamp").textContent = t("visor.stamp", { time: st.stamp.toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" }) });
}

let langReady = false;
function useLang(code) {
  if (langReady && code === getLang()) return;
  langReady = true;
  setLang(code);
  applyI18n(document);
  document.title = t("visor.title");
  paintStamp();
  if (st.g) {
    build();
    if (st.selected != null) showDetail(st.selected);
  }
}
invoke("state").then((s) => useLang(s.idioma)).catch(() => useLang("es"));
listen("k:state", (e) => { if (e.payload && e.payload.idioma) useLang(e.payload.idioma); });

// ── data ──

async function load() {
  const g = await invoke("memory_graph");
  st.g = g;
  st.byId = new Map(g.nodes.map((n) => [n.id, n]));
  st.adj = new Map();
  for (const e of g.edges) {
    if (!st.adj.has(e.f)) st.adj.set(e.f, []);
    if (!st.adj.has(e.t)) st.adj.set(e.t, []);
    st.adj.get(e.f).push({ o: e.t, e, out: true });
    st.adj.get(e.t).push({ o: e.f, e, out: false });
  }
  st.stamp = new Date();
  paintStamp();
  if (st.selected != null && !st.byId.has(st.selected)) st.selected = null;
  build();
  if (st.selected != null) showDetail(st.selected);
}

// ── layouts ──

function santuario() {
  const all = st.g.nodes;
  const core = all.filter((n) => n.core);
  const rules = all.filter((n) => !n.core && !n.terra && n.m === "yo" && (n.anch || n.k === "rasgo"));
  const cons = all.filter((n) => !n.core && !n.terra && n.cons && !rules.includes(n));
  const pts = new Map();
  const deco = [];
  const ring = (list, r, y) => {
    const sorted = [...list].sort((a, b) => (a.m + a.k).localeCompare(b.m + b.k) || b.deg - a.deg);
    sorted.forEach((n, i) => {
      const a = (i / Math.max(1, sorted.length)) * Math.PI * 2;
      pts.set(n.id, [Math.cos(a) * r, y, Math.sin(a) * r]);
      deco.push({ line: [[Math.cos(a) * r, y, Math.sin(a) * r], [Math.cos(a) * r, -1.4, Math.sin(a) * r]], c: "rgba(159,210,74,0.10)" });
    });
    deco.push({ circle: [r, -1.4], c: "rgba(141,151,143,0.22)" });
  };
  // Each ring wide enough for its names.
  const r0 = Math.max(1.4, core.length * 0.24);
  const r1 = Math.max(r0 + 1.1, rules.length * 0.22);
  const r2 = Math.max(r1 + 1.1, cons.length * 0.2);
  ring(core, r0, 0.9);
  ring(rules, r1, 0.3);
  ring(cons, r2, -0.3);
  deco.push({ circle: [0.35, 0.9], c: "rgba(159,210,74,0.5)" });
  const empty = core.length + rules.length + cons.length === 0 ? t("visor.empty.santuario") : "";
  return { pts, deco, empty, center: [0, 0.2, 0] };
}

function helice() {
  const all = st.g.nodes;
  const lived = all
    .filter((n) => !n.terra && n.u != null && (n.k === "episodio" || n.k === "evento" || n.dated || n.sched))
    .sort((a, b) => a.u - b.u || a.id - b.id);
  const pts = new Map();
  const deco = [];
  const H = 5.2;
  const R = 2.3;
  const n = lived.length;
  if (n) {
    const t0 = lived[0].u;
    const t1 = lived[n - 1].u;
    const span = Math.max(1, t1 - t0);
    let nowY = null;
    const spiral = [];
    lived.forEach((node, i) => {
      // Mostly by order (so bursts of activity spread out), a little by real time.
      const h = 0.75 * (n > 1 ? i / (n - 1) : 0.5) + 0.25 * ((node.u - t0) / span);
      const y = -H / 2 + H * h;
      const a = h * Math.PI * 2 * Math.max(2, Math.min(7, n / 18));
      const p = [Math.cos(a) * R, y, Math.sin(a) * R];
      pts.set(node.id, p);
      spiral.push(p);
      if (nowY == null && node.u > st.g.now) nowY = y - 0.04;
    });
    for (let i = 1; i < spiral.length; i++) deco.push({ line: [spiral[i - 1], spiral[i]], c: "rgba(63,214,200,0.22)" });
    if (nowY != null) deco.push({ circle: [R + 0.35, nowY], c: "rgba(227,180,90,0.55)", label: t("visor.now") });
    // A few dates along the coil.
    const step = Math.max(1, Math.round(n / 8));
    for (let i = 0; i < n; i += step) deco.push({ text: dateOf(lived[i].u), at: pts.get(lived[i].id), dx: 10 });
  }
  if (st.terra) {
    const terra = all.filter((x) => x.terra).sort((a, b) => b.deg - a.deg).slice(0, 220);
    terra.forEach((node, i) => {
      const h = i / Math.max(1, terra.length - 1);
      const a = h * Math.PI * 2 * 5;
      pts.set(node.id, [Math.cos(a) * (R + 1.1), -H / 2 - 1.6 + 1.3 * h, Math.sin(a) * (R + 1.1)]);
    });
    if (terra.length) deco.push({ text: "Terra", at: [R + 1.6, -H / 2 - 1.0, 0], dx: 0 });
  }
  return { pts, deco, empty: n ? "" : t("visor.empty.helice"), center: [0, 0, 0] };
}

function arboleda() {
  const all = st.g.nodes.filter((n) => CONCEPT_KINDS.has(n.k) && !n.terra || (n.terra && st.terra && CONCEPT_KINDS.has(n.k)));
  const isConcept = new Set(all.map((n) => n.id));
  const pts = new Map();
  const deco = [];
  const trunkKinds = new Set(["persona", "objeto", "lugar", "rasgo"]);
  let trunks = all.filter((n) => trunkKinds.has(n.k)).sort((a, b) => b.deg - a.deg).slice(0, 12);
  if (trunks.length < 4) trunks = all.slice().sort((a, b) => b.deg - a.deg).slice(0, 12);
  const R = 3.2;
  trunks.forEach((n, i) => {
    const a = (i / Math.max(1, trunks.length)) * Math.PI * 2;
    pts.set(n.id, [Math.cos(a) * R, 1.0, Math.sin(a) * R]);
    deco.push({ line: [[Math.cos(a) * R, 1.0, Math.sin(a) * R], [Math.cos(a) * R, -0.6, Math.sin(a) * R]], c: "rgba(127,209,139,0.35)", w: 3 });
  });
  deco.push({ circle: [R, -0.6], c: "rgba(141,151,143,0.18)" });
  // Roots: what a node is linked to, below it, fanning out from the centre.
  const grow = (id, depth) => {
    if (!st.expanded.has(id) || depth > 5) return;
    const p = pts.get(id);
    const kids = (st.adj.get(id) || [])
      .filter((x) => isConcept.has(x.o) && !pts.has(x.o))
      .sort((a, b) => (st.byId.get(b.o).deg - st.byId.get(a.o).deg))
      .slice(0, 14);
    const base = Math.atan2(p[2], p[0]);
    // Fanned out away from the centre, wider the more roots there are, in two tiers so their
    // names do not sit on top of each other.
    const spread = Math.min(2.6, 0.32 * kids.length);
    const reach = (1.2 + 0.07 * kids.length) / (1 + depth * 0.35);
    kids.forEach((x, j) => {
      const a = base + (kids.length > 1 ? (j / (kids.length - 1) - 0.5) * spread : 0);
      const d = reach * (j % 2 ? 1.25 : 0.9);
      pts.set(x.o, [p[0] + Math.cos(a) * d, p[1] - (j % 2 ? 1.15 : 0.75), p[2] + Math.sin(a) * d]);
    });
    kids.forEach((x) => grow(x.o, depth + 1));
  };
  for (const t of trunks) grow(t.id, 0);
  // Badges: how many roots a node still hides.
  for (const [id, p] of pts) {
    if (st.expanded.has(id)) continue;
    const hidden = (st.adj.get(id) || []).filter((x) => isConcept.has(x.o) && !pts.has(x.o)).length;
    if (hidden) deco.push({ text: `+${hidden}`, at: p, dx: 9, dy: 12, c: "rgba(141,151,143,0.8)" });
  }
  return { pts, deco, empty: trunks.length ? "" : t("visor.empty.arboleda"), center: [0, 0.2, 0] };
}

// ── the constellation: all of her memory at once ──
//
// The core in the middle, the macro-sectors around it as galaxies, their sectors and subsectors as
// clusters, and every memory a star by the sector it belongs to the most. Seen from afar, stars take
// their module's colour; close to a sector, the stars near the centre of the view split by kind.
// How bright and big a star is says how much it matters (belief, links, activation, recalls), so
// what barely counts fades out. Positions are seeded by id: the same memory stays in its place.

const KIND_COLORS = {
  persona: "#f3c06b", lugar: "#5fc9a8", evento: "#f08a5d", rasgo: "#9fd24a", objeto: "#6fb7e8",
  hecho: "#b59bff", episodio: "#e7d9a8", apariencia: "#e88fc4",
};
const SHADOW_COLOR = "#e0605a";
const SECTOR_COLOR = "#dfe6da";
/// Closer than this the stars near the centre of the view take their kind's colour.
const ZOOM_KIND = 9;

function seeded(id) {
  let a = (id * 2654435761) >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/// `n` points spread evenly over a sphere (golden spiral), turned by `turn`.
function sphereDirs(n, turn) {
  const out = [];
  for (let i = 0; i < n; i++) {
    const y = n === 1 ? 0 : 1 - (2 * (i + 0.5)) / n;
    const r = Math.sqrt(Math.max(0, 1 - y * y));
    const a = i * 2.399963 + turn;
    out.push([Math.cos(a) * r, y * 0.75, Math.sin(a) * r]);
  }
  return out;
}

const add3 = (a, b, k = 1) => [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k];
const norm3 = (v) => {
  const l = Math.hypot(v[0], v[1], v[2]) || 1;
  return [v[0] / l, v[1] / l, v[2] / l];
};

function mix(a, b, k) {
  const pa = parseInt(a.slice(1), 16), pb = parseInt(b.slice(1), 16);
  const ch = (s) => Math.round(((pa >> s) & 255) * (1 - k) + ((pb >> s) & 255) * k);
  return "#" + ((ch(16) << 16) | (ch(8) << 8) | ch(0)).toString(16).padStart(6, "0");
}

function constelacion() {
  const all = st.g.nodes;
  const isSector = new Set(all.filter((n) => n.k === "sector").map((n) => n.id));
  // The sector each node belongs to the most.
  const home = new Map();
  for (const e of st.g.edges) {
    if (e.k !== "pertenece" || !isSector.has(e.t) || e.f === e.t) continue;
    const b = home.get(e.f);
    if (!b || e.w > b.w) home.set(e.f, { s: e.t, w: e.w });
  }
  const kids = new Map();
  const members = new Map();
  for (const n of all) {
    const h = home.get(n.id);
    if (!h) continue;
    const into = isSector.has(n.id) ? kids : members;
    if (!into.has(h.s)) into.set(h.s, []);
    into.get(h.s).push(n);
  }
  const pts = new Map();
  const deco = [];
  const sectorPos = new Map();
  const sectorSize = new Map();
  const roots = all.filter((n) => isSector.has(n.id) && !home.has(n.id));
  // Clusters as wide as what hangs from them (members and subsectors, all the way down).
  const weight = (id) => (members.get(id) || []).length + (kids.get(id) || []).reduce((s, k) => s + weight(k.id), 0);
  const place = (s, pos, depth, out) => {
    pts.set(s.id, pos);
    sectorPos.set(s.id, pos);
    const w = weight(s.id);
    sectorSize.set(s.id, w);
    if (depth === 1) deco.push({ text: s.l, at: pos, dx: 12, dy: -10, c: "rgba(230,233,227,0.85)", font: "600 13px Segoe UI" });
    else if (depth >= 2) deco.push({ text: s.l, at: pos, dx: 9, dy: -7, c: "rgba(223,230,218,0.6)", minF: 60 + 25 * depth });
    const children = (kids.get(s.id) || []).slice().sort((a, b) => weight(b.id) - weight(a.id) || a.id - b.id);
    const dirs = sphereDirs(children.length, s.id * 0.7);
    const reach = depth === 0 ? 6.5 : Math.max(0.9, (depth === 1 ? 2.6 : 1.5) * Math.pow(0.6, depth - 1)) * (0.7 + 0.3 * Math.min(1, Math.sqrt(w / 40)));
    children.forEach((c, i) => {
      // Away from the centre, so a galaxy's clusters do not fall back into the core.
      const d = depth === 0 ? dirs[i] : norm3(add3(dirs[i], out, 1.1));
      place(c, add3(pos, d, reach), depth + 1, depth === 0 ? d : out);
    });
  };
  roots.forEach((r, i) => place(r, i === 0 ? [0, 0, 0] : [0, -3 - i * 1.5, 0], 0, [0, 1, 0]));
  // Stars: a flattened ball around their sector, denser in the middle.
  for (const [s, list] of members) {
    const c = sectorPos.get(s);
    if (!c) continue;
    const rc = 0.35 + 0.15 * Math.sqrt(list.length);
    for (const n of list) {
      if (n.terra && !st.terra) continue;
      const rnd = seeded(n.id);
      const u = Math.pow(rnd(), 0.6), a = rnd() * Math.PI * 2, y = rnd() * 2 - 1;
      const r = rc * u, ring = Math.sqrt(1 - y * y);
      pts.set(n.id, [c[0] + Math.cos(a) * ring * r, c[1] + y * r * 0.45, c[2] + Math.sin(a) * ring * r]);
    }
  }
  // What hangs from no sector: a halo by module, and Terra as a far nebula.
  const loose = all.filter((n) => !isSector.has(n.id) && !pts.has(n.id) && (!n.terra || st.terra));
  const mods = (st.g.modules || []).map((m) => m.name);
  for (const n of loose) {
    const rnd = seeded(n.id);
    if (n.terra) {
      const u = Math.cbrt(rnd()), a = rnd() * Math.PI * 2, y = rnd() * 2 - 1, ring = Math.sqrt(1 - y * y);
      pts.set(n.id, [11 + Math.cos(a) * ring * 2.6 * u, -3 + y * 1.4 * u, -7 + Math.sin(a) * ring * 2.6 * u]);
      continue;
    }
    const k = Math.max(0, mods.indexOf(n.m));
    const a = (k / Math.max(1, mods.length)) * Math.PI * 2 + rnd() * 1.2;
    const r = 10 + rnd() * 1.5;
    pts.set(n.id, [Math.cos(a) * r, (rnd() - 0.5) * 2, Math.sin(a) * r]);
  }
  if (st.terra && loose.some((n) => n.terra)) deco.push({ text: "Terra", at: [11, -0.9, -7], dx: 0, c: "rgba(143,138,122,0.9)", font: "600 13px Segoe UI" });

  // How much each memory matters, 0..1: belief, links, activation, recalls; the core always.
  const maxDeg = Math.max(1, ...all.map((n) => n.deg));
  const maxRec = Math.max(1, ...all.map((n) => n.rec || 0));
  const weightOf = new Map();
  for (const n of all) {
    const chi = (Math.max(-1, Math.min(1, n.chi)) + 1) / 2;
    const imp = n.core ? 1 : 0.35 * chi + 0.3 * Math.log1p(n.deg) / Math.log1p(maxDeg) + 0.2 * Math.min(1, n.act) + 0.15 * Math.log1p(n.rec || 0) / Math.log1p(maxRec);
    weightOf.set(n.id, Math.max(0, Math.min(1, imp)));
  }

  const style = (n, p3, p) => {
    if (isSector.has(n.id)) {
      // An empty sector is a faint ring; a full one, a bright and wider one.
      const size = sectorSize.get(n.id) || 0;
      const big = Math.sqrt(Math.min(size, 400) / 400);
      return { col: SECTOR_COLOR, alpha: size ? 0.75 : 0.25, r: Math.min(22, Math.max(2.5, (3 + 5 * big) * (p.f / 90))), ring: true };
    }
    const imp = weightOf.get(n.id);
    const base = n.terra ? "#8f8a7a" : n.chi < 0 ? SHADOW_COLOR : MOD_COLORS[n.m] || EXTRA_COLOR;
    // Close to where the camera looks, the module's colour gives way to the kind's.
    const c = st.cam;
    const near = Math.hypot(p3[0] - c.tx, p3[1] - c.ty, p3[2] - c.tz);
    const k = Math.max(0, Math.min(1, (ZOOM_KIND - c.dist) / (ZOOM_KIND * 0.4))) * Math.max(0, Math.min(1, 1.4 - near / 2.2));
    const col = k > 0 && !n.terra && n.chi >= 0 ? mix(base, KIND_COLORS[n.k] || base, k) : base;
    const alpha = (n.chi < 0 ? 0.5 : 1) * (0.12 + 0.88 * imp) * (n.terra ? 0.5 : 1);
    // Up close they grow, but not into discs that hide each other.
    return { col, alpha, r: Math.min(3 + 6 * imp, Math.max(1.3, (1.4 + 3.6 * imp) * (p.f / 90))) };
  };
  const label = (n, p) => isSector.has(n.id) ? false : weightOf.get(n.id) > 0.72 && p.f > 150;
  const empty = all.length ? "" : t("visor.empty.constelacion");
  return { pts, deco, empty, center: [0, 0, 0], style, label, hoverEdges: true, edgeKinds: new Set(["causa", "relacion", "contradice", "precede"]) };
}

function build() {
  if (!st.g) return;
  const lay = st.view === "santuario" ? santuario() : st.view === "arboleda" ? arboleda() : st.view === "constelacion" ? constelacion() : helice();
  const edges = st.g.edges.filter((e) => lay.pts.has(e.f) && lay.pts.has(e.t) && (!lay.edgeKinds || lay.edgeKinds.has(e.k)));
  st.scene = { ...lay, edges };
  $("empty").textContent = lay.empty;
  $("empty").classList.toggle("hidden", !lay.empty);
  $("terraBox").classList.toggle("hidden", st.view === "santuario");
  $("help").textContent = t(st.view === "constelacion" ? "visor.help.constelacion" : "visor.help");
  for (const b of document.querySelectorAll("#views button")) b.classList.toggle("on", b.dataset.v === st.view);
  legend();
}

function legend() {
  const box = $("legend");
  box.textContent = "";
  const mods = (st.g ? st.g.modules : []).map((m) => m.name);
  for (const m of mods) {
    const s = document.createElement("span");
    const i = document.createElement("i");
    i.style.background = MOD_COLORS[m] || EXTRA_COLOR;
    s.append(i, modTitle(m));
    box.append(s);
  }
  const chi = document.createElement("span");
  chi.textContent = t(st.view === "constelacion" ? "visor.legend.constelacion" : "visor.legend.chi");
  box.append(chi);
}

function dateOf(u) {
  return new Date(u * 1000).toLocaleDateString(locale(), { day: "numeric", month: "short" });
}

// ── camera and drawing ──

function resize() {
  const r = cv.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  cv.width = Math.round(r.width * dpr);
  cv.height = Math.round(r.height * dpr);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
}

function project(p, w, h) {
  const c = st.cam;
  let x = p[0] - c.tx, y = p[1] - c.ty, z = p[2] - c.tz;
  const cy = Math.cos(c.yaw), sy = Math.sin(c.yaw);
  [x, z] = [x * cy - z * sy, x * sy + z * cy];
  const cp = Math.cos(c.pitch), sp = Math.sin(c.pitch);
  [y, z] = [y * cp - z * sp, y * sp + z * cp];
  const depth = z + c.dist;
  if (depth < 0.2) return null;
  const f = (Math.min(w, h) * 0.9) / depth;
  return { x: w / 2 + x * f, y: h / 2 - y * f, d: depth, f };
}

function matches(n) {
  if (!st.search) return false;
  const q = st.search.toLowerCase();
  return n.l.toLowerCase().includes(q) || n.t.toLowerCase().includes(q);
}

function draw() {
  const w = cv.clientWidth, h = cv.clientHeight;
  ctx.clearRect(0, 0, w, h);
  const sc = st.scene;
  if (!sc) return;
  if (st.auto && !drag) st.cam.yaw += 0.0012;
  // In the constellation the hovered star lights its links too.
  const lit = st.selected ?? (sc.hoverEdges ? st.hover : null);
  const focus = lit != null ? new Set([lit, ...(st.adj.get(lit) || []).map((x) => x.o)]) : null;
  // Named around the chosen one: its real links, not everything sharing its sector (a sector
  // chosen in the constellation would name all of its members at once).
  const named = lit != null && sc.hoverEdges
    ? new Set([lit, ...(st.adj.get(lit) || []).filter((x) => x.e.k !== "pertenece").map((x) => x.o)])
    : focus;
  const searching = !!st.search;

  // Decoration: floors, columns, the coil.
  for (const d of sc.deco) {
    if (d.circle) {
      const [r, y] = d.circle;
      ctx.beginPath();
      let first = true;
      for (let i = 0; i <= 64; i++) {
        const a = (i / 64) * Math.PI * 2;
        const p = project([Math.cos(a) * r, y, Math.sin(a) * r], w, h);
        if (!p) { first = true; continue; }
        if (first) ctx.moveTo(p.x, p.y); else ctx.lineTo(p.x, p.y);
        first = false;
      }
      ctx.strokeStyle = d.c;
      ctx.lineWidth = 1;
      ctx.stroke();
      if (d.label) {
        const p = project([r, y, 0], w, h);
        if (p) { ctx.fillStyle = d.c; ctx.font = "11px Segoe UI"; ctx.fillText(d.label, p.x + 6, p.y - 4); }
      }
    } else if (d.line) {
      const a = project(d.line[0], w, h), b = project(d.line[1], w, h);
      if (!a || !b) continue;
      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(b.x, b.y);
      ctx.strokeStyle = d.c;
      ctx.lineWidth = d.w || 1;
      ctx.stroke();
    } else if (d.text) {
      const p = project(d.at, w, h);
      // `minF`: only once the camera is this close (a subsector's name, say).
      if (!p || (d.minF && p.f < d.minF)) continue;
      ctx.fillStyle = d.c || "rgba(93,104,98,0.9)";
      ctx.font = d.font || "10px Cascadia Mono, Consolas, monospace";
      ctx.fillText(d.text, p.x + (d.dx || 0), p.y + (d.dy || 0));
    }
  }

  // Edges.
  for (const e of sc.edges) {
    const a = project(sc.pts.get(e.f), w, h), b = project(sc.pts.get(e.t), w, h);
    if (!a || !b) continue;
    const on = focus ? focus.has(e.f) && focus.has(e.t) && (e.f === lit || e.t === lit) : !searching && !sc.hoverEdges;
    // The constellation's links barely show until a star is hovered or chosen.
    if (sc.hoverEdges && !on && focus) continue;
    ctx.beginPath();
    ctx.moveTo(a.x, a.y);
    ctx.lineTo(b.x, b.y);
    ctx.strokeStyle = rgba(EDGE_COLORS[e.k] || "#5d6862", on ? 0.25 + 0.5 * Math.min(1, e.w + 0.2) : sc.hoverEdges ? 0.025 : 0.05);
    ctx.lineWidth = on && focus ? 1.6 : 1;
    ctx.stroke();
  }

  // Nodes, far to near.
  const list = [];
  for (const [id, p3] of sc.pts) {
    const p = project(p3, w, h);
    if (p) list.push({ id, p });
  }
  list.sort((a, b) => b.p.d - a.p.d);
  st.projected = [];
  for (const { id, p } of list) {
    const n = st.byId.get(id);
    let r, alpha, col, ring = false;
    if (sc.style) {
      ({ r, alpha, col, ring = false } = sc.style(n, sc.pts.get(id), p));
      if (id === st.selected) r *= 1.6;
    } else {
      const base = 2.2 + 4.2 * Math.sqrt(Math.min(n.deg, 24) / 24) + 2.5 * Math.min(1, n.act);
      r = Math.max(1.5, base * (p.f / 90)) * (id === st.selected ? 1.5 : 1);
      const belief = 0.25 + 0.75 * ((Math.max(-1, Math.min(1, n.chi)) + 1) / 2);
      alpha = n.terra ? belief * 0.45 : belief;
      col = colorOf(n);
    }
    if (focus && !focus.has(id)) alpha *= 0.18;
    if (searching && !matches(n)) alpha *= 0.15;
    if (n.core || id === st.selected || (searching && matches(n))) {
      ctx.beginPath();
      ctx.arc(p.x, p.y, r * 2.4, 0, Math.PI * 2);
      ctx.fillStyle = rgba(col, alpha * 0.18);
      ctx.fill();
    }
    ctx.beginPath();
    ctx.arc(p.x, p.y, r, 0, Math.PI * 2);
    if (ring || (n.sched && n.u > st.g.now)) {
      ctx.strokeStyle = rgba(col, alpha);
      ctx.lineWidth = 1.5;
      ctx.stroke();
    } else {
      ctx.fillStyle = rgba(col, alpha);
      ctx.fill();
    }
    st.projected.push({ id, x: p.x, y: p.y, r: Math.max(r, 6) });
  }

  // Labels: the near and important ones, the hovered, the selected and its neighbours.
  ctx.font = "12px Segoe UI";
  const labelled = list
    .map(({ id, p }) => ({ id, p, n: st.byId.get(id) }))
    .filter(({ id, n, p }) => id === st.hover || id === st.selected || (named && named.has(id) && (!sc.hoverEdges || st.selected != null)) || (searching && matches(n)) || (!focus && !searching && (sc.label ? sc.label(n, p) : (n.core || n.deg >= 4) && p.f > 70)))
    .slice(-60);
  for (const { id, p, n } of labelled) {
    const text = n.l.length > 42 ? n.l.slice(0, 40) + "…" : n.l;
    ctx.fillStyle = id === st.selected || id === st.hover ? "#e6e9e3" : "rgba(230,233,227,0.72)";
    ctx.fillText(text, p.x + 8, p.y - 6);
  }
}

function frame() {
  draw();
  requestAnimationFrame(frame);
}

// ── detail ──

function showDetail(id) {
  const n = st.byId.get(id);
  if (!n) return;
  const box = $("detailBody");
  box.textContent = "";
  const h = document.createElement("h2");
  h.textContent = n.l;
  const k = document.createElement("div");
  k.className = "k";
  const flags = [n.core && t("visor.flag.core"), n.anch && t("visor.flag.anch"), n.cons && t("visor.flag.cons"), n.sched && t("visor.flag.sched")].filter(Boolean).join(" · ");
  k.textContent = `${n.k} · ${modTitle(n.m)} · ${n.o}${flags ? " · " + flags : ""} · ${n.fecha}`;
  const chi = document.createElement("div");
  chi.className = "chi";
  const dot = document.createElement("i");
  dot.style.left = `${((n.chi + 1) / 2) * 100}%`;
  dot.style.background = n.chi > 0.25 ? "#9fd24a" : n.chi < -0.25 ? "#e0605a" : "#e3b45a";
  chi.append(dot);
  const chiText = document.createElement("div");
  chiText.className = "o";
  chiText.textContent = `χ ${n.chi.toFixed(2)} · ${n.chi > 0.75 ? t("visor.chi.sure") : n.chi > 0.25 ? t("visor.chi.guess") : n.chi > -0.25 ? t("visor.chi.unsure") : t("visor.chi.none")} · ${t("visor.recalled", { n: n.rec })}`;
  const text = document.createElement("div");
  text.className = "t";
  text.textContent = n.t === n.l ? "" : n.t;
  const o = document.createElement("div");
  o.className = "o";
  o.textContent = `${n.model || "?"} · ${n.host || "?"}`;
  box.append(h, k, chi, chiText, text, o);
  const links = st.adj.get(id) || [];
  const groups = new Map();
  for (const x of links) {
    const key = x.e.k === "causa" ? (x.out ? t("visor.edge.causaOut") : t("visor.edge.causaIn")) : edgeName(x.e.k);
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(x.o);
  }
  for (const [g, ids] of groups) {
    const h4 = document.createElement("h4");
    h4.textContent = `${g} (${ids.length})`;
    box.append(h4);
    for (const other of ids.slice(0, 30)) {
      const m = st.byId.get(other);
      if (!m) continue;
      const a = document.createElement("a");
      const s = document.createElement("small");
      s.textContent = m.k;
      a.append(s, m.l);
      a.onclick = () => select(other, true);
      box.append(a);
    }
  }
  $("detail").classList.remove("hidden");
}

function select(id, center) {
  st.selected = id;
  st.auto = false;
  if (st.view === "arboleda" && st.scene && !st.scene.pts.has(id)) {
    // Not on screen in the grove: unfold the way to it from a linked tree.
    for (const x of st.adj.get(id) || []) if (st.scene.pts.has(x.o)) st.expanded.add(x.o);
    build();
  }
  if (center && st.scene && st.scene.pts.has(id)) {
    const p = st.scene.pts.get(id);
    [st.cam.tx, st.cam.ty, st.cam.tz] = p;
    st.cam.dist = Math.min(st.cam.dist, 6);
  }
  showDetail(id);
}

// ── input ──

let drag = null;

function hit(x, y) {
  let best = null;
  for (const p of st.projected) {
    const d = Math.hypot(p.x - x, p.y - y);
    if (d <= p.r + 3 && (!best || d < best.d)) best = { id: p.id, d };
  }
  return best && best.id;
}

cv.addEventListener("mousedown", (e) => {
  drag = { x: e.clientX, y: e.clientY, moved: false };
  cv.classList.add("drag");
});
window.addEventListener("mouseup", (e) => {
  cv.classList.remove("drag");
  if (drag && !drag.moved) {
    const r = cv.getBoundingClientRect();
    const id = hit(e.clientX - r.left, e.clientY - r.top);
    if (id != null) {
      if (st.view === "arboleda") {
        if (st.expanded.has(id)) st.expanded.delete(id);
        else st.expanded.add(id);
        build();
      }
      select(id, false);
    } else if (e.target === cv) {
      st.selected = null;
      $("detail").classList.add("hidden");
    }
  }
  drag = null;
});
window.addEventListener("mousemove", (e) => {
  const r = cv.getBoundingClientRect();
  if (drag) {
    const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
    if (Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
    st.cam.yaw += dx * 0.006;
    st.cam.pitch = Math.max(-1.3, Math.min(1.3, st.cam.pitch + dy * 0.005));
    drag.x = e.clientX;
    drag.y = e.clientY;
    st.auto = false;
    return;
  }
  const id = hit(e.clientX - r.left, e.clientY - r.top);
  st.hover = id;
  cv.classList.toggle("point", id != null);
});
cv.addEventListener("wheel", (e) => {
  e.preventDefault();
  st.cam.dist = Math.max(st.view === "constelacion" ? 1.2 : 2.5, Math.min(st.view === "constelacion" ? 40 : 24, st.cam.dist * (e.deltaY > 0 ? 1.1 : 0.9)));
}, { passive: false });
// A double click goes there: close to that sector or memory.
cv.addEventListener("dblclick", (e) => {
  const r = cv.getBoundingClientRect();
  const id = hit(e.clientX - r.left, e.clientY - r.top);
  if (id == null) return;
  select(id, true);
  if (st.view === "constelacion") st.cam.dist = st.byId.get(id)?.k === "sector" ? 5 : 3;
});

function resetCamera() {
  const c = st.scene ? st.scene.center : [0, 0, 0];
  const dist = st.view === "helice" ? 10 : st.view === "constelacion" ? 15 : 9;
  Object.assign(st.cam, { yaw: 0.6, pitch: st.view === "helice" ? 0.25 : st.view === "constelacion" ? 0.5 : 0.42, dist, tx: c[0], ty: c[1], tz: c[2] });
  st.auto = true;
}

for (const b of document.querySelectorAll("#views button")) {
  b.onclick = () => {
    st.view = b.dataset.v;
    try { localStorage.setItem("kaltsit.visor.vista", st.view); } catch {}
    build();
    resetCamera();
  };
}
$("terra").onchange = (e) => { st.terra = e.target.checked; build(); };
$("reset").onclick = resetCamera;
$("detailClose").onclick = () => { st.selected = null; $("detail").classList.add("hidden"); };
$("search").addEventListener("input", (e) => { st.search = e.target.value.trim(); });
$("search").addEventListener("keydown", (e) => {
  if (e.key !== "Enter" || !st.g) return;
  const found = st.g.nodes.find((n) => matches(n) && st.scene.pts.has(n.id)) || st.g.nodes.find((n) => matches(n));
  if (found) select(found.id, true);
});
window.addEventListener("resize", resize);

// She learned something: the snapshot is taken again (a second later, if several come at once).
let reloadTimer = null;
listen("k:memoria", () => {
  clearTimeout(reloadTimer);
  reloadTimer = setTimeout(() => load().catch(() => {}), 1000);
});

resize();
load()
  .then(() => { resetCamera(); requestAnimationFrame(frame); })
  .catch((e) => { $("empty").textContent = t("visor.error", { e }); $("empty").classList.remove("hidden"); });
