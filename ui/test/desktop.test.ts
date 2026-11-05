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
  const controller=initDesktop(async (cmd,args) => { if(cmd === "open_universe") { await invoke(cmd,args); return {token: "test-token", pages: await invoke("page_index")}; } const result = await invoke(cmd,args); if (cmd === "save_page" && result) return {mentions: [], ...result, title: result.title ?? args.request.title ?? "Page"}; return result; }), doc=dom.window.document;
  return {doc, controller, open(path){doc.querySelector('#database-path').value=path;doc.querySelector('#open-form').dispatchEvent(new dom.window.Event('submit',{cancelable:true}));}, view(){const node=doc.querySelector('.cm-editor'); return node ? EditorView.findFromDOM(node) : null;}, close(){dom.window.close();}};
}
const page = {id:'a',title:'Page',body:'original',revision:0,mentions:[]};

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
    if(changed) { assert.match(ui.doc.querySelector('#save-status').textContent,/draft.*memory/i); assert.equal(ui.doc.querySelector('#save').disabled,true); }
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

test('linked pages open with the mention-aware editor', async()=>{
  const ui=setup(async cmd=>cmd==='page_index'?[{id:'a',title:'Page'}]:cmd==='read_page'?{...page,mentions:[{id:'m',entityId:'e',startUtf16:0,endUtf16:1}]}:undefined);
  ui.open('a'); await tick(); assert.ok(ui.view()); assert.ok(ui.doc.querySelector('.entity-mention.saved')); ui.close();
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
    if(cmd==='read_page') return {...page,body:'original\r'};
    if(cmd==='create_page') throw {};
  });
  ui.open('a'); await tick();
  assert.equal(ui.view(),null);
  assert.equal(ui.doc.querySelector('.page-link').getAttribute('aria-current'),'true');
  ui.doc.querySelector('#new-page').click(); await tick();
  ui.doc.querySelector('#verify-create').click(); await tick();
  assert.equal(ui.doc.querySelector('.page-link').getAttribute('aria-current'),'true');
  assert.equal(ui.doc.querySelector('#page-body').textContent,'original\r');
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

test('empty titles keep a label after save, navigation, and reopen without renaming', async()=>{
  const store=new Map([['a',{id:'a',title:'Page',body:'alpha',revision:0,mentions:[]}],['b',{id:'b',title:'Other',body:'beta',revision:0,mentions:[]}]]);
  const ui=setup(async(cmd,args)=>{
    if(cmd==='page_index')return [...store.values()].map(({id,title})=>({id,title}));
    if(cmd==='read_page')return {...store.get(args.pageId)};
    if(cmd==='save_page'){
      const current=store.get(args.request.pageId);
      const saved={...current,title:args.request.title??current.title,revision:current.revision+1};
      store.set(saved.id,saved);
      return {pageId:saved.id,sessionId:args.request.sessionId,requestId:args.request.requestId,title:saved.title,body:saved.body,revision:saved.revision};
    }
  });
  const link=id=>ui.doc.querySelector(`.page-link[data-page-id="${id}"]`);
  const input=()=>ui.doc.querySelector('#title-input');
  const clearTitle=()=>{input().value='';input().dispatchEvent(new ui.doc.defaultView.Event('input',{bubbles:true}));};
  ui.open('universe');await tick();
  link('a').click();await tick();
  clearTitle();
  assert.equal(ui.doc.querySelector('#context-name').textContent,'Untitled document');
  ui.doc.querySelector('#save').click();await tick();
  assert.equal(store.get('a').title,'');
  assert.equal(link('a').textContent,'Untitled document');
  link('b').click();await tick();
  link('a').click();await tick();
  assert.equal(store.get('a').title,'');
  assert.equal(input().value,'');
  assert.equal(ui.doc.querySelector('#context-name').textContent,'Untitled document');
  assert.equal(link('a').textContent,'Untitled document');
  assert.equal(link('a').getAttribute('aria-current'),'true');
  assert.equal(ui.view().state.doc.toString(),'alpha');
  link('b').click();await tick();
  clearTitle();
  ui.doc.querySelector('#save').click();await tick();
  assert.equal(store.get('b').title,'');
  assert.deepEqual([...ui.doc.querySelectorAll('.page-link')].map(button=>button.textContent),['Untitled document','Untitled document']);
  assert.deepEqual([...ui.doc.querySelectorAll('.page-link')].map(button=>button.dataset.pageId),['a','b']);
  link('a').click();await tick();
  assert.equal(link('a').getAttribute('aria-current'),'true');
  assert.equal(ui.doc.querySelector('#context-name').textContent,'Untitled document');
  assert.equal(ui.view().state.doc.toString(),'alpha');
  link('b').click();await tick();
  assert.equal(link('b').getAttribute('aria-current'),'true');
  assert.equal(ui.view().state.doc.toString(),'beta');
  ui.close();
});

