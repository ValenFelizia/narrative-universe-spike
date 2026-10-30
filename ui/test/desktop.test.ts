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
  const controller=initDesktop(async (cmd,args) => { if(cmd === "open_universe") { await invoke(cmd,args); return {token: "test-token", pages: await invoke("page_index")}; } const result = await invoke(cmd,args); if (cmd === "save_page" && result && result.title === undefined) return {...result, title: args.request.title ?? "Page"}; return result; }), doc=dom.window.document;
  return {doc, controller, open(path){doc.querySelector('#database-path').value=path;doc.querySelector('#open-form').dispatchEvent(new dom.window.Event('submit',{cancelable:true}));}, view(){const node=doc.querySelector('.cm-editor'); return node ? EditorView.findFromDOM(node) : null;}, close(){dom.window.close();}};
}
const page = {id:'a',title:'Page',body:'original',revision:0,mentionCount:0};

test('discard blocks button and keyboard saves while page or universe transition is pending', async()=>{
  for (const target of ['page','universe']) {
    const pending=deferred();let saves=0, reads=0, opens=0;
    const ui=setup(async(cmd)=>{
      if(cmd==='open_universe' && ++opens===2) return pending.promise;
      if(cmd==='page_index') return [{id:'a',title:'Page'},{id:'b',title:'Other'}];
      if(cmd==='read_page') { if(++reads===2 && target==='page') return pending.promise; return page; }
      if(cmd==='save_page') saves++;
    });
    ui.open('old');await tick();ui.view().dispatch({changes:{from:8,insert:' draft'}});
    if(target==='page') ui.doc.querySelectorAll('.page-link')[1].click(); else ui.open('new');
    await tick();ui.doc.querySelector('#discard').click();await tick();
    assert.equal(ui.doc.querySelector('#save').disabled,true);
    ui.doc.querySelector('#save').click();ui.doc.dispatchEvent(new ui.doc.defaultView.KeyboardEvent('keydown',{key:'s',ctrlKey:true,bubbles:true}));await tick();
    assert.equal(saves,0);assert.equal(ui.view().state.doc.toString(),'original draft');
    pending.resolve(target==='page'?{...page,id:'b'}:undefined);await tick();ui.close();
  }
});

test('busy verification can be retried without writing; changed base stays uncertain', async()=>{
  for (const changed of [false,true]) {
    let reads=0,saves=0;
    const ui=setup(async(cmd,args)=>{
      if(cmd==='page_index') return [{id:'a',title:'Page'}];
      if(cmd==='read_page') { reads++; if(reads===2) throw {}; return changed && reads>2 ? {...page,revision:1} : page; }
      if(cmd==='save_page') { saves++; if(saves===1) throw {kind:'busy',pageId:args.request.pageId,sessionId:args.request.sessionId,requestId:args.request.requestId}; return {pageId:'a',sessionId:args.request.sessionId,requestId:args.request.requestId,body:'original draft',revision:1}; }
    });
    ui.open('a');await tick();const view=ui.view();view.dispatch({changes:{from:8,insert:' draft'}});ui.doc.querySelector('#save').click();await tick();
    assert.equal(ui.doc.querySelector('#verify').hidden,false);ui.doc.querySelector('#verify').click();await tick();assert.equal(saves,1);assert.equal(view.state.doc.toString(),'original draft');
    if(changed) { assert.match(ui.doc.querySelector('#save-status').textContent,/copy your draft/i); assert.equal(ui.doc.querySelector('#save').disabled,true); }
    else { ui.doc.querySelector('#save').click();await tick();assert.equal(saves,2);assert.match(ui.doc.querySelector('#save-status').textContent,/saved/i); }
    ui.close();
  }
});

