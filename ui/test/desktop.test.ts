import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";
import { initDesktop } from "../desktop/main.ts";

const tick = () => new Promise(resolve => setTimeout(resolve, 0));
function setup(invoke) {
  const dom = new JSDOM(readFileSync(new URL("../desktop/index.html", import.meta.url), "utf8"));
  globalThis.document = dom.window.document;
  initDesktop(invoke);
  const doc = dom.window.document;
  return { doc, open(path) { doc.querySelector("#database-path").value = path; doc.querySelector("#open-form").dispatchEvent(new dom.window.Event("submit", { cancelable: true })); }, click(i) { doc.querySelectorAll(".page-link")[i].click(); } };
}
const deferred = () => { let resolve, reject; const promise = new Promise((a,b) => {resolve=a; reject=b}); return {promise,resolve,reject}; };

test("open, empty list, and failed index", async () => {
  let fail = false;
  const ui = setup(async (cmd) => { if (cmd === "page_index") { if (fail) throw {}; return []; } });
  ui.open("one"); await tick(); assert.match(ui.doc.querySelector("#status").textContent, /No pages/);
  fail = true; ui.open("two"); await tick(); assert.match(ui.doc.querySelector("#status").textContent, /could not be read/);
});

test("latest selection wins, resets scroll, and failed reads are visible", async () => {
  const reads = [];
  const ui = setup((cmd) => cmd === "open_universe" ? Promise.resolve() : cmd === "page_index" ? Promise.resolve([{id:"a",title:"A"},{id:"b",title:"B"}]) : (reads.push(deferred()), reads.at(-1).promise));
  ui.open("one"); await tick(); ui.click(1);
  const pane = ui.doc.querySelector(".reading-area"); pane.scrollTop = 250;
  reads[1].resolve({id:"b",title:"B",body:"new"}); await tick(); assert.equal(pane.scrollTop, 0); assert.equal(ui.doc.querySelector("#page-body").textContent,"new");
  pane.scrollTop = 120; reads[0].resolve({id:"a",title:"A",body:"old"}); await tick(); assert.equal(pane.scrollTop,120); assert.equal(ui.doc.querySelector("#page-body").textContent,"new");
  ui.click(0); reads[2].reject({kind:"page_missing"}); await tick(); assert.match(ui.doc.querySelector("#status").textContent,/no longer available/);
});

test("old page response cannot replace a switched universe", async () => {
  const reads=[]; let count=0;
  const ui=setup(cmd => cmd === "open_universe" ? Promise.resolve() : cmd === "page_index" ? Promise.resolve([{id:String(++count),title:"Page"}]) : (reads.push(deferred()), reads.at(-1).promise));
  ui.open("one"); await tick(); reads[0].resolve({title:"First",body:"first"}); await tick();
  ui.click(0); ui.open("two"); await tick(); reads[2].resolve({title:"Second",body:"second"}); await tick(); reads[1].resolve({title:"Old",body:"old"}); await tick();
  assert.equal(ui.doc.querySelector("#page-title").textContent,"Second"); assert.equal(ui.doc.querySelector("#page-body").textContent,"second");
});
