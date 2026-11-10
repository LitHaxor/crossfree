/**
 * Pipeline: how crossfree turns a Windows game's drawing into a frame on a
 * Mac, as a hairline figure. Four stations stand on one bench, far to near:
 * the game's draw calls (a stack of command plates), the translator (a block
 * with three lanes: mtld3d, DXMT, vkd3d-proton), Metal on the GPU (a chip
 * whose die lifts off its board) and the frame (an upright screen). A draw
 * call, the small cube, hops station to station to the one under the pointer;
 * the stations answer by distance from it, each a beat after the last.
 *
 * Built on hairline's kernel (HL, MIT) to its ten rules: hits on the rest
 * pose, a stagger by distance, a clamped reach, one bright stroke, a composed
 * rest, opaque plates painted back to front, no words in the drawing.
 */
(() => {
  const { Cam, clamp, facing, fit, flatDot, mk, open, place, pointer, poly, prism, proj, put, ringAt, rings, rrect,
    solid, tour, tset, tval, tween, tdone, register, disposer } = HL;

  const XS = [0, 74, 148, 222]; // station centres along the bench
  const SEAT = [0, 74, 148, 244]; // where the draw call sits at each: on top, or in front of the screen
  const TOP = [20, 18, 10, 42]; // each station's top at rest
  const LIFT = 16, FALL = [1, 0.34, 0.12, 0.05], STAGGER = 45, HOP = 15;
  const NAMES = ["draw calls", "translate", "metal", "frame"];

  function mount(stage, svg, read) {
    const bag = disposer();
    const C = Cam(45, 0.5, 1.22);
    fit(C, [[-36, -32, -6], [258, 32, -6], [258, -32, 0], [-36, 32, 0], [222, -26, TOP[3] + LIFT], [74, 0, TOP[1] + LIFT + HOP + 9]], 200, 166);
    const P = proj(C), front = facing(C);
    const at = (x, y, z) => P(x, y, z);

    // The bench, and the dashed flow along its top: painted first, under everything.
    const [benchRing, benchInner] = rings(-36, -32, 258, 32, 10, 1.6);
    put(solid(svg), prism(P, front, benchRing, benchInner, -6, 0));
    mk("path", { class: "nf dash", d: open([at(-18, 0, 0), at(240, 0, 0)]) }, svg);

    // 1 · draw calls: three thin command plates; the stack fans out when chosen.
    const calls = [0, 1, 2].map(() => solid(svg));
    const plate = rings(-20, -16, 20, 16, 4, 1.2);
    const marks = [0, 1, 2, 3].map(() => flatDot(svg, C, 1.1, "dot off"));
    // 2 · translate: a block with three lanes cut in its top.
    const block = solid(svg), lanes = mk("path", { class: "nf lo" }, svg);
    const box = rings(XS[1] - 22, -20, XS[1] + 22, 20, 6, 1.6);
    // 3 · metal: a chip; its die lifts off the board, pins along the board's edges.
    const board = solid(svg), die = solid(svg);
    const pins = [];
    for (let k = 0; k < 16; k++) pins.push(flatDot(svg, C, 0.9, "dot off"));
    const boardR = rings(XS[2] - 24, -24, XS[2] + 24, 24, 4, 1.4), dieR = rings(XS[2] - 11, -11, XS[2] + 11, 11, 3, 1.2);
    // 4 · frame: a foot, and a screen standing on it.
    const foot = solid(svg), screen = solid(svg), glass = mk("path", { class: "nf lo" }, svg);
    const footR = rings(XS[3] - 8, -14, XS[3] + 8, 14, 3, 1.2), panelR = rings(XS[3] - 2.5, -26, XS[3] + 2.5, 26, 2, 0.6);
    // The draw call, last: it moves above whatever it sits on.
    const cube = solid(svg), cubeR = rings(-5, -5, 5, 5, 2, 1);

    const lift = XS.map(() => tween(0)), pos = tween(0);
    const parts = [calls, block, [board, die], [foot, screen]];
    const sils = [calls[2].sil, block.sil, die.sil, screen.sil];

    function draw(now) {
      const L = lift.map((tw) => tval(tw, now));
      const gap = 7 + L[0] * 0.55;
      calls.forEach((s, k) => put(s, prism(P, front, plate[0], plate[1], k * gap, k * gap + 3)));
      const callTop = 2 * gap + 3;
      [[-9, -6], [-3, -6], [3, -6], [9, -6]].forEach(([x, y], k) => place(marks[k], at(x, y, callTop)));

      const z1 = TOP[1] + L[1];
      put(block, prism(P, front, box[0], box[1], L[1], z1));
      lanes.setAttribute("d", [-11, 0, 11].map((y) => poly(ringAt(P, rrect(XS[1] - 15, y - 2, XS[1] + 15, y + 2, 2, 3), z1))).join(""));

      put(board, prism(P, front, boardR[0], boardR[1], 0, 4));
      put(die, prism(P, front, dieR[0], dieR[1], 4 + L[2] * 0.7, TOP[2] + L[2] * 0.7));
      for (let k = 0; k < 16; k++) {
        const side = k >> 2, t = -15 + (k & 3) * 10;
        const [x, y] = [[t, -21], [21, t], [t, 21], [-21, t]][side];
        place(pins[k], at(XS[2] + x, y, 4));
      }

      put(foot, prism(P, front, footR[0], footR[1], 0, 3));
      put(screen, prism(P, front, panelR[0], panelR[1], 3 + L[3] * 0.5, TOP[3] + L[3] * 0.5));
      const face = rrect(-21, 9 + L[3] * 0.5, 21, TOP[3] - 5 + L[3] * 0.5, 2, 3);
      glass.setAttribute("d", poly(face.map((q) => at(XS[3] + 2.6, q.u, q.v))));

      // The draw call hops: between stations it arcs over the bench.
      const s = tval(pos, now), i = Math.floor(s), f = s - i, j = Math.min(i + 1, 3);
      const tops = [callTop, z1, TOP[2] + L[2] * 0.7, 0];
      const x = SEAT[i] + (SEAT[j] - SEAT[i]) * f, base = tops[i] + (tops[j] - tops[i]) * f;
      const ring = cubeR[0].map((q) => ({ ...q, u: q.u + x }));
      const inner = cubeR[1].map((q) => ({ ...q, u: q.u + x }));
      const z = base + HOP * Math.sin(Math.PI * f);
      put(cube, prism(P, front, ring, inner, z, z + 9));
      return lift.some((tw) => !tdone(tw, now)) || !tdone(pos, now);
    }

    const B = register(stage, (_dt, now) => draw(now));
    bag.add(B.unregister);

    // Hits: the station whose resting centre is nearest the pointer's x (rule 01).
    const centres = XS.map((x, k) => at(x, 0, TOP[k] / 2)[0]);
    const hit = ([x]) => centres.reduce((best, c, k) => (Math.abs(c - x) < Math.abs(centres[best] - x) ? k : best), 0);

    let act = -1;
    function choose(a) {
      if (a === act) return;
      const now = performance.now(), from = a >= 0 ? a : act;
      act = a;
      lift.forEach((tw, k) => {
        const reach = a < 0 ? 0 : LIFT * FALL[clamp(Math.abs(k - a), 0, 3)];
        tset(tw, reach, now, Math.abs(k - from) * STAGGER);
      });
      tset(pos, a < 0 ? 0 : a, now, 0);
      sils.forEach((el, k) => el.classList.toggle("hi", k === a));
      cube.sil.classList.toggle("hi", a < 0);
      read.textContent = a < 0 ? "rest" : NAMES[a];
      B.wake();
    }

    cube.sil.classList.add("hi");
    read.textContent = "rest";
    bag.add(pointer(stage, { move: (p) => choose(hit(p)), leave: () => choose(-1) }));
    // At rest, an unseen hand walks the pipeline from the game to the screen.
    const stops = XS.map((x, k) => at(x, 0, TOP[k] / 2)).concat([null]);
    const walk = tour(stage, stops);
    bag.add(() => walk.stop());
    bag.add(() => svg.replaceChildren());
    return bag.dispose;
  }

  /** Mounts the figure in every element with data-figure="pipeline". */
  HL.inject(document);
  for (const stage of document.querySelectorAll('[data-figure="pipeline"]')) {
    stage.setAttribute("data-hairline", "");
    stage.setAttribute("data-hairline-theme", "dark");
    stage.setAttribute("role", "img");
    stage.setAttribute("aria-label", "How a frame is made: a game's draw calls go through a translator to Metal on the GPU, and out to the screen.");
    const svg = mk("svg", { viewBox: "0 0 400 320" }, stage);
    const read = document.createElement("p");
    read.className = "readout";
    stage.after(read);
    mount(stage, svg, read);
  }
})();
