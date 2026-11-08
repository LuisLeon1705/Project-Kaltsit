// A 3D body: her VRM 1.0 model (three.js + three-vrm, the same stack as riko_project), on the
// transparent desktop window. The same face as the Spine chibi and the pixel art (ready, play,
// face, speak), plus her mood: the overlay drives all three alike.
//
// - She moves with real animations (VRM Animation, `ui/anim/`): motion capture from Quaternius'
//   Universal Animation Library and KayKit Character Animations, both CC0, converted to VRMA by
//   open-vrma. They play in place (no walking off the canvas) and fade into each other.
//     Relax: standing idle (Kal'tsit with her arms folded now and then), talking idle while she
//     speaks; Move: walking; Sit: sitting down, sitting idle / talking, standing up; Sleep:
//     asleep sitting, head down and eyes shut (KayKit's lying down turns her with a root bone
//     the conversion drops: she stayed upright); Interact: a wave.
// - Her mouth says vowels: the backend reads which one is sounding from the audio itself
//   (`voice::vowels`) and sends it with the loudness; the five VRM shapes (aa ih ou ee oh) follow.
// - She blinks and looks at you; her hair and tail move with her (spring bones).
// - Her hormones pick the expression (happy, angry, sad, relaxed, surprised), never strong.
// - It draws at 60 fps while she talks or moves, 24 while still and 12 asleep.

import * as THREE from "three";
import { GLTFLoader } from "three/addons/loaders/GLTFLoader.js";
import { VRMLoaderPlugin, VRMUtils } from "@pixiv/three-vrm";
import { VRMAnimationLoaderPlugin, VRMLookAtQuaternionProxy, createVRMAnimationClip } from "@pixiv/three-vrm-animation";

const VOWELS = ["aa", "ih", "ou", "ee", "oh"];
const MOODS = ["happy", "angry", "sad", "relaxed", "surprised"];

/// The clips, by what they are for. `loop`: repeats; otherwise it plays once and holds or moves on.
const CLIPS = {
  idle: "Idle_Loop",
  idleArms: "Idle_FoldArms_Loop",
  idleTalk: "Idle_Talking_Loop",
  walk: "Walk_Loop",
  walkFormal: "Walk_Formal_Loop",
  sitDown: "Sitting_Enter",
  sit: "Sitting_Idle_Loop",
  sitTalk: "Sitting_Talking_Loop",
  sitUp: "Sitting_Exit",
  wave: "Waving",
};
/// Each companion's way of standing and walking.
const STYLE = {
  kaltsit: { idle: ["idleArms", "idle"], walk: "walkFormal" },
  lappland: { idle: ["idle", "idle"], walk: "walk" },
};

