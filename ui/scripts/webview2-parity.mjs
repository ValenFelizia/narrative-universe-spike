// Runs the TypeScript mention mirror inside the real Windows WebView2 of the
// desktop shell and checks it against the shared Rust fixtures.
//
// Usage, from ui/, after `cargo build --locked` in src-tauri:
//   node scripts/webview2-parity.mjs
// Prints the WebView2 user agent and every mismatch; exits 1 on any.

import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const ui = fileURLToPath(new URL("..", import.meta.url));
const port = 9333;
const fixture = (name) => JSON.parse(readFileSync(new URL(`../../tests/fixtures/${name}`, import.meta.url), "utf8"));
const bridge = fixture("bridge_cases.json");
const corpus = fixture("grapheme_boundaries.json");
const mirror = "/@fs/" + fileURLToPath(new URL("../src/mentions.ts", import.meta.url)).replaceAll("\\", "/");

const vite = spawn(process.execPath, ["node_modules/vite/bin/vite.js", "--config", "desktop/vite.config.ts"], { cwd: ui, stdio: "ignore" });
for (let attempt = 0; ; attempt += 1) {
  try {
    if ((await fetch("http://127.0.0.1:5173/")).ok) break;
  } catch {}
  if (attempt > 60) throw new Error("vite did not start");
  await new Promise((resolve) => setTimeout(resolve, 500));
}
const app = spawn(fileURLToPath(new URL("../src-tauri/target/debug/narrative-universe-desktop.exe", import.meta.url)), [], {
  cwd: ui,
  stdio: "ignore",
  env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` },
});
const stop = () => {
  app.kill();
  vite.kill();
};

async function target() {
  for (let attempt = 0; attempt < 120; attempt += 1) {
    try {
      const pages = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      const page = pages.find((entry) => entry.type === "page" && /^http:\/\/(localhost|127\.0\.0\.1):5173\//.test(entry.url));
      if (page) return page.webSocketDebuggerUrl;
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error("WebView2 debugging target did not appear");
}

function evaluate(url, expression) {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(url);
    socket.onopen = () => socket.send(JSON.stringify({ id: 1, method: "Runtime.evaluate", params: { expression, awaitPromise: true, returnByValue: true } }));
    socket.onmessage = (event) => {
      const message = JSON.parse(event.data);
      if (message.id !== 1) return;
      socket.close();
      if (message.result.exceptionDetails) reject(new Error(JSON.stringify(message.result.exceptionDetails)));
      else resolve(message.result.result.value);
    };
    socket.onerror = reject;
  });
}

const probe = `(async () => {
  const { applyMentionOp, validateMentions } = await import(${JSON.stringify(mirror)});
  const bridge = ${JSON.stringify(bridge)};
  const corpus = ${JSON.stringify(corpus)};
  const failures = [];
  const seg = new Intl.Segmenter("en", { granularity: "grapheme" });
  for (const item of corpus.cases) {
    const raw = [...seg.segment(item.text)].map((part) => part.index).concat(item.text.length);
    const found = [0];
    for (let end = 1; end <= item.text.length; end += 1) {
      try { validateMentions({ body: item.text, mentions: [{ id: "m", entityId: "e", startUtf16: 0, endUtf16: end }] }); found.push(end); } catch {}
    }
    if (JSON.stringify(raw) !== JSON.stringify(item.boundaries) || JSON.stringify(found) !== JSON.stringify(item.boundaries)) failures.push({ id: item.id, raw, found, expected: item.boundaries });
  }
  const mention = (held) => ({ id: held.id, entityId: held.entity_id, startUtf16: held.start_utf16, endUtf16: held.end_utf16 });
  for (const item of bridge.cases) {
    const ops = item.expected_ops
      ? item.expected_ops.map((op) => op.replace ? { kind: "replace", startUtf16: op.replace[0], endUtf16: op.replace[1], insertedText: op.replace[2] }
        : op.link ? { kind: "link", mentionId: op.link.id, entityId: op.link.entity_id, startUtf16: op.link.start_utf16, endUtf16: op.link.end_utf16 }
        : { kind: "unlink", mentionId: op.unlink })
      : item.expected_edits.map(([s, e, t]) => ({ kind: "replace", startUtf16: s, endUtf16: e, insertedText: t }));
    let body = { body: item.base, mentions: item.mentions.map(mention) };
    try { for (const op of ops) body = applyMentionOp(body, op); } catch (error) { failures.push({ id: item.id, error: String(error) }); continue; }
    if (body.body !== item.expected_body || JSON.stringify(body.mentions) !== JSON.stringify(item.expected_mentions.map(mention))) failures.push({ id: item.id, body: body.body, mentions: body.mentions });
  }
  return { userAgent: navigator.userAgent, boundaryCases: corpus.cases.length, reducerCases: bridge.cases.length, failures };
})()`;

try {
  const result = await evaluate(await target(), probe);
  console.log(JSON.stringify(result, null, 2));
  process.exitCode = result.failures.length === 0 ? 0 : 1;
} finally {
  stop();
}