test('save sends tagged ops with the full predicted aggregate and refuses a mention it did not send', async()=>{
  let sent;
  const ui=setup(async(cmd,args)=>{
    if(cmd==='page_index') return [{id:'a',title:'Page'}];
    if(cmd==='read_page') return page;
    if(cmd==='save_page'){sent=args.request;return {pageId:'a',sessionId:sent.sessionId,requestId:sent.requestId,body:'original draft',title:'Page',revision:1,mentions:[{id:'m',entityId:'e',startUtf16:0,endUtf16:8}]};}
  });
  ui.open('a');await tick();ui.view().dispatch({changes:{from:8,insert:' draft'}});
  ui.doc.querySelector('#save').click();await tick();
  assert.deepEqual(sent.edits,[{kind:'replace',startUtf16:8,endUtf16:8,insertedText:' draft'}]);
  assert.equal(sent.expectedBody,'original draft');
  assert.deepEqual(sent.expectedMentions,[]);
  assert.doesNotMatch(ui.doc.querySelector('#save-status').textContent,/^Saved$/);
  assert.equal(ui.doc.querySelector('#save').disabled,true);
  assert.equal(ui.view().state.doc.toString(),'original draft');
  ui.close();
});

test('a mention mismatch refusal keeps the draft and is never reported as saved', async()=>{
  const ui=setup(async(cmd,args)=>{
    if(cmd==='page_index') return [{id:'a',title:'Page'}];
    if(cmd==='read_page') return page;
    if(cmd==='save_page') throw {kind:'mention_mismatch',message:['m'],pageId:'a',sessionId:args.request.sessionId,requestId:args.request.requestId};
  });
  ui.open('a');await tick();ui.view().dispatch({changes:{from:0,insert:'An '}});
  ui.doc.querySelector('#save').click();await tick();
  assert.match(ui.doc.querySelector('#save-status').textContent,/draft.*retained|could not be confirmed/i);
  assert.equal(ui.view().state.doc.toString(),'An original');
  ui.close();
});
import { undo, redo, isolateHistory } from "@codemirror/commands";
import { mentionField } from "../src/mention-history.ts";
import { prepareEntityIndex } from "../desktop/connect.ts";
const entities = [
  {id:'place-0001',label:'Valdoria',typeLabel:'Place',primaryPageTitle:'Atlas'},
  {id:'house-0002',label:'Valdoria',typeLabel:'House',primaryPageTitle:null},
  {id:'place-0003',label:'Valdoria',typeLabel:'Place',primaryPageTitle:'Atlas'},
];
function connectSetup(extra = async()=>undefined, initial = {...page,body:'Valdoria at dawn.'}) {
  return setup(async(cmd,args)=>{
    if(cmd==='page_index')return [{id:'a',title:'Page'},{id:'b',title:'Other'}];
    if(cmd==='read_page')return {...initial,id:args.pageId};
    if(cmd==='entity_index')return entities;
    return extra(cmd,args);
  });
}
function key(ui, target, name, ctrlKey=false) { target.dispatchEvent(new ui.doc.defaultView.KeyboardEvent('keydown',{key:name,ctrlKey,bubbles:true,cancelable:true})); }
async function picker(ui) { const view=ui.view(); view.dispatch({selection:{anchor:0,head:8}}); key(ui,view.contentDOM,'k',true); await tick(); return ui.doc.querySelector('[role="combobox"]'); }
function choose(ui, id='place-0001') { ui.doc.querySelector(`[data-entity-id="${id}"]`).click(); }