/// The models bring their face (the mesh with the expressions) without an outline ("none"), and
/// three-vrm only builds an outline for materials that ask for one when the model is read: the
/// face is switched on here, in the model's JSON, before it is read, with the skin's line colour.
/// Not the eyes, scars, tears or anything transparent. `bytes`: the .vrm (glTF binary).
export function withFaceOutline(bytes) {
  const view = new DataView(bytes);
  if (view.getUint32(0, true) !== 0x46546c67) return bytes; // "glTF"
  const len = view.getUint32(12, true);
  const json = JSON.parse(new TextDecoder().decode(new Uint8Array(bytes, 20, len)));
  const mats = json.materials || [];
  const mtoon = (m) => m && m.extensions && m.extensions.VRMC_materials_mtoon;
  const lined = mats.find((m) => /skin/i.test(m.name || "") && mtoon(m) && mtoon(m).outlineWidthMode !== "none") || mats.find((m) => mtoon(m) && mtoon(m).outlineWidthMode !== "none");
  if (!lined) return bytes;
  // Materials of meshes with morph targets (the face).
  const faceMats = new Set();
  for (const mesh of json.meshes || []) {
    for (const p of mesh.primitives || []) if (p.targets && p.targets.length && p.material != null) faceMats.add(p.material);
  }
  let changed = 0;
  mats.forEach((m, i) => {
    const e = mtoon(m);
    if (!e || e.outlineWidthMode !== "none" || !faceMats.has(i) || m.alphaMode === "BLEND") return;
    if (/eye|scar|tear|transparent|cheek|blush|highlight|brow|lash|teeth|tongue|mouth/i.test(m.name || "")) return;
    e.outlineWidthMode = "worldCoordinates";
    e.outlineWidthFactor = mtoon(lined).outlineWidthFactor || 0.0012;
    e.outlineColorFactor = mtoon(lined).outlineColorFactor || [0.2, 0.12, 0.1];
    e.outlineLightingMixFactor = mtoon(lined).outlineLightingMixFactor ?? 1;
    changed++;
  });
  if (!changed) return bytes;
  // The JSON chunk again, padded with spaces to 4 bytes, and the binary chunk as it was.
  let text = JSON.stringify(json);
  while (new TextEncoder().encode(text).length % 4) text += " ";
  const head = new TextEncoder().encode(text);
  const rest = new Uint8Array(bytes, 20 + len);
  const out = new Uint8Array(20 + head.length + rest.length);
  const ov = new DataView(out.buffer);
  ov.setUint32(0, 0x46546c67, true);
  ov.setUint32(4, 2, true);
  ov.setUint32(8, out.length, true);
  ov.setUint32(12, head.length, true);
  ov.setUint32(16, 0x4e4f534a, true); // "JSON"
  out.set(head, 20);
  out.set(rest, 20 + head.length);
  return out.buffer;
}

export class VrmBody {
  /// `opts.bust`: the VTuber view (from the waist up, full window, she follows the pointer with
  /// her eyes, stays standing).
  constructor(canvas, who, opts = {}) {
    this.canvas = canvas;
    this.who = who;
    this.bust = !!opts.bust;
    this.rest = "Relax";
    // The first reply after she appears may wave; then not for ten minutes.
    this.lastWave = -Infinity;
    this.quiet = 99;
    this.style = STYLE[who] || STYLE.lappland;
    this.anim = "Relax";
    this.left = false;
    this.level = 0;
    this.talking = 0;
    this.shape = [0, 0, 0, 0, 0];
    this.mouth = { open: 0, peak: 0.02, v: [0, 0, 0, 0, 0] };
    this.mood = Object.fromEntries(MOODS.map((m) => [m, 0]));
    this.moodTarget = { ...this.mood };
    this.blinkAt = performance.now() + 2500;
    this.blinkT = -1;
    this.turn = 0;
    this.actions = {};
    this.current = null;
    this.idleSwap = 15;
    const r = (this.renderer = new THREE.WebGLRenderer({ canvas, alpha: true, antialias: true, premultipliedAlpha: true }));
    r.setClearColor(0x000000, 0);
    r.outputColorSpace = THREE.SRGBColorSpace;
    this.scene = new THREE.Scene();
    this.camera = new THREE.PerspectiveCamera(20, 1, 0.1, 30);
    const sun = new THREE.DirectionalLight(0xffffff, Math.PI * 0.9);
    sun.position.set(0.6, 1.4, 2.2);
    this.scene.add(sun, new THREE.AmbientLight(0xffffff, 0.9));
    // She looks a little above the camera, at you.
    this.eyes = new THREE.Object3D();
    this.scene.add(this.eyes);
    this.clock = new THREE.Clock();
    this.ready = this._load().then(() => {
      this._loop();
      return this;
    });
  }

