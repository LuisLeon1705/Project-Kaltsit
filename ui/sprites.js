// A pixel-art body: frames of each animation in a strip (ui/personajes/<id>/<Name>.png) and
// cuerpo.json with their sizes, durations, where her feet and her mouth are in each frame. The
// same face as the Spine chibi (ready, play, face, speak), so the overlay drives both alike.
// Drawn at its own size, pixel for pixel; the canvases of the animations differ, so each frame is
// placed by her feet at the bottom centre. Her mouth opens with her voice: a drawn mouth at the
// rig's mouth point of the frame, half open or open, like the chibi's.
"use strict";

class SpriteBody {
  constructor(canvas, id) {
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d");
    this.dir = `personajes/${id}/`;
    this.anim = null;
    this.flip = false;
    this.level = 0;
    this.mouth = { open: 0, peak: 0.02, shown: 0 };
    this.ready = fetch(this.dir + "cuerpo.json")
      .then((r) => r.json())
      .then(async (body) => {
        this.body = body;
        this.sheets = {};
        await Promise.all(Object.entries(body.animaciones).filter(([, a]) => !a.igual).map(([name]) => new Promise((ok, fail) => {
          const img = new Image();
          img.onload = () => { this.sheets[name] = img; ok(); };
          img.onerror = fail;
          img.src = `${this.dir}${name}.png`;
        })));
        this.animations = Object.keys(body.animaciones);
        // As tall as her tallest frame, and wide enough for the widest.
        const all = Object.values(body.animaciones).filter((a) => !a.igual);
        this.h = Math.max(...all.map((a) => a.h));
        this.w = Math.max(...all.map((a) => a.w));
        this.play("Relax");
        this.last = performance.now();
        requestAnimationFrame((t) => this._frame(t));
        return this;
      });
  }

  /// Loops an animation; one-shots fall back to `then` (Sit and Sleep hold their last frame).
  play(name, loop = true, then = "Relax") {
    if (!this.body || !this.body.animaciones[name]) return;
    if (name === this.anim && loop) return;
    let a = this.body.animaciones[name];
    let hold = false;
    if (a.igual) {
      hold = a.fijo === "ultimo";
      name = a.igual;
      a = this.body.animaciones[name];
    }
    this.anim = name;
    this.state = { a, sheet: this.sheets[name], i: hold ? a.duraciones.length - 1 : 0, t: 0, loop: a.bucle && loop, hold, then: a.bucle || name === "Sit" ? null : then };
  }

  face(left) {
    this.flip = left;
  }

  speak(level) {
    this.level = level || 0;
  }

  _talk(dt) {
    const m = this.mouth;
    const rms = this.level;
    const f = dt * 30;
    m.peak = Math.max(m.peak * Math.pow(0.999, f), rms);
    const target = Math.min(1, Math.max(0, (rms / m.peak - 0.15) / 0.6));
    m.open += (target - m.open) * (1 - Math.pow(target > m.open ? 0.4 : 0.75, f));
    const up = [0.22, 0.6];
    const down = [0.12, 0.45];
    if (m.shown < 2 && m.open > up[m.shown]) m.shown++;
    else if (m.shown > 0 && m.open < down[m.shown - 1]) m.shown--;
  }

  _frame(t) {
    const dt = Math.min(0.1, (t - this.last) / 1000);
    this.last = t;
    const s = this.state;
    if (s && !s.hold) {
      s.t += dt * 1000;
      while (s.t >= s.a.duraciones[s.i]) {
        s.t -= s.a.duraciones[s.i];
        if (s.i + 1 < s.a.duraciones.length) s.i++;
        else if (s.loop) s.i = 0;
        else if (s.then) { this.play(s.then); break; }
        else { s.hold = true; break; }
      }
    }
    this._talk(dt);
    const c = this.canvas;
    const dpr = window.devicePixelRatio || 1;
    c.style.width = `${this.w}px`;
    c.style.height = `${this.h}px`;
    c.width = Math.round(this.w * dpr);
    c.height = Math.round(this.h * dpr);
    const g = this.ctx;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.imageSmoothingEnabled = false;
    g.clearRect(0, 0, this.w, this.h);
    const st = this.state;
    if (st) {
      const a = st.a;
      // Her feet at the bottom centre, whichever canvas this animation has.
      const x0 = Math.round(this.w / 2 - (this.flip ? a.w - a.pies[0] : a.pies[0]));
      const y0 = this.h - a.pies[1];
      g.save();
      if (this.flip) {
        g.translate(x0 + a.w, y0);
        g.scale(-1, 1);
      } else {
        g.translate(x0, y0);
      }
      g.drawImage(st.sheet, st.i * a.w, 0, a.w, a.h, 0, 0, a.w, a.h);
      // An animation without mouth points (asleep) has no mouth to open.
      const at = a.bocas[st.i] || a.bocas[0];
      if (this.mouth.shown > 0 && at) this._mouth(g, at, this.mouth.shown);
      g.restore();
    }
    requestAnimationFrame((tt) => this._frame(tt));
  }

  /// An open mouth over the drawn one, in her own pixels: 2 px tall half open, 3 px open.
  _mouth(g, [mx, my], shown) {
    g.fillStyle = "rgba(0,0,0,0.6)";
    const w = shown === 2 ? 4 : 3;
    const h = shown === 2 ? 3 : 2;
    g.fillRect(mx - Math.floor(w / 2) - 1, my - 1, w + 2, h + 1);
    g.fillStyle = "#3a1618";
    g.fillRect(mx - Math.floor(w / 2), my, w, h);
    if (shown === 2) {
      g.fillStyle = "#c9686d";
      g.fillRect(mx - 1, my + h - 1, 2, 1);
    }
  }
}