test('visible action and shortcut share picker; exact ties show ids; Enter requires explicit choice', async()=>{
  const ui=connectSetup();ui.open('a');await tick();const view=ui.view();view.dispatch({selection:{anchor:0,head:8}});
  const action=ui.doc.querySelector('.link-entity-action');assert.equal(action.hidden,false);action.click();await tick();
  const search=ui.doc.querySelector('[role="combobox"]');assert.equal(search.value,'Valdoria');assert.equal(ui.doc.querySelectorAll('.entity-id').length,2);
  const captions=prepareEntityIndex(entities);assert.match(captions[0].caption,/0001/);assert.doesNotMatch(captions[1].caption,/0002/);
  key(ui,search,'Enter');assert.equal(view.state.field(mentionField).length,0);
  key(ui,search,'ArrowDown');key(ui,search,'ArrowDown');key(ui,search,'Enter');
  assert.equal(view.state.field(mentionField)[0].entityId,'house-0002');assert.equal(view.hasFocus,true);
  assert.equal(ui.doc.querySelector('#save').disabled,false);assert.ok(ui.doc.querySelector('.entity-mention.draft'));ui.close();
});

test('prepared captions preserve exact ties, shortest unique suffixes, null labels and index order', ()=>{
  const index=[
    {id:'a10001',label:'VÁLDORIA',typeLabel:'Place',primaryPageTitle:'Atlas'},
    {id:'house0001',label:'VÁLDORIA',typeLabel:'House',primaryPageTitle:'Atlas'},
    {id:'b10001',label:'VÁLDORIA',typeLabel:'Place',primaryPageTitle:'Atlas'},
    {id:'c0002',label:'VÁLDORIA',typeLabel:'Place',primaryPageTitle:'Atlas'},
    {id:'untyped',label:'Other',typeLabel:null,primaryPageTitle:null},
  ];
  const prepared=prepareEntityIndex(index);
  assert.deepEqual(prepared.map(e=>e.id),index.map(e=>e.id));
  assert.deepEqual(prepared.map(e=>e.caption),[
    'VÁLDORIA · Place · Atlas · …a10001','VÁLDORIA · House · Atlas',
    'VÁLDORIA · Place · Atlas · …b10001','VÁLDORIA · Place · Atlas · …0002',
    'Other · Untyped · No primary page',
  ]);
  for(const e of prepared) { assert.equal(e.label+e.suffix,e.caption);assert.equal(e.searchText,e.caption.toLocaleLowerCase()); }
  assert.deepEqual(prepareEntityIndex([...index].reverse()).reverse(),prepared);
});

test('typing and arrow navigation reuse prepared captions without rereading the entity index', async()=>{
  let reads=0,loads=0;
  const observed=entities.map(e=>Object.fromEntries(Object.keys(e).map(k=>[k,e[k]])));
  for(const e of observed) for(const k of Object.keys(e)) { const value=e[k];Object.defineProperty(e,k,{get(){reads++;return value;}}); }
  const ui=setup(async(cmd,args)=>cmd==='page_index'?[{id:'a',title:'Page'}]:cmd==='read_page'?{...page,body:'Valdoria at dawn.'}:cmd==='entity_index'?(loads++,observed):undefined);
  ui.open('a');await tick();const search=await picker(ui),preparedReads=reads;
  assert.equal(preparedReads,observed.length*4);
  const captions=()=>[...ui.doc.querySelectorAll('[data-entity-id]')].map(row=>row.textContent);
  assert.deepEqual(captions(),['Valdoria · Place · Atlas · …0001','Valdoria · House · No primary page','Valdoria · Place · Atlas · …0003']);
  for(const query of ['Place','0003','House','VALDORIA','']) {
    search.value=query;search.dispatchEvent(new ui.doc.defaultView.Event('input',{bubbles:true}));
    key(ui,search,'ArrowDown');key(ui,search,'ArrowDown');key(ui,search,'ArrowUp');
    assert.equal(reads,preparedReads);assert.equal(loads,1);
    if(query==='0003') assert.equal(ui.doc.querySelector('[aria-selected="true"]').dataset.entityId,'place-0003');
  }
  key(ui,search,'Escape');assert.equal(ui.view().hasFocus,true);assert.equal(ui.view().state.selection.main.to,8);
  await picker(ui);assert.equal(loads,2);assert.equal(reads,preparedReads*2);
  choose(ui,'house-0002');assert.equal(ui.view().state.field(mentionField)[0].entityId,'house-0002');ui.close();
});

test('Escape restores selection and focus; changed selection and document cancel picker', async()=>{
  const ui=connectSetup();ui.open('a');await tick();const view=ui.view();let search=await picker(ui);
  key(ui,search,'Escape');assert.equal(view.hasFocus,true);assert.equal(view.state.selection.main.to,8);
  await picker(ui);view.dispatch({selection:{anchor:9}});assert.equal(ui.doc.querySelector('.entity-picker').hidden,true);
  await picker(ui);view.dispatch({changes:{from:0,insert:'x'}});assert.equal(ui.doc.querySelector('.entity-picker').hidden,true);assert.equal(view.state.field(mentionField).length,0);ui.close();
});

