import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";
import { EditorView } from "@codemirror/view";
import { initDesktop } from "../desktop/main.ts";

const tick = () => new Promise(resolve => setTimeout(resolve, 5));
function deferred() { let resolve, reject; const promise = new Promise((a,b)=>{resolve=a;reject=b}); return {promise,resolve,reject}; }
function setup(invoke) {
  const dom = new JSDOM(readFileSync(new URL("../desktop/index.html", import.meta.url), "utf8"), {pretendToBeVisual:true});
  Object.assign(globalThis, {window:dom.window, document:dom.window.document, Window:dom.window.Window, MutationObserver:dom.window.MutationObserver, requestAnimationFrame:dom.window.requestAnimationFrame.bind(dom.window), cancelAnimationFrame:dom.window.cancelAnimationFrame.bind(dom.window)});
  dom.window.HTMLElement.prototype.scrollIntoView = () => {};
  dom.window.HTMLDialogElement.prototype.showModal = function() { this.open = true; };
  dom.window.HTMLDialogElement.prototype.close = function() { this.open = false; };
  const controller=initDesktop(async (cmd,args) => { if(cmd === "open_universe") { await invoke(cmd,args); return {token: "test-token", pages: await invoke("page_index")}; } return await invoke(cmd,args); }), doc=dom.window.document;
  return {doc, controller, open(path){doc.querySelector('#database-path').value=path;doc.querySelector('#open-form').dispatchEvent(new dom.window.Event('submit',{cancelable:true}));}, view(){const node=doc.querySelector('.cm-editor'); return node ? EditorView.findFromDOM(node) : null;}, close(){dom.window.close();}};
}
const page = {id:'a',title:'Page',body:'original',revision:0,mentionCount:0};

test('empty and failed universe report a readable state', async()=>{
  let fail=false; const ui=setup(async cmd=>{if(cmd==='page_index'){if(fail)throw {};return []}});
  ui.open('a'); await tick(); assert.match(ui.doc.querySelector('#page-body').textContent,/no pages/i);
  fail=true; ui.open('b'); await tick(); assert.match(ui.doc.querySelector('#status').textContent,/could not be opened/i); ui.close();
});

test('save acknowledgement keeps later edits dirty and close guard protects them', async()=>{
  const pending=deferred(); let calls=[]; let sent;
  const ui=setup(async (cmd,args)=>{calls.push(cmd); if(cmd==='page_index')return [{id:'a',title:'Page'}]; if(cmd==='read_page')return page; if(cmd==='save_page'){sent=args.request;return pending.promise;}});
  ui.open('a'); await tick(); const view=ui.view(); assert.ok(view);
  view.dispatch({changes:{from:8,insert:' A'}}); ui.doc.querySelector('#save').click(); await tick();
  view.dispatch({changes:{from:10,insert:' B'}});
  pending.resolve({requestId:1,sessionId:sent.sessionId,pageId:'a',body:'original A',revision:1});
  // An acknowledgement applies only to the sent snapshot.
  await tick(); assert.match(ui.doc.querySelector('#save-status').textContent,/Unsaved/i);
  assert.equal(view.state.doc.toString(),'original A B'); ui.controller.requestClose('token'); await tick(); assert.ok(ui.doc.querySelector('#guard').open); assert.ok(!calls.includes('close_window')); ui.close();
});

test('linked and unsupported pages are read-only', async()=>{
  const ui=setup(async cmd=>cmd==='page_index'?[{id:'a',title:'Page'}]:cmd==='read_page'?{...page,mentionCount:1}:undefined);
  ui.open('a'); await tick(); assert.equal(ui.view(),null); assert.match(ui.doc.querySelector('#save-status').textContent,/links.*read-only/i); ui.close();
});

test('navigation is blocked while page loading, and selection resets scroll', async()=>{
  const first=deferred(); let reads=0, opens=0;
  const ui=setup(async cmd=>{if(cmd==='open_universe'){opens++;return;} if(cmd==='page_index')return [{id:'a',title:'Page'},{id:'b',title:'Other'}]; if(cmd==='read_page')return ++reads===1?first.promise:{...page,id:'b',title:'Other'};});
  ui.open('a'); await tick(); ui.open('b'); ui.doc.querySelectorAll('.page-link')[1].click(); await tick(); assert.equal(opens,1); assert.equal(reads,1);
  first.resolve({...page,revision:null}); await tick(); const pane=ui.doc.querySelector('.reading-area'); pane.scrollTop=200; ui.doc.querySelectorAll('.page-link')[1].click(); await tick(); assert.equal(pane.scrollTop,0); assert.equal(ui.doc.querySelector('#page-title').textContent,'Other'); ui.close();
});

