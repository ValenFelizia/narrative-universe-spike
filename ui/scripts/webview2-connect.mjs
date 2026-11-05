import { spawn, execFileSync } from 'node:child_process';
import { writeFileSync, mkdirSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
const ui = fileURLToPath(new URL('..', import.meta.url));
const evidence = fileURLToPath(new URL('../../docs/evidence/connect-native/', import.meta.url));
mkdirSync(evidence, {recursive:true});
const database = join(mkdtempSync(join(tmpdir(), 'connect-native-')), 'synthetic.sqlite');
execFileSync(fileURLToPath(new URL('../../target/debug/spike.exe', import.meta.url)), ['import-new', database, fileURLToPath(new URL('../../tests/fixtures/connect-smoke.json', import.meta.url))]);
const vite = spawn(process.execPath, ['node_modules/vite/bin/vite.js','--config','desktop/vite.config.ts'],{cwd:ui,stdio:'ignore',windowsHide:true});
let app;
const pause = ms => new Promise(r=>setTimeout(r,ms));
async function target() {
  for(let i=0;i<120;i++) {
    try { const pages=await (await fetch('http://127.0.0.1:9334/json/list')).json();const page=pages.find(p=>p.type==='page'&&/5173/.test(p.url));if(page)return page.webSocketDebuggerUrl; } catch {}
    await pause(500);
  }
  throw Error('No native WebView2 target');
}
let seq=0;
function command(url,method,params={}) { return new Promise((resolve,reject)=>{const ws=new WebSocket(url),id=++seq;const timeout=setTimeout(()=>{ws.close();reject(Error('CDP timeout '+method));},30000);ws.onopen=()=>ws.send(JSON.stringify({id,method,params}));ws.onmessage=e=>{const msg=JSON.parse(e.data);if(msg.id!==id)return;clearTimeout(timeout);ws.close();if(msg.error||msg.result?.exceptionDetails)reject(Error(JSON.stringify(msg)));else resolve(msg.result);};ws.onerror=reject;}); }
const evaluate = async (url,expression) => (await command(url,'Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true})).result.value;
const imports = `const {EditorView}=await import('/@fs/'+${JSON.stringify(ui.replaceAll('\\','/').replace(/\/$/, ''))}+'/node_modules/@codemirror/view/dist/index.js'); const {mentionField}=await import('/@fs/'+${JSON.stringify(ui.replaceAll('\\','/').replace(/\/$/, ''))}+'/src/mention-history.ts');`;
try {
  for(let i=0;i<80;i++){try{if((await fetch('http://127.0.0.1:5173/')).ok)break;}catch{}await pause(250);}
  app=spawn(fileURLToPath(new URL('../src-tauri/target/debug/narrative-universe-desktop.exe',import.meta.url)),[],{cwd:ui,stdio:'ignore',windowsHide:true,env:{...process.env,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port=9334'}});
  let url=await target();
  const result=await evaluate(url,`(async()=>{${imports}
    const wait=()=>new Promise(r=>setTimeout(r,150));const assert=(v,m)=>{if(!v)throw Error(m)};
    document.querySelector('#database-path').value=${JSON.stringify(database)};document.querySelector('#open-form').requestSubmit();await wait();await wait();
    let view=EditorView.findFromDOM(document.querySelector('.cm-editor'));assert(view,'native editor');
    const from=view.state.doc.toString().indexOf('Valdoria');view.dispatch({selection:{anchor:from,head:from+8}});await wait();
    const action=document.querySelector('.link-entity-action');assert(!action.hidden,'visible contextual action');action.click();await wait();
    const rows=[...document.querySelectorAll('[data-entity-id]')];assert(rows.length===3,'duplicate candidates');assert(rows.filter(r=>r.textContent.includes('Place')).every(r=>r.textContent.includes('…')),'exact tie suffix');
    return {userAgent:navigator.userAgent,width:innerWidth,height:innerHeight,dpr:devicePixelRatio,from,candidates:rows.map(r=>r.textContent)};
  })()`);
  const screenshot=async name=>{const png=await command(url,'Page.captureScreenshot',{format:'png'});writeFileSync(join(evidence,name),Buffer.from(png.data,'base64'));};
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
  await new Promise(resolve => { app.once('exit', resolve); app.kill(); });
  app=spawn(fileURLToPath(new URL('../src-tauri/target/debug/narrative-universe-desktop.exe',import.meta.url)),[],{cwd:ui,stdio:'ignore',windowsHide:true,env:{...process.env,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port=9334'}});
  url=await target();
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
  writeFileSync(join(evidence,'result.json'),JSON.stringify({...result,...linked,synthetic:true},null,2)+'\n');
  console.log(JSON.stringify({...result,...linked,synthetic:true},null,2));
} finally { app?.kill();vite.kill(); }