test('selection refusals never expand graphemes or link multiple lines', async()=>{
  const ui=connectSetup(undefined,{...page,body:'a\u0301\nValdoria'});ui.open('a');await tick();const view=ui.view();
  for(const selection of [{anchor:0},{anchor:0,head:1},{anchor:0,head:4}]) {
    view.dispatch({selection});key(ui,view.contentDOM,'k',true);await tick();assert.equal(ui.doc.querySelector('.entity-picker').hidden,true);assert.equal(view.state.field(mentionField).length,0);
  }
  ui.close();
});

test('persistent drop notice and Undo/Redo preserve identity; deliberate Unlink is distinct', async()=>{
  const ui=connectSetup();ui.open('a');await tick();await picker(ui);choose(ui);const view=ui.view(), original={...view.state.field(mentionField)[0]};
  view.dispatch({changes:{from:3,insert:'x'}});assert.equal(ui.doc.querySelector('.entity-mention'),null);assert.equal(ui.doc.querySelector('.link-drop-notice').hidden,false);
  view.dispatch({selection:{anchor:10}});assert.equal(ui.doc.querySelector('.link-drop-notice').hidden,false);
  undo(view);assert.deepEqual(view.state.field(mentionField),[original]);assert.equal(ui.doc.querySelector('.link-drop-notice').hidden,true);
  redo(view);assert.equal(view.state.field(mentionField).length,0);undo(view);
  view.dispatch({selection:{anchor:1}});key(ui,view.contentDOM,'k',true);await tick();
  assert.match(ui.doc.querySelector('.entity-picker').textContent,/Valdoria.*Place.*Unsaved link/);
  [...ui.doc.querySelectorAll('.entity-picker button')].find(b=>b.textContent==='Unlink').click();assert.equal(view.state.field(mentionField).length,0);assert.equal(ui.doc.querySelector('.link-drop-notice').hidden,true);
  undo(view);assert.deepEqual(view.state.field(mentionField),[original]);redo(view);assert.equal(view.state.field(mentionField).length,0);ui.close();
});

test('drop notice undoes the last change and persists until each link is restored', async()=>{
  const mentions=[{id:'first',entityId:'place-0001',startUtf16:0,endUtf16:8},{id:'second',entityId:'house-0002',startUtf16:12,endUtf16:20}];
  const ui=connectSetup(undefined,{...page,body:'Valdoria at Valdoria.',mentions});ui.open('a');await tick();const view=ui.view();
  const isolated=()=>isolateHistory.of('full');
  view.dispatch({changes:{from:2,insert:'x'},annotations:isolated()});
  view.dispatch({changes:{from:15,insert:'x'},annotations:isolated()});
  view.dispatch({changes:{from:view.state.doc.length,insert:' Later.'},annotations:isolated()});
  const notice=ui.doc.querySelector('.link-drop-notice');
  assert.match(notice.textContent,/2 links removed.*Undo recent edits/);
  assert.equal(notice.querySelector('button').textContent,'Undo last change');
  notice.querySelector('button').click();
  assert.equal(view.state.doc.toString().endsWith('Later.'),false);assert.equal(view.state.field(mentionField).length,0);assert.equal(notice.hidden,false);
  notice.querySelector('button').click();assert.equal(view.state.field(mentionField)[0].id,'second');assert.match(notice.textContent,/1 link removed/);
  notice.querySelector('button').click();assert.deepEqual(view.state.field(mentionField),mentions);assert.equal(notice.hidden,true);ui.close();
});

test('link-only close guard keeps draft; Save confirms full aggregate and updates decorations', async()=>{
  let sent, revision=0;const ui=connectSetup(async(cmd,args)=>{if(cmd==='save_page'){sent=args.request;return {...sent,body:sent.expectedBody,title:'Page',mentions:sent.expectedMentions,revision:++revision};}});
  ui.open('a');await tick();await picker(ui);choose(ui);const view=ui.view(),id=view.state.field(mentionField)[0].id;
  ui.controller.requestClose('x');await tick();assert.equal(ui.doc.querySelector('#guard').open,true);ui.doc.querySelector('#continue').click();await tick();assert.equal(view.state.field(mentionField)[0].id,id);
  ui.doc.querySelector('#save').click();await tick();assert.equal(sent.expectedMentions[0].id,id);assert.match(ui.doc.querySelector('#save-status').textContent,/^Saved$/);assert.ok(ui.doc.querySelector('.entity-mention.saved'));
  view.dispatch({changes:{from:2,insert:'x'}});assert.equal(ui.doc.querySelector('.link-drop-notice').hidden,false);ui.doc.querySelector('#save').click();await tick();assert.equal(ui.doc.querySelector('.link-drop-notice').hidden,true);ui.close();
});