test('save then proceed still saves the draft before navigating', async()=>{
  let saves=0;const ui=setup(async(cmd,args)=>{
    if(cmd==='page_index')return [{id:'a',title:'Page'},{id:'b',title:'Other'}];
    if(cmd==='read_page')return args.pageId==='a'?page:{...page,id:'b',title:'Other'};
    if(cmd==='save_page'){saves++;return {pageId:'a',sessionId:args.request.sessionId,requestId:args.request.requestId,body:'original draft',revision:1};}
  });
  ui.open('a');await tick();ui.view().dispatch({changes:{from:8,insert:' draft'}});
  ui.doc.querySelectorAll('.page-link')[1].click();await tick();ui.doc.querySelector('#save-proceed').click();await tick();
  assert.equal(saves,1);assert.equal(ui.doc.querySelector('#page-title').textContent,'Other');ui.close();
});

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


test('title and prose save atomically while later title typing remains dirty and sidebar reorders', async()=>{
  const pending=deferred(); let sent;
  const ui=setup(async(cmd,args)=>{
    if(cmd==='page_index') return [{id:'a',title:'Page'},{id:'b',title:'Zoo'}];
    if(cmd==='read_page') return page;
    if(cmd==='save_page') { sent=args.request; return pending.promise; }
  });
  ui.open('a'); await tick(); const input=ui.doc.querySelector('#title-input');
  input.value='Zebra'; input.dispatchEvent(new ui.doc.defaultView.Event('input',{bubbles:true}));
  const view=ui.view(); view.dispatch({changes:{from:8,insert:'!'}}); ui.doc.querySelector('#save').click(); await tick();
  assert.equal(sent.title,'Zebra'); assert.ok(sent.edits.length);
  input.value='Zebras'; input.dispatchEvent(new ui.doc.defaultView.Event('input',{bubbles:true}));
  pending.resolve({pageId:'a',sessionId:sent.sessionId,requestId:sent.requestId,title:'Zebra',body:'original!',revision:1}); await tick();
  assert.equal(input.value,'Zebras'); assert.match(ui.doc.querySelector('#save-status').textContent,/Unsaved/);
  assert.equal(ui.doc.querySelector('.page-link[aria-current="true"]').dataset.pageId,'a'); assert.equal(ui.view(),view); ui.close();
});

test('lost creation response preserves draft and verification never retries creation', async()=>{
  let creates=0, id; const ui=setup(async(cmd,args)=>{
    if(cmd==='page_index') return id ? [{id:'a',title:'Page'},{id,title:'Untitled page'}] : [{id:'a',title:'Page'}];
    if(cmd==='read_page') return page;
    if(cmd==='create_page') { creates++; id=args.pageId; throw {}; }
  });
  ui.open('a'); await tick(); ui.view().dispatch({changes:{from:8,insert:' draft'}});
  ui.doc.querySelector('#new-page').click(); await tick(); ui.doc.querySelector('#discard').click(); await tick();
  assert.equal(ui.view().state.doc.toString(),'original draft'); assert.equal(creates,1);
  ui.doc.querySelector('#verify-create').click(); await tick(); assert.equal(creates,1);
  assert.match(ui.doc.querySelector('#status').textContent,/exists/); assert.equal(ui.view().state.doc.toString(),'original draft'); ui.close();
});

test('read-only page remains selected after creation verification refreshes the list', async()=>{
  const ui=setup(async cmd=>{
    if(cmd==='page_index') return [{id:'a',title:'Page'}];
    if(cmd==='read_page') return {...page,mentionCount:1};
    if(cmd==='create_page') throw {};
  });
  ui.open('a'); await tick();
  assert.equal(ui.view(),null);
  assert.equal(ui.doc.querySelector('.page-link').getAttribute('aria-current'),'true');
  ui.doc.querySelector('#new-page').click(); await tick();
  ui.doc.querySelector('#verify-create').click(); await tick();
  assert.equal(ui.doc.querySelector('.page-link').getAttribute('aria-current'),'true');
  assert.equal(ui.doc.querySelector('#page-body').textContent,'original');
  ui.close();
});

