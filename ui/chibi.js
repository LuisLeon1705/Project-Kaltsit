// Kal'tsit's chibi: the RIIC building model (Spine 3.8) drawn on a transparent canvas.
// Downloaded on demand from isHarryh/Ark-Models into modelos/spine/003_kalts/.
"use strict";

const SPINE_DIR = "spine/003_kalts/";
const SKEL = SPINE_DIR + "build_char_003_kalts.skel";
const ATLAS = SPINE_DIR + "build_char_003_kalts.atlas";
const PNG = SPINE_DIR + "build_char_003_kalts.png";
// The building model is about 260 units tall at scale 1.
const MODEL_SCALE = 0.62;

class Chibi {
  constructor(canvas) {
    this.canvas = canvas;
    const gl = canvas.getContext("webgl", { alpha: true, premultipliedAlpha: true, antialias: true });
    this.ctx = new spine.webgl.ManagedWebGLRenderingContext(gl);
    this.assets = new spine.webgl.AssetManager(this.ctx);
    this.renderer = new spine.webgl.SceneRenderer(canvas, this.ctx);
    this._rawAtlas = null;
    this._rawSkel = null;
    this.anim = null;
    this.flip = false;
    this.last = performance.now();
    this.ready = new Promise((ok, fail) => {
      const wait = () => {
        if (this.assets.isLoadingComplete()) {
          if (this.assets.hasErrors()) return fail(new Error(JSON.stringify(this.assets.getErrors())));
          this._build();
          ok(this);
        } else requestAnimationFrame(wait);
      };
      const load = () => {
        const invoke = window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;
        if (!invoke) {
          this.assets.loadBinary(SKEL);
          this.assets.loadTextureAtlas(ATLAS);
          wait();
          return;
        }
        invoke("chibi_model")
          .then((m) => {
            this._rawAtlas = m.atlas;
            const binStr = atob(m.skel);
            const bytes = new Uint8Array(binStr.length);
            for (let i = 0; i < binStr.length; i++) bytes[i] = binStr.charCodeAt(i);
            this._rawSkel = bytes;
            this.assets.setRawDataURI(PNG, m.png);
            this.assets.loadTexture(PNG);
            wait();
          })
          .catch(() => setTimeout(load, 5000));
      };
      load();
    });
  }

  _build() {
    const atlas = this._rawAtlas
      ? new spine.TextureAtlas(this._rawAtlas, (path) => this.assets.get(SPINE_DIR + path))
      : this.assets.get(ATLAS);
    const skel = this._rawSkel || this.assets.get(SKEL);
    const bin = new spine.SkeletonBinary(new spine.AtlasAttachmentLoader(atlas));
    bin.scale = MODEL_SCALE;
    this.data = bin.readSkeletonData(skel);
    this.skeleton = new spine.Skeleton(this.data);
    const sd = new spine.AnimationStateData(this.data);
    sd.defaultMix = 0.2;
    this.state = new spine.AnimationState(sd);
    this.animations = this.data.animations.map((a) => a.name);
    this._mouths();
    this.play("Relax");
    requestAnimationFrame((t) => this._frame(t));
  }

  /// Loops an animation; one-shots (Interact) fall back to `then`. `Default` is a still pose
  /// (0 s long): her real idle, breathing and swaying, is `Relax`.
  play(name, loop = true, then = "Relax") {
    if (!this.state || !this.animations.includes(name)) return;
    if (name === this.anim && loop) return;
    this.anim = name;
    this.state.setAnimation(0, name, loop);
    if (!loop) this.state.addAnimation(0, then, true, 0);
  }

  face(left) {
    this.flip = left;
  }

  /// The loudness of what her voice is playing right now (sent ~30 times a second from the
  /// sample the sound card is at); 0 closes her mouth.
  speak(level) {
    this.level = level || 0;
  }

  // ── her mouth ──
  // The model has a single closed mouth (F_Mouth, on a bone of its own). Two more are drawn
  // here, half open and open: painted at 6 px per texel and shrunk to the atlas' density, with
  // a ~60 % black outline like the original line's, so they blur like the rest of the sprite.

