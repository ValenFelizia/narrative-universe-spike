import { spawn, execFileSync } from 'node:child_process';
import { writeFileSync, mkdirSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ui = fileURLToPath(new URL('..', import.meta.url));
const evidence = fileURLToPath(new URL('../../docs/evidence/context-native/', import.meta.url));
mkdirSync(evidence, { recursive: true });

const workDir = mkdtempSync(join(tmpdir(), 'context-native-'));
const database = join(workDir, 'reveal-fixture.sqlite');
const syntheticJson = join(workDir, 'synthetic-context.json');
const syntheticDb = join(workDir, 'synthetic-context.sqlite');
const webviewProfile = mkdtempSync(join(tmpdir(), 'context-webview-'));
const spikeExe = fileURLToPath(new URL('../../target/debug/spike.exe', import.meta.url));

execFileSync(spikeExe, ['import-new', database, fileURLToPath(new URL('../../tests/fixtures/reveal-facts.json', import.meta.url))]);

writeFileSync(syntheticJson, JSON.stringify({
  format_version: 3,
  universe_id: '00000000-0000-4000-8000-000000000090',
  pages: [{
    id: '00000000-0000-4000-8000-000000000091',
    title: 'Archive and Self-Reference',
    body: 'Alden watches himself in the mirror.',
    mentions: [{
      id: '00000000-0000-4000-8000-000000000191',
      entity_id: '00000000-0000-4000-8000-000000000011',
      start_utf16: 0,
      end_utf16: 5,
    }],
  }],
  types: [{ id: '00000000-0000-4000-8000-000000000021', label: 'Person' }],
  entities: [{
    id: '00000000-0000-4000-8000-000000000011',
    label: 'Alden',
    type_id: '00000000-0000-4000-8000-000000000021',
    page_id: '00000000-0000-4000-8000-000000000091',
  }],
  definitions: [
    {
      id: '00000000-0000-4000-8000-000000000031',
      owner: { type: 'type', id: '00000000-0000-4000-8000-000000000021' },
      value_class: 'reference',
      cardinality: 'multiple',
      archived: false,
      label: 'sworn rival',
    },
    {
      id: '00000000-0000-4000-8000-000000000032',
      owner: { type: 'type', id: '00000000-0000-4000-8000-000000000021' },
      value_class: 'boolean',
      cardinality: 'singular',
      archived: false,
      label: 'oath bound',
    },
    {
      id: '00000000-0000-4000-8000-000000000033',
      owner: { type: 'entity', id: '00000000-0000-4000-8000-000000000011' },
      value_class: 'text',
      cardinality: 'singular',
      archived: true,
      label: 'former title',
    },
    {
      id: '00000000-0000-4000-8000-000000000034',
      owner: { type: 'entity', id: '00000000-0000-4000-8000-000000000011' },
      value_class: 'text',
      cardinality: 'singular',
      archived: true,
      label: 'unused retired slot',
    },
  ],
  assertions: [
    {
      id: '00000000-0000-4000-8000-000000000041',
      subject_id: '00000000-0000-4000-8000-000000000011',
      definition_id: '00000000-0000-4000-8000-000000000031',
      value: { kind: 'reference', target_id: '00000000-0000-4000-8000-000000000011' },
      canon: 'MISTERIO_INTENCIONAL',
      source_status_raw: null,
      provenance: { external_ref: 'mirror-notes', locator: 'entry 1', fragment: null, source_revision: null },
      scope_note: 'unresolved double identity',
      archived: false,
    },
    {
      id: '00000000-0000-4000-8000-000000000042',
      subject_id: '00000000-0000-4000-8000-000000000011',
      definition_id: '00000000-0000-4000-8000-000000000032',
      value: { kind: 'boolean', value: false },
      canon: 'PROVISIONAL',
      source_status_raw: null,
      provenance: { external_ref: null, locator: null, fragment: null, source_revision: null },
      scope_note: null,
      archived: true,
    },
    {
      id: '00000000-0000-4000-8000-000000000043',
      subject_id: '00000000-0000-4000-8000-000000000011',
      definition_id: '00000000-0000-4000-8000-000000000033',
      value: { kind: 'text', text: 'Warden of Glass' },
      canon: 'ACORDADO',
      source_status_raw: null,
      provenance: { external_ref: 'ledger-2', locator: null, fragment: null, source_revision: null },
      scope_note: null,
      archived: false,
    },
  ],
}));
execFileSync(spikeExe, ['import-new', syntheticDb, syntheticJson]);

const vite = spawn(process.execPath, ['node_modules/vite/bin/vite.js', '--config', 'desktop/vite.config.ts'], { cwd: ui, stdio: 'ignore', windowsHide: true });
let app;
const pause = ms => new Promise(r => setTimeout(r, ms));
async function target() {
  for (let i = 0; i < 120; i++) {
    try {
      const pages = await (await fetch('http://127.0.0.1:9335/json/list')).json();
      const page = pages.find(p => p.type === 'page' && /5173/.test(p.url));
      if (page) return page.webSocketDebuggerUrl;
    } catch {}
    await pause(500);
  }
  throw Error('No native WebView2 target');
}
let seq = 0;
function command(url, method, params = {}) {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(url), id = ++seq;
    const timeout = setTimeout(() => { ws.close(); reject(Error('CDP timeout ' + method)); }, 30000);
    ws.onopen = () => ws.send(JSON.stringify({ id, method, params }));
    ws.onmessage = e => {
      const msg = JSON.parse(e.data);
      if (msg.id !== id) return;
      clearTimeout(timeout); ws.close();
      if (msg.error || msg.result?.exceptionDetails) reject(Error(JSON.stringify(msg)));
      else resolve(msg.result);
    };
    ws.onerror = reject;
  });
}
const evaluate = async (url, expression) => (await command(url, 'Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })).result.value;
const imports = `const {EditorView}=await import('/@fs/'+${JSON.stringify(ui.replaceAll('\\', '/').replace(/\/$/, ''))}+'/node_modules/@codemirror/view/dist/index.js'); const {mentionField}=await import('/@fs/'+${JSON.stringify(ui.replaceAll('\\', '/').replace(/\/$/, ''))}+'/src/mention-history.ts');`;
const assert = (value, message) => { if (!value) throw Error(message); };

async function waitPageReady(url) {
  for (let i = 0; i < 80; i++) {
    try {
      const state = await evaluate(url, '({ready:document.readyState,href:location.href,hasDb:Boolean(document.querySelector("#database-path")),hasTauri:Boolean(window.__TAURI_INTERNALS__)})');
      if (state && state.ready === 'complete' && /5173/.test(state.href) && state.hasDb && state.hasTauri) return state;
    } catch {}
    await pause(200);
  }
  throw Error('Page never became ready on 5173');
}

async function resizeWindow(url, width, height) {
  const dpr = await evaluate(url, 'devicePixelRatio');
  const script = (w, h) => `Add-Type -TypeDefinition 'using System;using System.Text;using System.Runtime.InteropServices;public class ContextWin{public delegate bool EnumWindowsProc(IntPtr h,IntPtr l);[DllImport("user32.dll")]public static extern bool EnumWindows(EnumWindowsProc p,IntPtr l);[DllImport("user32.dll")]public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);[DllImport("user32.dll")]public static extern int GetClassName(IntPtr h,StringBuilder s,int n);[DllImport("user32.dll")]public static extern int GetWindowText(IntPtr h,StringBuilder s,int n);[DllImport("user32.dll")]public static extern bool SetWindowPos(IntPtr h,IntPtr a,int x,int y,int w,int z,uint f);public static IntPtr FindTauri(uint target){IntPtr found=IntPtr.Zero;EnumWindows((h,l)=>{uint pid;GetWindowThreadProcessId(h,out pid);if(pid==target){var c=new StringBuilder(256);var t=new StringBuilder(256);GetClassName(h,c,256);GetWindowText(h,t,256);if(c.ToString()=="Tauri Window"||t.ToString()=="Narrative Universe"){found=h;return false;}}return true;},IntPtr.Zero);return found;}}'; $h=[ContextWin]::FindTauri(${app.pid}); if($h -eq [IntPtr]::Zero){throw "No Tauri window handle for pid ${app.pid}"}; if(-not [ContextWin]::SetWindowPos($h,[IntPtr]::Zero,0,0,${Math.round(w * dpr)},${Math.round(h * dpr)},22)){throw "SetWindowPos failed"}`;
  execFileSync('powershell.exe', ['-NoProfile', '-Command', script(width, height)], { windowsHide: true });
  await pause(350);
  const actual = await evaluate(url, '({width:innerWidth,height:innerHeight})');
  if (actual.width !== width || actual.height !== height) {
    execFileSync('powershell.exe', ['-NoProfile', '-Command', script(2 * width - actual.width, 2 * height - actual.height)], { windowsHide: true });
    await pause(350);
  }
  const settled = await evaluate(url, '({width:innerWidth,height:innerHeight,dpr:devicePixelRatio})');
  assert(settled.width === width && settled.height === height, `window dimensions mismatch: ${JSON.stringify(settled)} vs ${width}x${height}`);
  return settled;
}

try {
  for (let i = 0; i < 80; i++) { try { if ((await fetch('http://127.0.0.1:5173/')).ok) break; } catch {} await pause(250); }
  app = spawn(fileURLToPath(new URL('../src-tauri/target/debug/narrative-universe-desktop.exe', import.meta.url)), [], {
    cwd: ui, stdio: 'ignore', windowsHide: true,
    env: { ...process.env, WEBVIEW2_USER_DATA_FOLDER: webviewProfile, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=9335' },
  });
  const url = await target();
  await waitPageReady(url);
  const run = body => evaluate(url, `(async()=>{${imports} const wait=()=>new Promise(r=>setTimeout(r,250));const assert=(v,m)=>{if(!v)throw Error(m)}; ${body} })()`);
  const screenshot = async name => { const png = await command(url, 'Page.captureScreenshot', { format: 'png' }); writeFileSync(join(evidence, name), Buffer.from(png.data, 'base64')); };

  await run(`document.querySelector('#database-path').value=${JSON.stringify(database)};document.querySelector('#open-form').requestSubmit();await wait();await wait();assert(window.__TAURI_INTERNALS__,'actual native bridge');`);

  const captures = [];

  // 1. 1600x900 wide viewport (library open & closed, live resize transition).
  const win1600 = await resizeWindow(url, 1600, 900);
  const wideOpen = await run(`
    const view=EditorView.findFromDOM(document.querySelector('.cm-editor'));
    if(document.querySelector('#library').hidden) document.querySelector('#library-toggle').click();
    await wait();
    view.dispatch({changes:{from:view.state.doc.length,insert:' Context check.'}});
    view.dispatch({selection:{anchor:2}});view.focus();
    const docBefore=document.querySelector('.document').getBoundingClientRect();
    const proseBefore=document.querySelector('.cm-content').getBoundingClientRect();
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown',{key:'k',ctrlKey:true,bubbles:true,cancelable:true}));await wait();
    [...document.querySelectorAll('.entity-picker button')].find(b=>b.textContent==='Show facts').click();await wait();
    [...document.querySelectorAll('.entity-picker button')].find(b=>b.textContent==='Explore context').click();await wait();
    const popup=document.querySelector('.entity-picker');
    assert(popup.dataset.contextLayout==='edge','wide uses V2 edge layout');
    assert(popup.textContent.includes('member of · …0031')&&popup.textContent.includes('member of · …0038'),'distinct same-label definitions');
    assert(popup.textContent.includes('oath sworn')&&popup.textContent.includes('Not recorded'),'applicable zero-claim definition');
    const archived=popup.querySelector('.context-archived');assert(archived&&archived.open===false,'archived closed by default');
    archived.open=true;archived.dispatchEvent(new Event('toggle'));
    const prov=popup.querySelector('.fact-row details');prov.open=true;prov.dispatchEvent(new Event('toggle'));await wait();
    assert(popup.textContent.includes('archived sigil')&&popup.textContent.includes('ash and salt'),'archived history shown');
    const docAfter=document.querySelector('.document').getBoundingClientRect();
    const proseAfter=document.querySelector('.cm-content').getBoundingClientRect();
    const rect=popup.getBoundingClientRect();
    assert(docBefore.left===docAfter.left&&docBefore.width===docAfter.width,'zero prose recentering/reflow');
    assert(proseBefore.width===proseAfter.width,'zero cm-content reflow');
    assert(rect.left>=docAfter.right+20,'clear separation from prose column');
    assert(rect.right<=innerWidth&&rect.top>=76&&rect.bottom<=innerHeight-44,'fits reading band');
    return {viewport:1600,library:'open',dpr:devicePixelRatio,layout:popup.dataset.contextLayout,proseWidth:proseAfter.width,docLeftBefore:docBefore.left,docLeftAfter:docAfter.left,popup:{left:rect.left,top:rect.top,right:rect.right,bottom:rect.bottom,width:rect.width,height:rect.height}};
  `);
  await screenshot('context-1600-library-open.png');
  captures.push({ ...win1600, ...wideOpen });

  const wideClosed = await run(`
    const popup=document.querySelector('.entity-picker');
    [...popup.querySelectorAll('button')].find(b=>b.textContent==='Back to facts').click();await wait();
    assert(popup.classList.contains('facts'),'Back to facts restores compact facts');
    [...popup.querySelectorAll('button')].find(b=>b.textContent==='Explore context').click();await wait();
    const docBefore=document.querySelector('.document').getBoundingClientRect();
    document.querySelector('#library-toggle').click();await wait();
    const docAfter=document.querySelector('.document').getBoundingClientRect();
    const rect=popup.getBoundingClientRect();
    assert(document.querySelector('#library').hidden===true,'library closed');
    assert(popup.dataset.contextLayout==='edge','still edge with library closed');
    assert(docBefore.left===docAfter.left&&docBefore.width===docAfter.width,'zero prose shift when library closes');
    assert(rect.left>=docAfter.right+20,'clear separation with library closed');
    return {viewport:1600,library:'closed',dpr:devicePixelRatio,layout:popup.dataset.contextLayout,proseWidth:document.querySelector('.cm-content').getBoundingClientRect().width,popup:{left:rect.left,top:rect.top,right:rect.right,bottom:rect.bottom,width:rect.width,height:rect.height}};
  `);
  await screenshot('context-1600-library-closed.png');
  captures.push({ ...win1600, ...wideClosed });

  // Live resize from 1600x900 down to 1120x780 while context is open!
  const win1120 = await resizeWindow(url, 1120, 780);
  const medClosed = await run(`
    await wait();
    const popup=document.querySelector('.entity-picker');
    assert(popup.dataset.contextLayout==='anchored','live resize transitions edge -> anchored');
    const rect=popup.getBoundingClientRect();
    assert(rect.left>=0&&rect.top>=0&&rect.right<=innerWidth&&rect.bottom<=innerHeight,'anchored 1120 fits viewport');
    return {viewport:1120,library:'closed',dpr:devicePixelRatio,layout:popup.dataset.contextLayout,proseWidth:document.querySelector('.cm-content').getBoundingClientRect().width,popup:{left:rect.left,top:rect.top,right:rect.right,bottom:rect.bottom,width:rect.width,height:rect.height}};
  `);
  captures.push({ ...win1120, ...medClosed });

  const medOpen = await run(`
    document.querySelector('#library-toggle').click();await wait();
    const popup=document.querySelector('.entity-picker');
    assert(document.querySelector('#library').hidden===false,'library open at 1120');
    assert(popup.dataset.contextLayout==='anchored','1120 library open uses anchored fallback');
    const rect=popup.getBoundingClientRect();
    assert(rect.left>=0&&rect.top>=0&&rect.right<=innerWidth&&rect.bottom<=innerHeight,'1120 library open fits viewport');
    return {viewport:1120,library:'open',dpr:devicePixelRatio,layout:popup.dataset.contextLayout,proseWidth:document.querySelector('.cm-content').getBoundingClientRect().width,popup:{left:rect.left,top:rect.top,right:rect.right,bottom:rect.bottom,width:rect.width,height:rect.height}};
  `);
  await screenshot('context-1120-library-open.png');
  captures.push({ ...win1120, ...medOpen });

  // Inspect Ashfall Order incoming grouping at 1120x780.
  await run(`
    const view=EditorView.findFromDOM(document.querySelector('.cm-editor'));
    document.querySelector('.entity-picker').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));
    assert(document.activeElement===view.contentDOM,'escape returns DOM focus');
    view.dispatch({selection:{anchor:38}});
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown',{key:'k',ctrlKey:true,bubbles:true,cancelable:true}));await wait();
    [...document.querySelectorAll('.entity-picker button')].find(b=>b.textContent==='Show facts').click();await wait();
    [...document.querySelectorAll('.entity-picker button')].find(b=>b.textContent==='Explore context').click();await wait();
    const text=document.querySelector('.entity-picker').textContent;
    assert(text.includes('Incoming references')&&text.includes('Mara Vey · member of · …0031 → Ashfall Order')&&text.includes('Corin Hale · member of · …0031 → Ashfall Order'),'incoming grouped subject-first');
  `);
  await screenshot('ashfall-1120.png');
  await run(`document.querySelector('.entity-picker').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));`);

  // 3. 800x600 small viewport (library closed, archived expanded, Escape focus/scroll restore, long near-bottom caret).
  const win800 = await resizeWindow(url, 800, 600);
  const small800 = await run(`
    if(!document.querySelector('#library').hidden) document.querySelector('#library-toggle').click();
    await wait();
    const view=EditorView.findFromDOM(document.querySelector('.cm-editor'));
    view.dispatch({selection:{anchor:2}});view.focus();
    const sel=view.state.selection, doc=view.state.doc;
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown',{key:'k',ctrlKey:true,bubbles:true,cancelable:true}));await wait();
    [...document.querySelectorAll('.entity-picker button')].find(b=>b.textContent==='Show facts').click();await wait();
    [...document.querySelectorAll('.entity-picker button')].find(b=>b.textContent==='Explore context').click();await wait();
    const popup=document.querySelector('.entity-picker');
    assert(popup.dataset.contextLayout==='anchored','800x600 uses anchored fallback');
    const archived=popup.querySelector('.context-archived');archived.open=true;archived.dispatchEvent(new Event('toggle'));await wait();
    const rect=popup.getBoundingClientRect();
    assert(rect.left>=0&&rect.top>=0&&rect.right<=innerWidth&&rect.bottom<=innerHeight,'800x600 fits viewport');
    assert(view.state.doc===doc&&view.state.selection.eq(sel),'doc/selection preserved');
    return {viewport:800,library:'closed',dpr:devicePixelRatio,layout:popup.dataset.contextLayout,proseWidth:document.querySelector('.cm-content').getBoundingClientRect().width,popup:{left:rect.left,top:rect.top,right:rect.right,bottom:rect.bottom,width:rect.width,height:rect.height}};
  `);
  await screenshot('context-800.png');
  captures.push({ ...win800, ...small800 });

  // Long scene + near-bottom caret placement at 800x600.
  const longNearBottom = await run(`
    const view=EditorView.findFromDOM(document.querySelector('.cm-editor'));
    document.querySelector('.entity-picker').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));
    const filler='\\nSynthetic long scene line for scrolling. '.repeat(150);
    view.dispatch({changes:{from:0,insert:filler}});
    const targetPos=filler.length+2;
    view.dispatch({selection:{anchor:targetPos},scrollIntoView:true});await wait();
    const pane=document.querySelector('.reading-area'), scrollBefore=pane.scrollTop;
    const caretCoords=view.coordsAtPos(targetPos);
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown',{key:'k',ctrlKey:true,bubbles:true,cancelable:true}));await wait();
    [...document.querySelectorAll('.entity-picker button')].find(b=>b.textContent==='Show facts').click();await wait();
    [...document.querySelectorAll('.entity-picker button')].find(b=>b.textContent==='Explore context').click();await wait();
    const popup=document.querySelector('.entity-picker'), rect=popup.getBoundingClientRect();
    assert(pane.scrollTop===scrollBefore,'long page scroll preserved');
    assert(rect.top>=0&&rect.bottom<=innerHeight,'near-bottom fits viewport');
    assert(rect.bottom<=caretCoords.top+4||rect.top>=caretCoords.bottom-4,'does not obscure caret line');
    return {viewport:800,scenario:'long-near-bottom',dpr:devicePixelRatio,scrollTop:scrollBefore,caretTop:caretCoords.top,caretBottom:caretCoords.bottom,popup:{left:rect.left,top:rect.top,right:rect.right,bottom:rect.bottom,height:rect.height}};
  `);
  await screenshot('long-near-bottom-800.png');
  captures.push({ ...win800, ...longNearBottom });

  // Test synthetic DB with active-def archived claim, archived-def zero-claim, unarchived claim, and self-reference at 1600x900.
  await resizeWindow(url, 1600, 900);
  await run(`
    document.querySelector('.entity-picker').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));
    document.querySelector('#database-path').value=${JSON.stringify(syntheticDb)};
    document.querySelector('#open-form').requestSubmit();await wait();
    document.querySelector('#discard').click();await wait();await wait();
    if(document.querySelector('#library').hidden) document.querySelector('#library-toggle').click();
    await wait();
    const view=EditorView.findFromDOM(document.querySelector('.cm-editor'));
    view.dispatch({selection:{anchor:2}});view.focus();
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown',{key:'k',ctrlKey:true,bubbles:true,cancelable:true}));await wait();
    [...document.querySelectorAll('.entity-picker button')].find(b=>b.textContent==='Show facts').click();await wait();
    [...document.querySelectorAll('.entity-picker button')].find(b=>b.textContent==='Explore context').click();await wait();
    const popup=document.querySelector('.entity-picker');
    const archived=popup.querySelector('.context-archived');archived.open=true;archived.dispatchEvent(new Event('toggle'));await wait();
    const t=popup.textContent;
    assert(t.includes('oath bound')&&t.includes('Not recorded · Archived history available'),'active zero-claim with archived history');
    assert(t.includes('Alden · sworn rival → Alden'),'self-reference in both outgoing and incoming');
    assert(t.includes('Warden of Glass')&&t.includes('Claim unarchived (under archived definition)'),'unarchived claim under archived def');
    assert(t.includes('unused retired slot')&&t.includes('Archived · No assertions recorded'),'zero-claim archived def');
  `);
  await screenshot('synthetic-archive-selfref-1600.png');

  const result = {
    synthetic: true,
    native: true,
    userAgent: await evaluate(url, 'navigator.userAgent'),
    database: '<fresh temp dir>/reveal-fixture.sqlite & synthetic-context.sqlite',
    fixture: 'tests/fixtures/reveal-facts.json + temp synthetic archive/self-ref fixture',
    captures,
    checks: [
      'actual Tauri invoke against fresh v3 databases',
      'V2 edge layout at 1600x900 (library open & closed) with zero prose reflow/recentering',
      'live resize transition edge <-> anchored between 1600x900 and 1120x780 / 800x600',
      'V1 anchored fallback at 1120x780 and 800x600 including near-bottom caret flipping above caret',
      'Back to facts state restoration without refetching',
      'Escape focus, selection, scroll and dirty draft preservation',
      'archived history closed by default, independent archive bits, zero-claim archived defs, and self-references',
    ],
    manualRemaining: [
      'physical keyboard and IME traversal (Tab/Shift+Tab/Escape)',
      'human readability and contrast check on real display scaling (100% and 125%+ DPI)',
      'physical outside pointer click without stealing focus from clicked target',
    ],
  };
  writeFileSync(join(evidence, 'result.json'), JSON.stringify(result, null, 2) + '\n');
  console.log(JSON.stringify(result, null, 2));
} finally {
  app?.kill();
  vite.kill();
}