test('stale, busy, mismatch and unknown save outcomes retain mentions and refuse new links', async()=>{
  for(const kind of ['stale','busy','mention_mismatch','unknown']) {
    const ui=connectSetup(async(cmd,args)=>{if(cmd==='save_page')throw kind==='unknown'?{}:{...args.request,kind};});
    ui.open('a');await tick();await picker(ui);choose(ui);const view=ui.view(),original={...view.state.field(mentionField)[0]};ui.doc.querySelector('#save').click();await tick();
    assert.deepEqual(view.state.field(mentionField),[original]);assert.doesNotMatch(ui.doc.querySelector('#save-status').textContent,/^Saved$/);
    view.dispatch({selection:{anchor:0,head:8}});key(ui,view.contentDOM,'k',true);await tick();assert.equal(ui.doc.querySelector('.entity-picker').hidden,true);ui.close();
  }
});

test('late entity index results cannot reopen canceled or switched picker', async()=>{
  const pending=deferred();const ui=setup(async(cmd,args)=>cmd==='page_index'?[{id:'a',title:'Page'},{id:'b',title:'Other'}]:cmd==='read_page'?{...page,id:args.pageId,body:'Valdoria'}:cmd==='entity_index'?pending.promise:undefined);
  ui.open('a');await tick();await picker(ui);key(ui,ui.view().contentDOM,'Escape');pending.resolve(entities);await tick();assert.equal(ui.doc.querySelector('.entity-picker').hidden,true);assert.equal(ui.view().state.field(mentionField).length,0);ui.close();
});

test('search filter resets choice by stable id; keyboard and pointer cannot pick old filtered row', async()=>{
  const ui=connectSetup();ui.open('a');await tick();const search=await picker(ui);key(ui,search,'ArrowDown');search.value='House';search.dispatchEvent(new ui.doc.defaultView.Event('input',{bubbles:true}));key(ui,search,'Enter');assert.equal(ui.view().state.field(mentionField).length,0);key(ui,search,'ArrowUp');key(ui,search,'Enter');assert.equal(ui.view().state.field(mentionField)[0].entityId,'house-0002');ui.close();
});

test('linked editor boundary edits and refused text transactions preserve exact draft and identity', async()=>{
  for(const [from,insert,expectedStart,drop] of [[0,'x',1,false],[8,'x',0,false],[3,'x',0,true],[8,'\u0301',0,true]]) {
    const held={id:'original',entityId:'place-0001',startUtf16:0,endUtf16:8};const ui=connectSetup(undefined,{...page,body:'Valdoria',mentions:[held]});ui.open('a');await tick();const view=ui.view();
    view.dispatch({changes:{from,insert}});assert.equal(view.state.field(mentionField).length,drop?0:1);assert.equal(ui.doc.querySelector('.link-drop-notice').hidden,!drop);
    if(!drop)assert.equal(view.state.field(mentionField)[0].startUtf16,expectedStart);
    undo(view);assert.deepEqual(view.state.field(mentionField),[held]);const before=view.state.doc.toString();view.dispatch({changes:{from:0,insert:'\u0000'}});assert.equal(view.state.doc.toString(),before);assert.deepEqual(view.state.field(mentionField),[held]);ui.close();
  }
});

test('link-only navigation supports explicit Discard and Save then proceed', async()=>{
  for(const choice of ['discard','save-proceed']) {
    let sent;const ui=connectSetup(async(cmd,args)=>{if(cmd==='save_page'){sent=args.request;return {...sent,body:sent.expectedBody,mentions:sent.expectedMentions,title:'Page',revision:1};}});
    ui.open('a');await tick();await picker(ui);choose(ui);ui.doc.querySelectorAll('.page-link')[1].click();await tick();assert.equal(ui.doc.querySelector('#guard').open,true);ui.doc.querySelector('#'+choice).click();await tick();assert.equal(ui.doc.querySelector('#context-name').textContent,'Page');assert.equal(ui.view().state.field(mentionField).length,0);assert.equal(!!sent,choice==='save-proceed');ui.close();
  }
});