  _mouths() {
    this.mouth = { slot: this.skeleton.findSlot("F_Mouth"), open: 0, peak: 0.02, shown: 0 };
    const m = this.mouth;
    const closed = m.slot && m.slot.getAttachment();
    if (!(closed instanceof spine.RegionAttachment)) {
      m.slot = null;
      return;
    }
    const OUTLINE = "rgba(0,0,0,0.6)";
    const half = (g) => {
      g.beginPath();
      g.ellipse(0, 2, 13, 5.5, 0, 0, Math.PI * 2);
      g.fillStyle = "#3a1618";
      g.fill();
      g.lineWidth = 5;
      g.strokeStyle = OUTLINE;
      g.stroke();
    };
    // Flat top, round bottom: the usual chibi talking mouth, with a tongue.
    const open = (g) => {
      const shape = () => {
        g.beginPath();
        g.moveTo(-15, -4);
        g.lineTo(15, -4);
        g.bezierCurveTo(15, 14, -15, 14, -15, -4);
        g.closePath();
      };
      shape();
      g.fillStyle = "#3a1618";
      g.fill();
      g.save();
      shape();
      g.clip();
      g.beginPath();
      g.ellipse(0, 11, 10, 5, 0, 0, Math.PI * 2);
      g.fillStyle = "#c9686d";
      g.fill();
      g.restore();
      shape();
      g.lineWidth = 5;
      g.lineJoin = "round";
      g.strokeStyle = OUTLINE;
      g.stroke();
    };
    m.frames = [closed, this._drawnMouth(closed, half), this._drawnMouth(closed, open)];
  }

  /// A mouth 10x8 texels (the closed one's 10x6 box, taller), centred on the closed line.
  _drawnMouth(closed, paint) {
    const canvas = (w, h) => Object.assign(document.createElement("canvas"), { width: w, height: h });
    const big = canvas(60, 48);
    const g = big.getContext("2d");
    g.translate(30, 20);
    g.rotate(-0.22);
    paint(g);
    // In two steps so the browser averages pixels instead of skipping them.
    let src = big;
    for (const [w, h] of [[20, 16], [10, 8]]) {
      const c = canvas(w, h);
      const cg = c.getContext("2d");
      cg.imageSmoothingQuality = "high";
      cg.drawImage(src, 0, 0, w, h);
      src = c;
    }
    const region = {
      u: 0, v: 0, u2: 1, v2: 1, rotate: false, offsetX: 0, offsetY: 0,
      width: 10, height: 8, originalWidth: 10, originalHeight: 8,
      texture: new spine.webgl.GLTexture(this.ctx, src),
    };
    region.renderObject = region;
    const a = closed.copy();
    a.name = closed.name + "_drawn";
    a.height = (closed.height * 8) / 6;
    a.setRegion(region);
    a.updateOffset();
    return a;
  }

  /// Picks the mouth for this frame: closed, half open or open, by how loud she is now.
  _talk(dt) {
    const m = this.mouth;
    if (!m.slot) return;
    const rms = this.level || 0;
    // Relative to her recent loudest, so a quiet voice moves it as much as a loud one. The
    // constants were tuned at 30 fps; scaled by dt they hold at any frame rate.
    const f = dt * 30;
    m.peak = Math.max(m.peak * Math.pow(0.999, f), rms);
    const target = Math.min(1, Math.max(0, (rms / m.peak - 0.15) / 0.6));
    m.open += (target - m.open) * (1 - Math.pow(target > m.open ? 0.4 : 0.75, f));
    // A little hysteresis so it doesn't flicker on the borders.
    const up = [0.22, 0.6];
    const down = [0.12, 0.45];
    if (m.shown < 2 && m.open > up[m.shown]) m.shown++;
    else if (m.shown > 0 && m.open < down[m.shown - 1]) m.shown--;
    // Closed, the animation's own mouth stays: it is only put back once after talking.
    if (m.shown > 0 || m.slot.getAttachment() !== m.frames[0]) m.slot.setAttachment(m.frames[m.shown]);
  }

  _frame(t) {
    const dt = Math.min(0.1, (t - this.last) / 1000);
    this.last = t;
    const c = this.canvas;
    const w = (c.width = c.clientWidth * devicePixelRatio);
    const h = (c.height = c.clientHeight * devicePixelRatio);
    this.state.update(dt);
    this.state.apply(this.skeleton);
    this._talk(dt);
    this.skeleton.scaleX = this.flip ? -1 : 1;
    // Feet at the bottom centre of the canvas.
    this.skeleton.x = 0;
    this.skeleton.y = 0;
    this.skeleton.updateWorldTransform();
    const r = this.renderer;
    r.camera.position.x = 0;
    r.camera.position.y = h / devicePixelRatio / 2 - 6;
    r.camera.viewportWidth = w / devicePixelRatio;
    r.camera.viewportHeight = h / devicePixelRatio;
    r.resize(spine.webgl.ResizeMode.Fit);
    const gl = this.ctx.gl;
    gl.clearColor(0, 0, 0, 0);
    gl.clear(gl.COLOR_BUFFER_BIT);
    r.begin();
    r.drawSkeleton(this.skeleton, true);
    r.end();
    requestAnimationFrame((tt) => this._frame(tt));
  }
}