  async _load() {
    const invoke = window.__TAURI__.core.invoke;
    const bytes = withFaceOutline(await invoke("vrm_model", { who: this.who }));
    const loader = new GLTFLoader();
    loader.register((p) => new VRMLoaderPlugin(p));
    loader.register((p) => new VRMAnimationLoaderPlugin(p));
    const gltf = await new Promise((ok, fail) => loader.parse(bytes, "", ok, fail));
    const vrm = (this.vrm = gltf.userData.vrm);
    VRMUtils.removeUnnecessaryVertices(gltf.scene);
    VRMUtils.combineSkeletons(gltf.scene);
    VRMUtils.rotateVRM0(vrm);
    vrm.scene.traverse((o) => (o.frustumCulled = false));
    this._outline(vrm.scene);
    this.scene.add(vrm.scene);
    if (vrm.lookAt) {
      vrm.lookAt.target = this.eyes;
      // The clips' look-at track drives this (three-vrm-animation asks for it once, by name).
      const proxy = new VRMLookAtQuaternionProxy(vrm.lookAt);
      proxy.name = "VRMLookAtQuaternionProxy";
      vrm.scene.add(proxy);
    }
    this.has = new Set(Object.keys(vrm.expressionManager ? vrm.expressionManager.expressionMap : {}));
    // Framed by her height standing: feet at the bottom, a little room over her head.
    const box = new THREE.Box3().setFromObject(vrm.scene);
    this.height = box.max.y - box.min.y;
    this.floor = box.min.y;
    this._frame();
    this.mixer = new THREE.AnimationMixer(vrm.scene);
    this.mixer.addEventListener("finished", (e) => this._finished(e.action));
    await Promise.all(Object.entries(CLIPS).map(([key, file]) => this._clip(loader, key, file)));
    this._enter("Relax");
  }

  /// An anime line around her, as thick on any screen: the models bring MToon outlines sized in
  /// the world (1.2 mm, a fifth of a pixel at desktop size); measured on the screen instead.
  _outline(root) {
    // A fraction of the canvas height: ~1.4 px on the 300 px desktop body, ~2.5 px full screen.
    const width = this.bust ? 0.0024 : 0.0046;
    root.traverse((o) => {
      for (const m of [].concat(o.material || [])) {
        if (!m || !m.isMToonMaterial || m.outlineWidthMode === "none") continue;
        m.outlineWidthMode = "screenCoordinates";
        m.outlineWidthFactor = width;
        m.outlineLightingMixFactor = 0.6;
        m.needsUpdate = true;
      }
    });
  }

  async _clip(loader, key, file) {
    try {
      const gltf = await loader.loadAsync(`anim/${file}.vrma`);
      const anim = gltf.userData.vrmAnimations && gltf.userData.vrmAnimations[0];
      if (!anim) return;
      const clip = createVRMAnimationClip(anim, this.vrm);
      this._inPlace(clip);
      const a = this.mixer.clipAction(clip);
      const once = !file.endsWith("_Loop");
      if (once) {
        a.setLoop(THREE.LoopOnce, 1);
        a.clampWhenFinished = true;
      }
      this.actions[key] = a;
    } catch (e) {
      console.warn(`anim ${file}: ${e}`);
    }
  }

  /// The hips keep their height but not their way across the floor: she animates where she is
  /// (her window walks her across the screen).
  _inPlace(clip) {
    const hips = this.vrm.humanoid.getNormalizedBoneNode("hips");
    const track = clip.tracks.find((t) => t.name === `${hips.name}.position`);
    if (!track) return;
    const v = track.values;
    const [x0, z0] = [v[0], v[2]];
    for (let i = 0; i < v.length; i += 3) {
      v[i] = x0;
      v[i + 2] = z0;
    }
  }

  /// Fades from what she is doing into `key` (a clip), from its start.
  _to(key, fade = 0.35) {
    const next = this.actions[key];
    if (!next || next === this.current) return !!next;
    next.reset().setEffectiveWeight(1).fadeIn(fade).play();
    if (this.current) this.current.fadeOut(fade);
    this.current = next;
    this.currentKey = key;
    return true;
  }