test('stale save preserves the draft and requires a decision before navigation', async()=>{
  let saves=0, reads=0;
  const ui=setup(async (cmd,args)=>{if(cmd==='page_index')return [{id:'a',title:'Page'},{id:'b',title:'Other'}]; if(cmd==='read_page'){reads++;return page;} if(cmd==='save_page'){saves++;throw {kind:'stale',pageId:args.request.pageId,sessionId:args.request.sessionId,requestId:args.request.requestId};}});
  ui.open('a'); await tick(); const view=ui.view(); view.dispatch({changes:{from:8,insert:' draft'}}); ui.doc.querySelector('#save').click(); await tick();
  assert.equal(saves,1); assert.equal(view.state.doc.toString(),'original draft'); assert.match(ui.doc.querySelector('#save-status').textContent,/Changed elsewhere/);
  ui.doc.querySelectorAll('.page-link')[1].click(); await tick(); assert.ok(ui.doc.querySelector('#guard').open); assert.equal(reads,1);
  ui.doc.querySelector('#continue').click(); await tick(); assert.equal(view.state.doc.toString(),'original draft'); ui.close();
});

test('busy reread with unchanged revision keeps the draft and allows an explicit retry', async()=>{
  let reads=0, attempts=0;
  const ui=setup(async (cmd,args)=>{if(cmd==='page_index')return [{id:'a',title:'Page'}]; if(cmd==='read_page'){reads++;return page;} if(cmd==='save_page'){attempts++;if(attempts===1)throw {kind:'busy',pageId:args.request.pageId,sessionId:args.request.sessionId,requestId:args.request.requestId};return {pageId:'a',sessionId:args.request.sessionId,requestId:args.request.requestId,body:'original draft',revision:1};}});
  ui.open('a'); await tick(); const view=ui.view(); view.dispatch({changes:{from:8,insert:' draft'}}); ui.doc.querySelector('#save').click(); await tick(); await tick();
  assert.equal(reads,2); assert.equal(view.state.doc.toString(),'original draft'); assert.match(ui.doc.querySelector('#save-status').textContent,/Unsaved/);
  ui.doc.querySelector('#save').click(); await tick(); assert.equal(attempts,2); assert.match(ui.doc.querySelector('#save-status').textContent,/Saved/); ui.close();
});

test('failed save prevents save-then-proceed and preserves text on open-universe request', async()=>{
  let opens=0; const ui=setup(async (cmd,args)=>{if(cmd==='open_universe'){opens++;return;} if(cmd==='page_index')return [{id:'a',title:'Page'}];if(cmd==='read_page')return page;if(cmd==='save_page')throw {kind:'storage',pageId:args.request.pageId,sessionId:args.request.sessionId,requestId:args.request.requestId};});
  ui.open('a');await tick();const view=ui.view();view.dispatch({changes:{from:8,insert:' draft'}});ui.open('b');await tick();assert.ok(ui.doc.querySelector('#guard').open);ui.doc.querySelector('#save-proceed').click();await tick();assert.equal(opens,1);assert.equal(view.state.doc.toString(),'original draft');assert.match(ui.doc.querySelector('#save-status').textContent,/could not be confirmed/i);ui.controller.requestClose('token');await tick();assert.ok(ui.doc.querySelector('#guard').open);assert.equal(ui.doc.querySelector('#discard').disabled,true);ui.close();
});

test('revision limit makes the editor read-only after its final acknowledgement', async()=>{
  const pending=deferred(); let sent; const ui=setup(async(cmd,args)=>{if(cmd==='page_index')return [{id:'a',title:'Page'}];if(cmd==='read_page')return {...page,revision:Number.MAX_SAFE_INTEGER-1};if(cmd==='save_page'){sent=args.request;return pending.promise;}});
  ui.open('a');await tick();const view=ui.view();view.dispatch({changes:{from:8,insert:' x'}});ui.doc.querySelector('#save').click();await tick();view.dispatch({changes:{from:10,insert:' later'}});
  pending.resolve({pageId:'a',sessionId:sent.sessionId,requestId:sent.requestId,body:'original x',revision:Number.MAX_SAFE_INTEGER});await tick();
  assert.equal(view.state.doc.toString(),'original x later'); assert.match(ui.doc.querySelector('#save-status').textContent,/Revision limit/); assert.equal(view.contentDOM.getAttribute('contenteditable'),'false');
  view.dispatch({changes:{from:16,insert:' bad'}});assert.equal(view.state.doc.toString(),'original x later'); ui.close();
});
test('busy reread at a different revision is ambiguous and preserves the draft', async()=>{
  let reads=0; const ui=setup(async (cmd,args)=>{if(cmd==='page_index')return [{id:'a',title:'Page'}];if(cmd==='read_page')return ++reads===1?page:{...page,revision:1};if(cmd==='save_page')throw {kind:'busy',pageId:args.request.pageId,sessionId:args.request.sessionId,requestId:args.request.requestId};});
  ui.open('a');await tick();const view=ui.view();view.dispatch({changes:{from:8,insert:' draft'}});ui.doc.querySelector('#save').click();await tick();await tick();
  assert.equal(view.state.doc.toString(),'original draft');assert.match(ui.doc.querySelector('#save-status').textContent,/could not be confirmed/);assert.equal(ui.doc.querySelector('#save').disabled,true);ui.close();
});

