import { execFileSync } from 'node:child_process';
import { writeFileSync, mkdirSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  cdpCommand,
  editorImports,
  evaluate,
  spawnDesktopApp,
  spikeExe,
  startVite,
  stopAppAndWait,
  uiDir,
  waitForTarget,
  waitPageReady,
} from './webview2-harness.mjs';

const evidence = fileURLToPath(new URL('../../docs/evidence/connect-native/', import.meta.url));
mkdirSync(evidence, {recursive:true});
const database = join(mkdtempSync(join(tmpdir(), 'connect-native-')), 'synthetic.sqlite');
const webviewProfile = mkdtempSync(join(tmpdir(), 'connect-webview-'));
execFileSync(spikeExe, ['import-new', database, fileURLToPath(new URL('../../tests/fixtures/connect-smoke.json', import.meta.url))]);
let vite;
let app;
const port = 9334;
const imports = editorImports(uiDir);
try {
  vite = await startVite({ cwd: uiDir });
  app = spawnDesktopApp({ port, userDataDir: webviewProfile, cwd: uiDir });
  let url = await waitForTarget(port);
  await waitPageReady(url);
  const result=await evaluate(url,`(async()=>{${imports}
    const wait=()=>new Promise(r=>setTimeout(r,150));const assert=(v,m)=>{if(!v)throw Error(m)};
    document.querySelector('#database-path').value=${JSON.stringify(database)};document.querySelector('#open-form').requestSubmit();await wait();await wait();
    let view=EditorView.findFromDOM(document.querySelector('.cm-editor'));assert(view,'native editor');
    const from=view.state.doc.toString().indexOf('Valdoria');view.dispatch({selection:{anchor:from,head:from+8}});await wait();
    const action=document.querySelector('.link-entity-action');assert(!action.hidden,'visible contextual action');action.click();await wait();
    const rows=[...document.querySelectorAll('[data-entity-id]')];assert(rows.length===3,'duplicate candidates');assert(rows.filter(r=>r.textContent.includes('Place')).every(r=>r.textContent.includes('…')),'exact tie suffix');
    return {userAgent:navigator.userAgent,width:innerWidth,height:innerHeight,dpr:devicePixelRatio,from,candidates:rows.map(r=>r.textContent)};
  })()`);
  const screenshot=async name=>{const png=await cdpCommand(url,'Page.captureScreenshot',{format:'png'});writeFileSync(join(evidence,name),Buffer.from(png.data,'base64'));};
  await screenshot('picker.png');
  const linked=await evaluate(url,`(async()=>{${imports}
    const wait=()=>new Promise(r=>setTimeout(r,180));const assert=(v,m)=>{if(!v)throw Error(m)};const view=EditorView.findFromDOM(document.querySelector('.cm-editor'));
    document.querySelector('[data-entity-id="00000000-0000-4000-8000-000000000014"]').click();await wait();
    const original={...view.state.field(mentionField)[0]};assert(original,'link');document.querySelector('#save').click();await wait();await wait();assert(document.querySelector('#save-status').textContent==='Saved','saved aggregate');
    view.dispatch({changes:{from:original.startUtf16+2,insert:'x'}});assert(!document.querySelector('.link-drop-notice').hidden,'drop notice');view.contentDOM.dispatchEvent(new KeyboardEvent('keydown',{key:'z',code:'KeyZ',ctrlKey:true,bubbles:true,cancelable:true}));assert(JSON.stringify(view.state.field(mentionField)[0])===JSON.stringify(original),'undo exact identity');view.contentDOM.dispatchEvent(new KeyboardEvent('keydown',{key:'y',code:'KeyY',ctrlKey:true,bubbles:true,cancelable:true}));assert(!document.querySelector('.link-drop-notice').hidden,'redo notice');view.contentDOM.dispatchEvent(new KeyboardEvent('keydown',{key:'z',code:'KeyZ',ctrlKey:true,bubbles:true,cancelable:true}));
    return {original,checks:['native picker','explicit Place selection','manual Save','text-driven notice','Undo same id/entity/range','Redo notice']};
  })()`);
  await screenshot('linked.png');
  await evaluate(url,`(async()=>{${imports}const v=EditorView.findFromDOM(document.querySelector('.cm-editor'));v.dispatch({selection:{anchor:${result.from+1}}});v.contentDOM.dispatchEvent(new KeyboardEvent('keydown',{key:'k',ctrlKey:true,bubbles:true,cancelable:true}));await new Promise(r=>setTimeout(r,200));})()`);
  await screenshot('inspect.png');
  await stopAppAndWait(app);
  app = spawnDesktopApp({ port, userDataDir: webviewProfile, cwd: uiDir });
  url = await waitForTarget(port);
  await waitPageReady(url);
  const exactSelection = await evaluate(url, `(async()=>{${imports}
    const wait=()=>new Promise(r=>setTimeout(r,200));const assert=(v,m)=>{if(!v)throw Error(m)};
    document.querySelector('#database-path').value=${JSON.stringify(database)};document.querySelector('#open-form').requestSubmit();await wait();await wait();
    const view=EditorView.findFromDOM(document.querySelector('.cm-editor')),original=${JSON.stringify(linked.original)};
    const unchanged=()=>assert(JSON.stringify(view.state.field(mentionField))===JSON.stringify([original]),'exact selection retains mention identity');
    unchanged();
    for(const shortcut of [true,false]) {
      view.dispatch({selection:{anchor:original.startUtf16,head:original.endUtf16}});
      const captured=view.state.selection,action=document.querySelector('.link-entity-action');assert(!action.hidden&&action.textContent==='Inspect link','exact selection contextual Inspect link');
      if(shortcut)view.contentDOM.dispatchEvent(new KeyboardEvent('keydown',{key:'k',ctrlKey:true,bubbles:true,cancelable:true}));else action.click();await wait();
      const popup=document.querySelector('.entity-picker');assert(!popup.hidden&&popup.getAttribute('aria-label')==='Inspect link','exact selection inspector');assert(popup.textContent.includes('Saved link')&&popup.textContent.includes('Place'),'saved target inspector');assert(!popup.querySelector('[role="combobox"]'),'no new link search');unchanged();assert(document.querySelector('#save-status').textContent==='Saved','inspection leaves aggregate clean');
      popup.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));assert(view.hasFocus&&view.state.selection.eq(captured),'exact selection Escape focus and range');
    }
    view.dispatch({selection:{anchor:original.startUtf16,head:original.endUtf16-1}});assert(document.querySelector('.link-entity-action').hidden,'partial overlap hides action');view.contentDOM.dispatchEvent(new KeyboardEvent('keydown',{key:'k',ctrlKey:true,bubbles:true,cancelable:true}));await wait();assert(document.querySelector('.entity-picker').hidden,'partial overlap refuses popup');unchanged();
    view.dispatch({selection:{anchor:original.startUtf16,head:original.endUtf16}});document.querySelector('.link-entity-action').click();await wait();view.dispatch({selection:{anchor:0}});assert(document.querySelector('.entity-picker').hidden,'exact inspection selection drift cancels');unchanged();
    view.dispatch({selection:{anchor:original.startUtf16,head:original.endUtf16}});document.querySelector('.link-entity-action').click();await wait();
    return ['saved/relaunched exact selection inspects via Ctrl+K and contextual action','exact inspection preserves mention identity and clean aggregate','exact selection Escape restores focus and range','partial overlap remains refused','exact inspection cancels on selection drift'];
  })()`);
  await screenshot('exact-selection-inspect.png');
  const resumed = await evaluate(url, `(async()=>{${imports}
    const wait=()=>new Promise(r=>setTimeout(r,200));const assert=(v,m)=>{if(!v)throw Error(m)};
    document.querySelector('#database-path').value=${JSON.stringify(database)};document.querySelector('#open-form').requestSubmit();await wait();await wait();
    const view=EditorView.findFromDOM(document.querySelector('.cm-editor'));
    assert(view.state.field(mentionField)[0].id===${JSON.stringify(linked.original.id)},'process relaunch persisted same id');
    document.querySelector('.entity-picker').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));
    document.querySelector('#save').click();await wait();
    document.querySelector('#open-form').requestSubmit();await wait();await wait();
    const reopened=EditorView.findFromDOM(document.querySelector('.cm-editor'));assert(reopened.state.field(mentionField)[0].id===${JSON.stringify(linked.original.id)},'reopen saved identity');
    reopened.dispatch({selection:{anchor:${result.from+1}}});reopened.contentDOM.dispatchEvent(new KeyboardEvent('keydown',{key:'k',ctrlKey:true,bubbles:true,cancelable:true}));await wait();
    [...document.querySelectorAll('.entity-picker button')].find(b=>b.textContent==='Unlink').click();assert(reopened.state.field(mentionField).length===0,'deliberate unlink');assert(document.querySelector('.link-drop-notice').hidden,'no accidental drop');document.querySelector('#save').click();await wait();
    [...document.querySelectorAll('.page-link')].find(b=>b.textContent==='B long scene').click();await wait();
    const long=EditorView.findFromDOM(document.querySelector('.cm-editor'));long.dispatch({selection:{anchor:14,head:22}});long.focus();
    const before=document.querySelector('.cm-content').getBoundingClientRect().width;document.querySelector('.link-entity-action').click();await wait();const after=document.querySelector('.cm-content').getBoundingClientRect().width;assert(before===after,'prose width unchanged');
    document.querySelector('.entity-picker').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));assert(long.hasFocus&&long.state.selection.main.from===14,'Escape focus/selection');
    document.querySelector('.link-entity-action').click();await wait();document.querySelector('[data-entity-id="00000000-0000-4000-8000-000000000014"]').click();
    long.dispatch({changes:{from:16,insert:'x'}});await wait();
    const notice=document.querySelector('.link-drop-notice'),bounds=notice.getBoundingClientRect();
    assert(!notice.hidden&&bounds.top>=0&&bounds.bottom<=innerHeight,'long scene drop notice visible');
    assert(notice.querySelector('button').textContent==='Undo last change','ordinary Undo label');
    return {checks:['process relaunch persisted same mention id','save and reopen same mention id','inspect saved link','deliberate Unlink and Save','long scene picker preserves prose width','Escape restores focus and selection','long scene drop notice visible'],proseWidth:before};
  })()`);
  await screenshot('long-scene.png');
  linked.checks.push(...resumed.checks);
  linked.checks.push(...exactSelection);
  writeFileSync(join(evidence,'result.json'),JSON.stringify({...result,...linked,synthetic:true},null,2)+'\n');
  console.log(JSON.stringify({...result,...linked,synthetic:true},null,2));
} finally { app?.kill();vite?.kill(); }