  /// What a state is, made of clips: the way in, then the loop.
  _enter(state) {
    const from = this.anim;
    this.anim = state;
    this.pending = null;
    // Leaving a seat or the floor: stand up first.
    const seated = (s) => s === "Sit" || s === "Sleep";
    // Getting up before walking would slide her across the screen while she stands: walking
    // starts at once; to anything else she stands up first.
    if (seated(from) && !seated(state) && state !== "Move" && this._to("sitUp")) return (this.pending = state);
    switch (state) {
      case "Move":
        return this._to(this.style.walk, 0.2) || this._to("walk", 0.2);
      case "Sit":
      case "Sleep":
        // Already seated (resting after sitting): she stays in her seat.
        if (seated(from)) return this._to("sit", 0.8);
        return this._to("sitDown") ? (this.pending = "Sit:loop") : this._to("sit");
      case "Interact":
        return this._to("wave") || this._enter("Relax");
      default:
        return this._idle();
    }
  }

  _idle() {
    const pool = this.style.idle;
    return this._to(pool[Math.floor(Math.random() * pool.length)]) || this._to("idle");
  }

  /// A one-shot ended: on to what comes after it.
  _finished(action) {
    if (action !== this.current) return;
    const p = this.pending;
    this.pending = null;
    if (p === "Sit:loop") return this._to("sit");
    if (p) return this._enter(p);
    // The wave: back to what she was doing.
    if (this.anim === "Interact") return this._enter(this.rest === "Interact" ? "Relax" : this.rest);
  }

  /// The camera fits her whole standing height in the canvas.
  _frame() {
    const w = this.canvas.clientWidth || 260;
    const h = this.canvas.clientHeight || 300;
    // Supersampled on the small desktop canvas (the hair's cut-out edges grain otherwise); full
    // screen, the screen's own density is enough.
    const dpr = this.bust ? Math.min(window.devicePixelRatio || 1, 2) : Math.min(2.5, (window.devicePixelRatio || 1) * 2);
    this.renderer.setPixelRatio(dpr);
    this.renderer.setSize(w, h, false);
    this.camera.aspect = w / h;
    const full = this.height || 1.6;
    // The desktop body: all of her. The VTuber view: from the waist to a little over her head.
    const tall = this.bust ? full * 0.5 : full * 1.06;
    const mid = this.bust ? this.floor + full * 0.78 : this.floor + tall / 2;
    const dist = tall / 2 / Math.tan(THREE.MathUtils.degToRad(this.camera.fov / 2));
    this.camera.position.set(0, mid, dist);
    this.camera.lookAt(0, mid, 0);
    this.camera.updateProjectionMatrix();
    this.eyes.position.set(0, mid + tall * 0.25, dist);
    this.eyeHome = this.eyes.position.clone();
  }

  play(name, loop = true) {
    if (name === "Default") name = "Relax";
    // The VTuber view stays standing, facing you: walking and sitting are the desktop's.
    if (this.bust && (name === "Move" || name === "Sit")) name = "Relax";
    if (name === "Interact") {
      // Sent as every reply starts (a quick gesture for the chibi): a wave each time, or
      // standing up from her seat to wave, was too much. Standing, and not for ten minutes.
      const now = performance.now();
      if (this.anim !== "Relax" || this.moving || this.pending || now - this.lastWave < 600_000) return;
      this.lastWave = now;
      this._enter("Interact");
      return;
    }
    this.rest = name;
    if (name === this.anim) return;
    // Walking (her window is sliding): the new state waits until she stops.
    if (this.moving && name !== "Move") {
      this.anim = name;
      return;
    }
    this._enter(name);
  }

  /// The VTuber view: her eyes follow the pointer (x, y in -1..1 of the window).
  lookAt(x, y) {
    if (!this.eyeHome) return;
    this.eyes.position.set(this.eyeHome.x + x * 1.2, this.eyeHome.y + y * 0.7, this.eyeHome.z);
  }

  face(left) {
    this.left = left;
  }

  speak(level, shape) {
    this.level = level || 0;
    if (shape) this.shape = shape;
  }