test('a stored multiline title is displayed exactly and makes the whole page read-only', async()=>{
  let saves=0; const multiline='First\n<second>\n';
  const ui=setup(async cmd=>{if(cmd==='page_index')return [{id:'a',title:multiline}];if(cmd==='read_page')return {...page,title:multiline};if(cmd==='save_page')saves++;});
  ui.open('a');await tick();
  const heading=ui.doc.querySelector('#page-title'), input=ui.doc.querySelector('#title-input');
  assert.equal(heading.hidden,false);assert.equal(heading.textContent,multiline);assert.equal(heading.children.length,0);
  assert.equal(input.hidden,true);assert.equal(input.readOnly,true);assert.equal(ui.view(),null);
  assert.equal(ui.doc.querySelector('#page-body').textContent,'original');assert.match(ui.doc.querySelector('#save-status').textContent,/title.*line breaks.*read-only/i);
  ui.doc.querySelector('#save').click();await tick();assert.equal(saves,0);ui.close();
});

test('new page guard honors keep, discard, and save before creation', async()=>{
  for(const choice of ['continue','discard','save-proceed']) {
    let creates=0,saves=0;const ui=setup(async(cmd,args)=>{
      if(cmd==='page_index')return [{id:'a',title:'Page'}];
      if(cmd==='read_page')return args.pageId==='a'?page:{...page,id:args.pageId,title:'Untitled page',body:''};
      if(cmd==='save_page'){saves++;return {pageId:'a',sessionId:args.request.sessionId,requestId:args.request.requestId,title:'Page',body:'original!',revision:1};}
      if(cmd==='create_page'){creates++;return {...page,id:args.pageId,title:'Untitled page',body:''};}
    });
    ui.open('a');await tick();ui.view().dispatch({changes:{from:8,insert:'!'}});ui.doc.querySelector('#new-page').click();await tick();
    assert.ok(ui.doc.querySelector('#guard').open);assert.equal(creates,0);ui.doc.querySelector('#'+choice).click();await tick();await tick();
    assert.equal(creates,choice==='continue'?0:1);assert.equal(saves,choice==='save-proceed'?1:0);
    if(choice==='continue')assert.equal(ui.view().state.doc.toString(),'original!');ui.close();
  }
});

test('failed save from new page guard preserves title and body and prevents creation', async()=>{
  let creates=0;const ui=setup(async(cmd,args)=>{if(cmd==='page_index')return [{id:'a',title:'Page'}];if(cmd==='read_page')return page;if(cmd==='create_page')creates++;if(cmd==='save_page')throw {kind:'storage',pageId:args.request.pageId,sessionId:args.request.sessionId,requestId:args.request.requestId};});
  ui.open('a');await tick();const input=ui.doc.querySelector('#title-input');input.value='Draft';input.dispatchEvent(new ui.doc.defaultView.Event('input'));ui.view().dispatch({changes:{from:8,insert:'!'}});
  ui.doc.querySelector('#new-page').click();await tick();ui.doc.querySelector('#save-proceed').click();await tick();
  assert.equal(creates,0);assert.equal(input.value,'Draft');assert.equal(ui.view().state.doc.toString(),'original!');assert.match(ui.doc.querySelector('#save-status').textContent,/could not be confirmed/i);ui.close();
});

