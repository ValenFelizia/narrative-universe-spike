import { spawn, execFileSync } from 'node:child_process';
import { writeFileSync, mkdirSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
const ui = fileURLToPath(new URL('..', import.meta.url));
const evidence = fileURLToPath(new URL('../../docs/evidence/reveal-native/', import.meta.url));
mkdirSync(evidence, {recursive:true});
const database = join(mkdtempSync(join(tmpdir(), 'reveal-native-')), 'synthetic.sqlite');
const webviewProfile = mkdtempSync(join(tmpdir(), 'reveal-webview-'));
execFileSync(fileURLToPath(new URL('../../target/debug/spike.exe', import.meta.url)), ['import-new', database, fileURLToPath(new URL('../../tests/fixtures/reveal-facts.json', import.meta.url))]);
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
const assert = (value, message) => {if(!value) throw Error(message)};
try {
  for(let i=0;i<80;i++){try{if((await fetch('http://127.0.0.1:5173/')).ok)break;}catch{}await pause(250);}
  app=spawn(fileURLToPath(new URL('../src-tauri/target/debug/narrative-universe-desktop.exe',import.meta.url)),[],{cwd:ui,stdio:'ignore',windowsHide:true,env:{...process.env,WEBVIEW2_USER_DATA_FOLDER:webviewProfile,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port=9334'}});
  const url=await target();
  const run = body => evaluate(url,`(async()=>{${imports} const wait=()=>new Promise(r=>setTimeout(r,250));const assert=(v,m)=>{if(!v)throw Error(m)}; ${body} })()`);
  const screenshot=async name=>{const png=await command(url,'Page.captureScreenshot',{format:'png'});writeFileSync(join(evidence,name),Buffer.from(png.data,'base64'));};
  await run(`document.querySelector('#database-path').value=${JSON.stringify(database)};document.querySelector('#open-form').requestSubmit();await wait();await wait();assert(window.__TAURI_INTERNALS__,'actual native bridge');`);
  const results=[];
  for(const [width,height] of [[1120,780],[800,600]]) {
    const dpr=await evaluate(url,'devicePixelRatio');
    const resize=`Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;public class RevealWindow{[DllImport("user32.dll")]public static extern bool SetWindowPos(IntPtr h,IntPtr a,int x,int y,int w,int z,uint f);}'; $p=Get-Process -Id ${app.pid}; [RevealWindow]::SetWindowPos($p.MainWindowHandle,[IntPtr]::Zero,0,0,${Math.round(width*dpr)},${Math.round(height*dpr)},6)`;
    execFileSync('powershell.exe',['-NoProfile','-Command',resize],{windowsHide:true});await pause(350);
    const actual=await evaluate(url,'({width:innerWidth,height:innerHeight})');
    const fixed=resize.replace(`,${Math.round(width*dpr)},${Math.round(height*dpr)},6)`,`,${Math.round((2*width-actual.width)*dpr)},${Math.round((2*height-actual.height)*dpr)},6)`);
    execFileSync('powershell.exe',['-NoProfile','-Command',fixed],{windowsHide:true});await pause(350);

    if(width<=900) await run(`if(!document.querySelector('#library').hidden) document.querySelector('#library-toggle').click();`);
    const result=await run(`
      const view=EditorView.findFromDOM(document.querySelector('.cm-editor'));assert(view,'editor');
      const held=JSON.stringify(view.state.field(mentionField));
      view.dispatch({changes:{from:view.state.doc.length,insert:' Before facts.'}});
      view.dispatch({selection:{anchor:2}});view.focus();
      const doc=view.state.doc,selection=view.state.selection, pane=document.querySelector('.reading-area'),scroll=pane.scrollTop;
      const column=document.querySelector('.cm-content').getBoundingClientRect().width;
      view.contentDOM.dispatchEvent(new KeyboardEvent('keydown',{key:'k',ctrlKey:true,bubbles:true,cancelable:true}));await wait();
      [...document.querySelectorAll('.entity-picker button')].find(b=>b.textContent==='Show facts').click();await wait();
      const popup=document.querySelector('.entity-picker');assert(popup.textContent.includes('Mara Vey')&&popup.textContent.includes('Ashfall Order'),'stored relation');assert(popup.textContent.includes('PROVISIONAL')&&popup.textContent.includes('folio 3'),'canon/source');assert(popup.textContent.includes('Tidewardens')&&popup.textContent.includes('ABIERTO'),'alternative');
      const rect=popup.getBoundingClientRect();assert(rect.left>=0&&rect.top>=0&&rect.right<=innerWidth&&rect.bottom<=innerHeight,'viewport fit');
      assert(column===document.querySelector('.cm-content').getBoundingClientRect().width,'no column reflow');assert(pane.scrollTop===scroll,'no scroll change');
      assert(view.state.doc===doc&&view.state.selection.eq(selection),'doc selection unchanged');assert(JSON.stringify(view.state.field(mentionField))===held,'mention identity');assert(document.querySelector('#save-status').textContent==='Unsaved','dirty retained');
      return {width:innerWidth,height:innerHeight,dpr:devicePixelRatio,proseWidth:column,popup:{left:rect.left,top:rect.top,right:rect.right,bottom:rect.bottom}};
    `);
    assert(result.width===width&&result.height===height,'actual window dimensions '+JSON.stringify(result));
    await screenshot(`mara-${width}.png`);
    await run(`const popup=document.querySelector('.entity-picker');popup.querySelector('details').open=true;await wait();const rect=popup.getBoundingClientRect();assert(rect.top>=0&&rect.bottom<=innerHeight,'expanded source viewport fit');`);
    await screenshot(`source-${width}.png`);

    await run(`const view=EditorView.findFromDOM(document.querySelector('.cm-editor'));const selection=view.state.selection;document.querySelector('.entity-picker').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));assert(view.hasFocus&&view.state.selection.eq(selection),'escape focus/selection');view.dispatch({changes:{from:view.state.doc.length,insert:' After facts.'}});document.querySelector('#save').click();await wait();assert(document.querySelector('#save-status').textContent==='Saved','save');document.querySelector('#open-form').requestSubmit();await wait();await wait();const reopened=EditorView.findFromDOM(document.querySelector('.cm-editor'));assert(reopened.state.doc.toString().includes('Before facts. After facts.'),'both edits persisted');assert(reopened.state.field(mentionField)[0].id==='00000000-0000-4000-8000-000000000101','same persisted link');
      reopened.dispatch({selection:{anchor:38}});reopened.contentDOM.dispatchEvent(new KeyboardEvent('keydown',{key:'k',ctrlKey:true,bubbles:true,cancelable:true}));await wait();[...document.querySelectorAll('.entity-picker button')].find(b=>b.textContent==='Show facts').click();await wait();const text=document.querySelector('.entity-picker').textContent;assert(text.includes('Incoming references')&&text.includes('Mara Vey')&&text.includes('member of')&&text.includes('Ashfall Order'),'incoming original orientation');assert(!text.includes('has member'),'no guessed inverse');`);
    await screenshot(`ashfall-${width}.png`);
    await run(`document.querySelector('.entity-picker').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));`);
    results.push(result);
  }
  const long=await run(`
    const view=EditorView.findFromDOM(document.querySelector('.cm-editor'));const text='\\nSynthetic long scene. '.repeat(150);view.dispatch({changes:{from:0,insert:text}});view.dispatch({selection:{anchor:text.length+2},scrollIntoView:true});await wait();const pane=document.querySelector('.reading-area'),top=pane.scrollTop;const selection=view.state.selection, doc=view.state.doc;
    document.querySelector('.link-entity-action').click();await wait();[...document.querySelectorAll('.entity-picker button')].find(b=>b.textContent==='Show facts').click();await wait();assert(pane.scrollTop===top,'long page scroll stable');assert(view.state.doc===doc&&view.state.selection.eq(selection),'long doc stable');return {scrollTop:top};
  `);
  await screenshot('long-scene-800.png');
  await run(`const view=EditorView.findFromDOM(document.querySelector('.cm-editor'));const before=document.querySelector('.reading-area').scrollTop;document.querySelector('.entity-picker').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));assert(view.hasFocus&&document.querySelector('.reading-area').scrollTop===before,'long page escape');`);
  const result={synthetic:true,native:true,userAgent:await evaluate(url,'navigator.userAgent'),database,windows:results,long,checks:['actual Tauri invoke against fresh v3 fixture','stored outgoing alternative/canon/provenance','incoming subject-first','1120x780 and 800x600 viewport fit and no column reflow','dirty draft, selection and mentions preserved','Escape focus','both edits saved and reopened with same mention id','long-page scroll stable'],manualRemaining:['physical keyboard and IME','human readability/focus traversal','Undo/Redo physical smoke']};
  writeFileSync(join(evidence,'result.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify(result,null,2));
} finally {app?.kill();vite.kill();}