  /// Her hormones (`hormonas` of her state) as a faint expression.
  feel(h) {
    if (!h) return;
    const d = (k, base) => (h[k] ?? base) - base;
    const t = { happy: 0, angry: 0, sad: 0, relaxed: 0, surprised: 0 };
    t.happy = Math.max(0, d("dopamina", 0.35) + d("oxitocina", 0.25)) * 1.2;
    t.angry = Math.max(0, Math.min(d("cortisol", 0.3), d("adrenalina", 0.15))) * 2;
    t.sad = Math.max(0, -d("serotonina", 0.6) - Math.max(0, d("dopamina", 0.35))) * 1.5;
    t.relaxed = Math.max(0, d("serotonina", 0.6) - Math.max(0, d("cortisol", 0.3))) * 1.5;
    t.surprised = Math.max(0, d("adrenalina", 0.15) - 0.35) * 1.5;
    // The strongest wins, and never more than 0.6: a face, not a mask.
    const top = MOODS.reduce((a, b) => (t[b] > t[a] ? b : a));
    for (const m of MOODS) this.moodTarget[m] = m === top ? Math.min(0.6, t[m]) : 0;
  }

  _set(name, v) {
    if (this.has.has(name)) this.vrm.expressionManager.setValue(name, v);
  }

  _talk(dt) {
    const m = this.mouth;
    const f = dt * 30;
    m.peak = Math.max(m.peak * Math.pow(0.999, f), this.level);
    const target = Math.min(1, Math.max(0, (this.level / m.peak - 0.12) / 0.6));
    m.open += (target - m.open) * (1 - Math.pow(target > m.open ? 0.35 : 0.6, f));
    for (let i = 0; i < 5; i++) {
      m.v[i] += (this.shape[i] - m.v[i]) * (1 - Math.pow(0.5, f));
      this._set(VOWELS[i], Math.min(1, m.v[i] * m.open * 1.1));
    }
    // Talking for a while (not a click): her body talks too; quiet for 2.5 s, it stops (the
    // pauses between sentences are shorter).
    if (this.level > 0.01) {
      this.talking = Math.min(1.5, this.talking + dt);
      this.quiet = 0;
    } else {
      this.quiet += dt;
      if (this.quiet > 2.5) this.talking = 0;
    }
  }

  _blink(now) {
    if (this.anim === "Sleep") return this._set("blink", 1);
    if (this.blinkT < 0 && now >= this.blinkAt) this.blinkT = 0;
    let v = 0;
    if (this.blinkT >= 0) {
      this.blinkT += 1;
      // Closes in 3 frames, opens in 5.
      v = this.blinkT <= 3 ? this.blinkT / 3 : Math.max(0, 1 - (this.blinkT - 3) / 5);
      if (this.blinkT > 8) {
        this.blinkT = -1;
        this.blinkAt = now + 2000 + Math.random() * 4500;
      }
    }
    this._set("blink", v);
  }

  /// Which loop fits now: talking or not, and now and then another way of standing.
  _choose(dt) {
    if (this.pending) return;
    const talk = this.talking > 0.6;
    // Another way of standing only now and then, never in the middle of a sentence.
    if (this.anim === "Relax") {
      if (talk && this.currentKey !== "idleTalk") this._to("idleTalk", 0.5);
      else if (this.talking === 0 && this.currentKey === "idleTalk") this._idle();
      else if (this.talking === 0 && (this.idleSwap -= dt) <= 0) {
        this.idleSwap = 20 + Math.random() * 25;
        this._idle();
      }
    } else if (this.anim === "Sit") {
      if (talk && this.currentKey === "sit") this._to("sitTalk", 0.5);
      else if (this.talking === 0 && this.currentKey === "sitTalk") this._to("sit", 0.5);
    }
  }