test('title and body survive busy, stale, and mismatched save outcomes', async()=>{
  for(const outcome of ['busy','stale','mismatch']) {
    let reads=0, closes=0;
    const ui=setup(async(cmd,args)=>{
      if(cmd==='page_index')return [{id:'a',title:'Page'}];
      if(cmd==='read_page'){reads++;return outcome==='busy' && reads>1?{...page,revision:1}:page;}
      if(cmd==='close_window')closes++;
      if(cmd==='save_page')throw {kind:outcome==='mismatch'?'busy':outcome,pageId:outcome==='mismatch'?'wrong':args.request.pageId,sessionId:args.request.sessionId,requestId:args.request.requestId};
    });
    ui.open('a');await tick();const input=ui.doc.querySelector('#title-input');input.value='Draft';input.dispatchEvent(new ui.doc.defaultView.Event('input'));ui.view().dispatch({changes:{from:8,insert:'!'}});
    ui.doc.querySelector('#save').click();await tick();await tick();ui.controller.requestClose('close');await tick();
    assert.equal(input.value,'Draft');assert.equal(ui.view().state.doc.toString(),'original!');assert.ok(ui.doc.querySelector('#guard').open);assert.equal(closes,0);
    assert.equal(ui.doc.querySelector('#save-proceed').disabled,true);if(outcome!=='stale')assert.equal(ui.doc.querySelector('#discard').disabled,true);ui.close();
  }
});

test('delayed rename sorts the sidebar and keeps selection after acknowledgement', async()=>{
  const pending=deferred(); let sent;
  const ui=setup(async(cmd,args)=>{
    if(cmd==='page_index')return [{id:'a',title:'Page'},{id:'b',title:'Zoo'}];
    if(cmd==='read_page')return page;
    if(cmd==='save_page'){sent=args.request;return pending.promise;}
  });
  ui.open('one');await tick();const input=ui.doc.querySelector('#title-input');
  input.value='ZZ';input.dispatchEvent(new ui.doc.defaultView.Event('input'));ui.doc.querySelector('#save').click();await tick();
  assert.deepEqual([...ui.doc.querySelectorAll('.page-link')].map(x=>x.textContent),['Page','Zoo']);
  pending.resolve({pageId:'a',sessionId:sent.sessionId,requestId:sent.requestId,title:'ZZ',body:'original',revision:1});await tick();
  assert.deepEqual([...ui.doc.querySelectorAll('.page-link')].map(x=>x.textContent),['ZZ','Zoo']);
  assert.equal(ui.doc.querySelector('[aria-current="true"]').dataset.pageId,'a');ui.close();
});

test('pending creation blocks switching universes and created page saves via keyboard then closes', async()=>{
  const pending=deferred();let opens=0, created, saved, closed;
  const ui=setup(async(cmd,args)=>{
    if(cmd==='open_universe')opens++;
    if(cmd==='page_index')return [{id:'a',title:'Page'}];
    if(cmd==='read_page')return args.pageId==='a'?page:{...page,id:created,title:'Untitled page',body:''};
    if(cmd==='create_page'){created=args.pageId;return pending.promise;}
    if(cmd==='save_page'){saved=args.request;return {pageId:created,sessionId:saved.sessionId,requestId:saved.requestId,title:'New',body:'text',revision:1};}
    if(cmd==='close_window')closed=args.token;
  });
  ui.open('one');await tick();ui.doc.querySelector('#new-page').click();await tick();ui.open('two');await tick();assert.equal(opens,1);
  pending.resolve({...page,id:created,title:'Untitled page',body:''});await tick();await tick();
  assert.equal(ui.doc.querySelector('[aria-current="true"]').dataset.pageId,created);
  const input=ui.doc.querySelector('#title-input');input.value='New';input.dispatchEvent(new ui.doc.defaultView.Event('input'));
  ui.view().dispatch({changes:{from:0,insert:'text'}});ui.doc.dispatchEvent(new ui.doc.defaultView.KeyboardEvent('keydown',{key:'s',ctrlKey:true,bubbles:true}));await tick();
  assert.equal(saved.pageId,created);assert.equal(saved.title,'New');assert.match(ui.doc.querySelector('#save-status').textContent,/Saved/);
  ui.controller.requestClose('close-token');await tick();assert.equal(closed,'close-token');ui.close();
});

