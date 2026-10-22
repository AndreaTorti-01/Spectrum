// Screenshots of the built site through the Chrome DevTools Protocol. No dependencies: Node's global
// WebSocket talks to a headless Edge. A plain `msedge --headless --screenshot` call is not usable here: it
// lays a 390 px request out at about 500 px wide and often captures before the page is ready.
//
//   cd site; npm run build; npm run shots -- [options]       (starts `vite preview` itself if nothing listens)
//
//   --out <dir>        output folder (default ../screenshots-raw/site, gitignored)
//   --ids a,b          section ids (default: hero,window,hdr,gamut,tray; "all" = full page)
//   --sizes d,m        d = 1440x900 at DPR 1, m = 390x844 at DPR 2 (default both)
//   --themes light,dark
//   --reduced          emulate prefers-reduced-motion: reduce (files get "-rm")
//   --nojs             script execution off (files get "-nojs")
//   --hover <id>       animated page (no ?shot), pointer moved over the slab of that section, desktop only
//   --widths 320,360   extra pass: one hero capture per CSS width at DPR 1 (files "w<width>-<id>-<theme>")
//   --longtasks        no pictures: loads the page with 4x CPU throttle at 390 (DPR 3) and 1440 and prints the
//                      longest main-thread tasks from a DevTools trace plus the long-task observer entries
//   --url <base>       default http://127.0.0.1:4173/
//
// Files are named <id>-<width>-<theme>[-rm|-nojs|-hover].png. The page sets window.__shotReady once the final
// state of ?shot=<id> has rendered (fonts, images, canvases).
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const siteRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const EDGE = process.env.EDGE_PATH || "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe";
const ALL_IDS = ["hero", "window", "hdr", "gamut", "tray"];
const SIZES = { d: { name: "1440", width: 1440, height: 900, dpr: 1, mobile: false }, m: { name: "390", width: 390, height: 844, dpr: 2, mobile: true } };

const opt = { out: join(siteRoot, "..", "screenshots-raw", "site"), url: "http://127.0.0.1:4173/", ids: ALL_IDS, sizes: ["d", "m"], themes: ["light", "dark"], reduced: false, nojs: false, hover: "", widths: [], longtasks: false };
const argv = process.argv.slice(2);
for (let i = 0; i < argv.length; i++) {
  const a = argv[i];
  const val = () => argv[++i];
  if (a === "--out") opt.out = resolve(val());
  else if (a === "--url") opt.url = val().replace(/\/?$/, "/");
  else if (a === "--ids") opt.ids = val().split(",");
  else if (a === "--sizes") opt.sizes = val().split(",");
  else if (a === "--themes") opt.themes = val().split(",");
  else if (a === "--reduced") opt.reduced = true;
  else if (a === "--nojs") opt.nojs = true;
  else if (a === "--hover") opt.hover = val();
  else if (a === "--widths") opt.widths = val().split(",").map(Number);
  else if (a === "--longtasks") opt.longtasks = true;
  else throw new Error(`unknown option ${a}`);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function reachable(url) {
  try {
    return (await fetch(url)).ok;
  } catch {
    return false;
  }
}

class Cdp {
  constructor(ws) {
    this.ws = ws;
    this.id = 0;
    this.pending = new Map();
    this.listeners = [];
    ws.addEventListener("message", (ev) => {
      const m = JSON.parse(ev.data);
      if (m.id && this.pending.has(m.id)) {
        const { ok, fail } = this.pending.get(m.id);
        this.pending.delete(m.id);
        m.error ? fail(new Error(`${m.error.message} (${JSON.stringify(m.error)})`)) : ok(m.result);
      } else if (m.method) this.listeners.forEach((l) => l(m));
    });
  }
  send(method, params = {}) {
    const id = ++this.id;
    this.ws.send(JSON.stringify({ id, method, params }));
    return new Promise((ok, fail) => this.pending.set(id, { ok, fail }));
  }
  once(method) {
    return new Promise((ok) => {
      const l = (m) => {
        if (m.method === method) {
          this.listeners = this.listeners.filter((x) => x !== l);
          ok(m.params);
        }
      };
      this.listeners.push(l);
    });
  }
  on(fn) {
    this.listeners.push(fn);
  }
  async eval(expression, awaitPromise = false) {
    const r = await this.send("Runtime.evaluate", { expression, awaitPromise, returnByValue: true });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text);
    return r.result.value;
  }
}