  /// Asleep in her seat: her head drops forward and to a side, breathing slowly; it lifts again
  /// when she wakes. Over whatever the clip did this frame.
  _doze(dt) {
    const target = this.anim === "Sleep" && !this.moving ? 1 : 0;
    this.dozing = (this.dozing || 0) + (target - (this.dozing || 0)) * (1 - Math.pow(0.15, dt));
    if (this.dozing < 0.01) return;
    const t = this.clock.elapsedTime;
    const b = (n) => this.vrm.humanoid.getNormalizedBoneNode(n);
    const head = b("head"), neck = b("neck"), chest = b("chest") || b("spine");
    const k = this.dozing;
    if (head) head.rotation.x += k * (0.45 + Math.sin(t * 0.8) * 0.02), head.rotation.z += k * 0.15;
    if (neck) neck.rotation.x += k * 0.2;
    if (chest) chest.rotation.x += k * (0.12 + Math.sin(t * 0.8) * 0.015);
  }

  /// Her window sliding across the screen (the backend walks it) is what counts: while it moves
  /// she walks, turned the way it goes, whatever else was asked meanwhile (a sleep order, an
  /// answer); when it stops, she goes back to what she was doing.
  _followWindow(dt) {
    if (this.bust) return;
    const x = window.screenX;
    const dx = this._lastX === undefined ? 0 : x - this._lastX;
    this._lastX = x;
    if (Math.abs(dx) >= 1) {
      this.left = dx < 0;
      this.stillFor = 0;
      if (!this.moving) {
        this.moving = true;
        this.pending = null;
        this._to(this.style.walk, 0.2) || this._to("walk", 0.2);
      }
      return;
    }
    this.stillFor = (this.stillFor || 0) + dt;
    if (this.moving && this.stillFor > 0.25) {
      this.moving = false;
      // Arrived: the backend says what she does now; until then, what she was doing.
      const state = this.anim === "Move" ? this.rest || "Relax" : this.anim;
      this.anim = "";
      this._enter(state === "Move" ? "Relax" : state);
    }
  }

  /// Which way she faces: the way she walks; standing, turned a little toward it.
  _turn(dt) {
    let turn = this.bust ? 0 : this.left ? 0.3 : -0.3;
    if (this.anim === "Move" || this.moving) turn = this.left ? -1.1 : 1.1;
    else if (this.anim === "Sit" || this.anim === "Sleep") turn = 0;
    // Asleep or sitting while the window moves would never happen: she walks then (above).
    this.turn += (turn - this.turn) * (1 - Math.pow(0.02, dt));
    this.vrm.scene.rotation.y = this.turn;
  }

  _loop() {
    let last = 0;
    const tick = (now) => {
      requestAnimationFrame(tick);
      const busy = this.level > 0.005 || this.anim === "Move" || this.moving || this.anim === "Interact" || this.pending || this.blinkT >= 0;
      // 60 fps while talking or moving, 24 while still, 12 asleep (measured: both companions
      // still at 30 fps took ~7 % of the GPU's 3D engine).
      const fps = busy || this.bust ? 60 : this.anim === "Sleep" ? 12 : 24;
      if (now - last < 1000 / (fps + 1)) return;
      last = now;
      const dt = Math.min(0.1, this.clock.getDelta());
      if (this.canvas.clientWidth !== this._w || this.canvas.clientHeight !== this._h) {
        this._w = this.canvas.clientWidth;
        this._h = this.canvas.clientHeight;
        this._frame();
      }
      for (const m of MOODS) {
        this.mood[m] += (this.moodTarget[m] - this.mood[m]) * (1 - Math.pow(0.3, dt));
        this._set(m, this.anim === "Interact" && m === "happy" ? Math.max(0.45, this.mood[m]) : this.mood[m]);
      }
      this._talk(dt);
      this._followWindow(dt);
      if (!this.moving) this._choose(dt);
      this.mixer.update(dt);
      this._doze(dt);
      this._turn(dt);
      this._blink(now);
      this.vrm.update(dt);
      this.renderer.render(this.scene, this.camera);
    };
    requestAnimationFrame(tick);
  }
}