test('pending save cannot cross into another universe', async()=>{
  const pending=deferred();let opens=0,sent;
  const ui=setup(async(cmd,args)=>{
    if(cmd==='open_universe')opens++;
    if(cmd==='page_index')return [{id:'a',title:'Page'}];
    if(cmd==='read_page')return page;
    if(cmd==='save_page'){sent=args.request;return pending.promise;}
  });
  ui.open('one');await tick();ui.view().dispatch({changes:{from:8,insert:'!'}});ui.doc.querySelector('#save').click();await tick();
  ui.open('two');await tick();assert.equal(opens,1);assert.equal(sent.universeToken,'test-token');
  pending.resolve({pageId:'a',sessionId:sent.sessionId,requestId:sent.requestId,title:'Page',body:'original!',revision:1});await tick();
  assert.equal(ui.view().state.doc.toString(),'original!');ui.open('two');await tick();assert.equal(opens,2);ui.close();
});

test('narrow library Escape preserves selection and draft', async()=>{
  const ui=setup(async cmd=>cmd==='page_index'?[{id:'a',title:'Page'}]:cmd==='read_page'?page:undefined);
  Object.defineProperty(ui.doc.defaultView,'innerWidth',{value:800,configurable:true}); ui.open('a'); await tick();
  const view=ui.view(), library=ui.doc.querySelector('#library'), toggle=ui.doc.querySelector('#library-toggle');
  assert.equal(library.hidden,true); view.dispatch({changes:{from:8,insert:' draft'},selection:{anchor:1,head:4}});
  toggle.click(); assert.equal(library.hidden,false); ui.doc.dispatchEvent(new ui.doc.defaultView.KeyboardEvent('keydown',{key:'Escape',bubbles:true}));
  assert.equal(library.hidden,true); assert.equal(view.state.doc.toString(),'original draft'); assert.equal(view.state.selection.main.anchor,1); assert.equal(view.state.selection.main.head,4); assert.equal(toggle.getAttribute('aria-expanded'),'false'); ui.close();
});

test('narrow document navigation honors guard and then dismisses drawer', async()=>{
  const ui=setup(async(cmd,args)=>cmd==='page_index'?[{id:'a',title:'Page'},{id:'b',title:'Other'}]:cmd==='read_page'?args.pageId==='b'?{...page,id:'b',title:'Other'}:page:undefined);
  Object.defineProperty(ui.doc.defaultView,'innerWidth',{value:800,configurable:true});ui.open('a');await tick();ui.view().dispatch({changes:{from:8,insert:' draft'}});
  const toggle=ui.doc.querySelector('#library-toggle'),library=ui.doc.querySelector('#library');toggle.click();[...ui.doc.querySelectorAll('.page-link')].find(b=>b.textContent==='Other').click();await tick();assert.equal(ui.doc.querySelector('#guard').open,true);ui.doc.querySelector('#continue').click();await tick();assert.equal(library.hidden,false);assert.equal(ui.view().state.doc.toString(),'original draft');
  [...ui.doc.querySelectorAll('.page-link')].find(b=>b.textContent==='Other').click();await tick();ui.doc.querySelector('#discard').click();await tick();assert.equal(library.hidden,true);assert.equal(ui.doc.querySelector('#context-name').textContent,'Other');ui.close();
});

test('narrow drawer restores document title focus without changing its selection', async()=>{
  const ui=setup(async cmd=>cmd==='page_index'?[{id:'a',title:'Page'}]:cmd==='read_page'?page:undefined);
  Object.defineProperty(ui.doc.defaultView,'innerWidth',{value:800,configurable:true});ui.open('a');await tick();
  const title=ui.doc.querySelector('#title-input'),toggle=ui.doc.querySelector('#library-toggle');title.focus();title.setSelectionRange(1,3);toggle.click();ui.doc.dispatchEvent(new ui.doc.defaultView.KeyboardEvent('keydown',{key:'Escape',bubbles:true}));
  assert.equal(ui.doc.activeElement,title);assert.equal(title.selectionStart,1);assert.equal(title.selectionEnd,3);ui.close();
});