async function launchEdge() {
  const dir = mkdtempSync(join(tmpdir(), "spectrum-edge-"));
  const proc = spawn(
    EDGE,
    ["--headless=new", "--remote-debugging-port=0", `--user-data-dir=${dir}`, "--no-first-run", "--no-default-browser-check", "--disable-extensions", "--hide-scrollbars", "--force-color-profile=srgb", "--mute-audio", "about:blank"],
    { stdio: "ignore" },
  );
  const file = join(dir, "DevToolsActivePort");
  for (let i = 0; i < 200 && !existsSync(file); i++) await sleep(100);
  if (!existsSync(file)) throw new Error("Edge did not open a debugging port");
  const port = readFileSync(file, "utf8").split(/\r?\n/)[0];
  let target;
  for (let i = 0; i < 50 && !target; i++) {
    target = (await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find((t) => t.type === "page");
    if (!target) await sleep(100);
  }
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((ok, fail) => {
    ws.addEventListener("open", ok);
    ws.addEventListener("error", fail);
  });
  const cdp = new Cdp(ws);
  const close = async () => {
    try {
      ws.close();
    } catch {}
    proc.kill();
    await sleep(300);
    try {
      rmSync(dir, { recursive: true, force: true });
    } catch {}
  };
  return { cdp, close };
}

async function setup(cdp, size, theme, extra = {}) {
  await cdp.send("Page.enable");
  await cdp.send("Emulation.setDeviceMetricsOverride", { width: size.width, height: size.height, deviceScaleFactor: size.dpr, mobile: size.mobile, screenWidth: size.width, screenHeight: size.height });
  await cdp.send("Emulation.setEmulatedMedia", {
    features: [
      { name: "prefers-color-scheme", value: theme },
      { name: "prefers-reduced-motion", value: opt.reduced ? "reduce" : "no-preference" },
    ],
  });
  await cdp.send("Emulation.setScriptExecutionDisabled", { value: opt.nojs });
  if (extra.cpu) await cdp.send("Emulation.setCPUThrottlingRate", { rate: extra.cpu });
}

async function open(cdp, url) {
  const loaded = cdp.once("Page.loadEventFired");
  await cdp.send("Page.navigate", { url });
  await loaded;
}

async function waitReady(cdp) {
  if (opt.nojs) {
    await sleep(500);
    return;
  }
  const t0 = Date.now();
  while (Date.now() - t0 < 30000) {
    if (await cdp.eval("window.__shotReady === true")) break;
    await sleep(100);
  }
  if (!(await cdp.eval("window.__shotReady === true"))) throw new Error("window.__shotReady never became true");
  await cdp.eval("document.fonts.ready.then(() => Promise.all(Array.from(document.images).map((i) => i.decode().catch(() => {})))).then(() => true)", true);
  await sleep(250);
}

async function grab(cdp, file, full = false) {
  const params = { format: "png", captureBeyondViewport: full };
  if (full) {
    const m = await cdp.send("Page.getLayoutMetrics");
    params.clip = { x: 0, y: 0, width: m.cssContentSize.width, height: m.cssContentSize.height, scale: 1 };
  }
  const r = await cdp.send("Page.captureScreenshot", params);
  writeFileSync(file, Buffer.from(r.data, "base64"));
  console.log(file);
}

async function shots(cdp) {
  mkdirSync(opt.out, { recursive: true });
  const suffix = (opt.reduced ? "-rm" : "") + (opt.nojs ? "-nojs" : "");
  for (const s of opt.sizes) {
    const size = SIZES[s];
    for (const theme of opt.themes) {
      await setup(cdp, size, theme);
      for (const id of opt.ids) {
        await open(cdp, opt.nojs ? `${opt.url}?theme=${theme}#${id === "all" ? "" : id}` : `${opt.url}?shot=${id}&theme=${theme}`);
        await waitReady(cdp);
        await grab(cdp, join(opt.out, `${id}-${size.name}-${theme}${suffix}.png`), id === "all");
      }
    }
  }
}

async function widths(cdp) {
  mkdirSync(opt.out, { recursive: true });
  for (const theme of opt.themes) {
    for (const w of opt.widths) {
      await setup(cdp, { width: w, height: 700, dpr: 1, mobile: true }, theme);
      for (const id of opt.ids.length === ALL_IDS.length ? ["hero"] : opt.ids) {
        await open(cdp, `${opt.url}?shot=${id}&theme=${theme}`);
        await waitReady(cdp);
        // html and body clip overflow-x, so measure the rightmost edge of the elements instead
        const edge = await cdp.eval("Math.round(Math.max(...Array.from(document.querySelectorAll('.nav-inner *, main *')).map((e) => e.getBoundingClientRect().right)))");
        const btn = await cdp.eval("Math.round(document.querySelector('.nav .btn').getBoundingClientRect().right)");
        console.log(`w${w} ${id} ${theme}: rightmost element edge ${edge}px, nav Download right edge ${btn}px (viewport ${w}px)`);
        await grab(cdp, join(opt.out, `w${w}-${id}-${theme}.png`));
      }
    }
  }
}

async function hover(cdp) {
  mkdirSync(opt.out, { recursive: true });
  const size = SIZES.d;
  for (const theme of opt.themes) {
    await setup(cdp, size, theme);
    await open(cdp, `${opt.url}?theme=${theme}`);
    await cdp.eval("document.fonts.ready.then(() => true)", true);
    await sleep(800);
    // slab near the top of the viewport, so its scroll-in animation has finished
    // (the entry animation shifts the box while it plays, so settle in three passes)
    for (let k = 0; k < 3; k++) {
      await cdp.eval(`(() => { const e = document.querySelector('#${opt.hover} [data-slab]'); window.scrollTo(0, e.getBoundingClientRect().top + scrollY - 90); })()`);
      await sleep(1200);
    }
    const r = await cdp.eval(`(() => { const b = document.querySelector('#${opt.hover} [data-slab]').getBoundingClientRect(); return { x: b.left, y: b.top, w: b.width, h: b.height }; })()`);
    const at = (fx, fy) => ({ type: "mouseMoved", x: Math.round(r.x + r.w * fx), y: Math.round(r.y + r.h * fy), pointerType: "mouse" });
    for (const [fx, fy] of [[0.9, 0.9], [0.6, 0.5], [0.3, 0.2]]) {
      await cdp.send("Input.dispatchMouseEvent", at(fx, fy));
      await sleep(250);
    }
    await sleep(1600);
    await grab(cdp, join(opt.out, `${opt.hover}-${size.name}-${theme}-hover.png`));
  }
}

async function longtasks(cdp) {
  const lt = `window.__lt = []; try { new PerformanceObserver((l) => { for (const e of l.getEntries()) { window.__lt.push([Math.round(e.startTime), Math.round(e.duration)]); console.log('longtask', Math.round(e.startTime), Math.round(e.duration) + 'ms'); } }).observe({ type: 'longtask', buffered: true }); } catch (e) {}`;
  await cdp.send("Page.enable");
  await cdp.send("Page.addScriptToEvaluateOnNewDocument", { source: lt });
  const events = [];
  cdp.on((m) => {
    if (m.method === "Tracing.dataCollected") events.push(...m.params.value);
  });
  // The first load of a fresh browser also pays one-off font and shaping initialisation in the renderer, which is
  // not site code. Run the first size twice and label the first run "cold".
  const runs = opt.sizes.map((k) => SIZES[k]);
  runs.unshift(runs[0]);
  for (const [n, size] of runs.entries()) {
    const theme = opt.themes[0];
    const dpr = size.mobile ? 3 : 1;
    await setup(cdp, { ...size, dpr }, theme, { cpu: 4 });
    events.length = 0;
    await cdp.send("Tracing.start", { transferMode: "ReportEvents", traceConfig: { includedCategories: ["devtools.timeline", "disabled-by-default-devtools.timeline", "__metadata"] } });
    await open(cdp, `${opt.url}?theme=${theme}`);
    await sleep(3500);
    const total = await cdp.eval("document.documentElement.scrollHeight");
    for (let y = 0; y < total; y += size.height * 0.6) {
      await cdp.eval(`window.scrollTo(0, ${y})`);
      await sleep(350);
    }
    await sleep(1500);
    const done = cdp.once("Tracing.tracingComplete");
    await cdp.send("Tracing.end");
    await done;
    const main = new Set(events.filter((e) => e.ph === "M" && e.name === "thread_name" && e.args?.name === "CrRendererMain").map((e) => `${e.pid}:${e.tid}`));
    const mine = events.filter((e) => main.has(`${e.pid}:${e.tid}`) && e.ph === "X");
    const tasks = mine.filter((e) => e.name === "RunTask").sort((a, b) => b.dur - a.dur);
    const t0 = Math.min(...tasks.map((e) => e.ts));
    if (process.env.DUMP_TRACE) writeFileSync(join(process.env.DUMP_TRACE, `trace-${size.name}.json`), JSON.stringify(mine));
    console.log(`
${n === 0 ? "COLD first load, " : ""}${size.name} px, DPR ${dpr}, theme ${theme}, CPU 4x: ${tasks.length} main-thread tasks, page height ${total}px`);
    console.log("longest tasks (ms@page ms):", tasks.slice(0, 8).map((e) => `${(e.dur / 1000).toFixed(1)}@${Math.round((e.ts - t0) / 1000)}`).join(", "));
    console.log("tasks over 50 ms:", tasks.filter((e) => e.dur > 50000).length);
    for (const t of tasks.filter((e) => e.dur > 50000).slice(0, Number(process.env.SHOW_TASKS || 4))) {
      const inner = new Map();
      for (const e of mine) {
        if (e === t || e.ts < t.ts || e.ts + e.dur > t.ts + t.dur + 1) continue;
        const name = e.name === "FunctionCall" ? `FunctionCall ${e.args?.data?.functionName || ""} ${String(e.args?.data?.url || "").split("/").pop()}:${e.args?.data?.lineNumber ?? ""}` : e.name;
        inner.set(name, (inner.get(name) || 0) + e.dur / 1000);
      }
      console.log(`  ${(t.dur / 1000).toFixed(0)} ms at ${Math.round((t.ts - t0) / 1000)}:`, [...inner].sort((a, b) => b[1] - a[1]).slice(0, 7).map(([n, d]) => `${n} ${d.toFixed(0)}`).join(" | "));
    }
  }
  await cdp.send("Emulation.setCPUThrottlingRate", { rate: 1 });
}

let preview = null;
if (!(await reachable(opt.url))) {
  const u = new URL(opt.url);
  preview = spawn(process.execPath, [join(siteRoot, "node_modules", "vite", "bin", "vite.js"), "preview", "--host", u.hostname, "--port", u.port || "80", "--strictPort"], { cwd: siteRoot, stdio: "ignore" });
  for (let i = 0; i < 100 && !(await reachable(opt.url)); i++) await sleep(100);
  if (!(await reachable(opt.url))) throw new Error(`could not reach ${opt.url}; run npm run build first`);
}
const { cdp, close } = await launchEdge();
try {
  if (opt.longtasks) await longtasks(cdp);
  else if (opt.widths.length) await widths(cdp);
  else if (opt.hover) await hover(cdp);
  else await shots(cdp);
} finally {
  await close();
  preview?.kill();
}
process.exit(0);