test('incorrect save identity cannot make the draft clean or close the window', async()=>{
  const ui=setup(async(cmd,args)=>{if(cmd==='page_index')return [{id:'a',title:'Page'}];if(cmd==='read_page')return page;if(cmd==='save_page')return {pageId:'wrong',sessionId:args.request.sessionId,requestId:args.request.requestId,body:'original x',revision:1};});
  ui.open('a');await tick();const view=ui.view();view.dispatch({changes:{from:8,insert:' x'}});ui.doc.querySelector('#save').click();await tick();ui.controller.requestClose('token');await tick();
  assert.equal(view.state.doc.toString(),'original x');assert.ok(ui.doc.querySelector('#guard').open);assert.equal(ui.doc.querySelector('#discard').disabled,true);assert.equal(ui.doc.querySelector('#save').disabled,true);ui.close();
});

test('mixed line endings and unsafe revisions remain read-only', async()=>{
  for (const item of [{...page,body:'one\r\ntwo'}, {...page,revision:null}, {...page,revision:Number.MAX_SAFE_INTEGER}]) {
    const ui=setup(async cmd=>cmd==='page_index'?[{id:'a',title:'Page'}]:cmd==='read_page'?item:undefined);ui.open('a');await tick();assert.equal(ui.view(),null);assert.match(ui.doc.querySelector('#save-status').textContent,/read-only|not editable/i);ui.close();
  }
});

test('failed new universe index after discard keeps the old visible draft', async()=>{
  let indexes=0;const ui=setup(async cmd=>{if(cmd==='page_index'){if(++indexes===2)throw {};return [{id:'a',title:'Page'}];}if(cmd==='read_page')return page;});
  ui.open('a');await tick();const view=ui.view();view.dispatch({changes:{from:8,insert:' draft'}});ui.open('b');await tick();ui.doc.querySelector('#discard').click();await tick();
  assert.equal(ui.view(),view);assert.equal(view.state.doc.toString(),'original draft');assert.match(ui.doc.querySelector('#status').textContent,/could not be opened/i);ui.close();
});

test('a lost open response keeps the old token and preserves the draft if the backend changed', async()=>{
  let opens=0; let sent;
  const ui=setup(async(cmd,args)=>{
    if(cmd==='open_universe' && ++opens===2) throw {kind:'transport'};
    if(cmd==='page_index') return [{id:'a',title:'Page'}];
    if(cmd==='read_page') return page;
    if(cmd==='save_page') { sent=args.request; throw {kind:'universe_changed',pageId:sent.pageId,sessionId:sent.sessionId,requestId:sent.requestId}; }
  });
  ui.open('old');await tick();const view=ui.view();view.dispatch({changes:{from:8,insert:' draft'}});
  ui.open('new');await tick();ui.doc.querySelector('#discard').click();await tick();
  ui.doc.querySelector('#save').click();await tick();
  assert.equal(sent.universeToken,'test-token');assert.equal(view.state.doc.toString(),'original draft');
  assert.match(ui.doc.querySelector('#save-status').textContent,/could not be confirmed/i);ui.close();
});

test('mismatched error identity cannot settle a save or permit discarding', async()=>{
  const ui=setup(async(cmd,args)=>{if(cmd==='page_index')return [{id:'a',title:'Page'}];if(cmd==='read_page')return page;if(cmd==='save_page')throw {kind:'busy',pageId:'other',sessionId:args.request.sessionId,requestId:args.request.requestId};});
  ui.open('a');await tick();const view=ui.view();view.dispatch({changes:{from:8,insert:' draft'}});ui.doc.querySelector('#save').click();await tick();
  assert.match(ui.doc.querySelector('#save-status').textContent,/could not be confirmed/i);ui.controller.requestClose('token');await tick();assert.ok(ui.doc.querySelector('#guard').open);assert.equal(ui.doc.querySelector('#discard').disabled,true);assert.equal(view.state.doc.toString(),'original draft');ui.close();
});

test('repeated navigation and close during an unsaved decision cannot bypass it', async()=>{
  let reads=0,closes=0;const ui=setup(async cmd=>{if(cmd==='page_index')return [{id:'a',title:'Page'},{id:'b',title:'Other'}];if(cmd==='read_page'){reads++;return page;}if(cmd==='close_window')closes++;});
  ui.open('a');await tick();const view=ui.view();view.dispatch({changes:{from:8,insert:' draft'}});ui.doc.querySelectorAll('.page-link')[1].click();ui.doc.querySelectorAll('.page-link')[1].click();ui.controller.requestClose('token');await tick();
  assert.ok(ui.doc.querySelector('#guard').open);assert.equal(reads,1);assert.equal(closes,0);ui.doc.querySelector('#continue').click();await tick();assert.equal(view.state.doc.toString(),'original draft');assert.equal(closes,0);ui.close();
});
