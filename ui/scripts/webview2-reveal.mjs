import { execFileSync } from 'node:child_process';
import { writeFileSync, mkdirSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  cdpCommand,
  editorImports,
  evaluate,
  resizeWindow,
  spawnDesktopApp,
  spikeExe,
  startVite,
  uiDir,
  waitForTarget,
  waitPageReady,
} from './webview2-harness.mjs';

const evidence = fileURLToPath(new URL('../../docs/evidence/reveal-native/', import.meta.url));
mkdirSync(evidence, {recursive:true});
const database = join(mkdtempSync(join(tmpdir(), 'reveal-native-')), 'synthetic.sqlite');
const webviewProfile = mkdtempSync(join(tmpdir(), 'reveal-webview-'));
execFileSync(spikeExe, ['import-new', database, fileURLToPath(new URL('../../tests/fixtures/reveal-facts.json', import.meta.url))]);
let vite;
let app;
const port = 9334;
// Focus is asserted on the DOM (activeElement), not view.hasFocus: the native window may not be OS-foreground during capture.
const imports = editorImports(uiDir);
const assert = (value, message) => {if(!value) throw Error(message)};
try {
  vite = await startVite({ cwd: uiDir });
  app = spawnDesktopApp({ port, userDataDir: webviewProfile, cwd: uiDir });
  const url = await waitForTarget(port);
  await waitPageReady(url);
  const run = body => evaluate(url,`(async()=>{${imports} const wait=()=>new Promise(r=>setTimeout(r,250));const assert=(v,m)=>{if(!v)throw Error(m)}; ${body} })()`);
  const screenshot=async name=>{const png=await cdpCommand(url,'Page.captureScreenshot',{format:'png'});writeFileSync(join(evidence,name),Buffer.from(png.data,'base64'));};
  await run(`document.querySelector('#database-path').value=${JSON.stringify(database)};document.querySelector('#open-form').requestSubmit();await wait();await wait();assert(window.__TAURI_INTERNALS__,'actual native bridge');`);
  const results=[];
  for(const [width,height] of [[1120,780],[800,600]]) {
    await resizeWindow(url, app.pid, width, height);

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
      const rect=popup.getBoundingClientRect();assert(rect.left>=0&&rect.top>=0&&rect.right<=innerWidth&&rect.bottom<=innerHeight,'viewport fit');assert(rect.height<=0.6*innerHeight,'compact peek '+rect.height);
      assert(column===document.querySelector('.cm-content').getBoundingClientRect().width,'no column reflow');assert(pane.scrollTop===scroll,'no scroll change');
      assert(view.state.doc===doc&&view.state.selection.eq(selection),'doc selection unchanged');assert(JSON.stringify(view.state.field(mentionField))===held,'mention identity');assert(document.querySelector('#save-status').textContent==='Unsaved','dirty retained');
      return {width:innerWidth,height:innerHeight,dpr:devicePixelRatio,proseWidth:column,popup:{left:rect.left,top:rect.top,right:rect.right,bottom:rect.bottom,height:rect.height,viewportShare:Math.round(rect.height/innerHeight*100)/100}};
    `);
    assert(result.width===width&&result.height===height,'actual window dimensions '+JSON.stringify(result));
    await screenshot(`mara-${width}.png`);
    await run(`const popup=document.querySelector('.entity-picker');popup.querySelector('details').open=true;await wait();const rect=popup.getBoundingClientRect();assert(rect.top>=0&&rect.bottom<=innerHeight,'expanded source viewport fit');assert(rect.height<=0.6*innerHeight,'expanded source stays compact');`);
    await screenshot(`source-${width}.png`);

    await run(`const view=EditorView.findFromDOM(document.querySelector('.cm-editor'));const selection=view.state.selection;document.querySelector('.entity-picker').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));assert(document.activeElement===view.contentDOM&&view.state.selection.eq(selection),'escape focus/selection');view.dispatch({changes:{from:view.state.doc.length,insert:' After facts.'}});document.querySelector('#save').click();await wait();assert(document.querySelector('#save-status').textContent==='Saved','save');document.querySelector('#open-form').requestSubmit();await wait();await wait();const reopened=EditorView.findFromDOM(document.querySelector('.cm-editor'));assert(reopened.state.doc.toString().includes('Before facts. After facts.'),'both edits persisted');assert(reopened.state.field(mentionField)[0].id==='00000000-0000-4000-8000-000000000101','same persisted link');
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
  await run(`const view=EditorView.findFromDOM(document.querySelector('.cm-editor'));const before=document.querySelector('.reading-area').scrollTop;document.querySelector('.entity-picker').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true,cancelable:true}));assert(document.activeElement===view.contentDOM&&document.querySelector('.reading-area').scrollTop===before,'long page escape');`);
  const result={synthetic:true,native:true,userAgent:await evaluate(url,'navigator.userAgent'),database:'<fresh temp dir>/synthetic.sqlite',fixture:'tests/fixtures/reveal-facts.json',windows:results,long,checks:['actual Tauri invoke against fresh v3 fixture','stored outgoing alternative/canon/provenance','incoming subject-first','1120x780 and 800x600 viewport fit and no column reflow','dirty draft, selection and mentions preserved','Escape focus','both edits saved and reopened with same mention id','long-page scroll stable'],manualRemaining:['physical keyboard and IME','human readability/focus traversal','Undo/Redo physical smoke']};
  writeFileSync(join(evidence,'result.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify(result,null,2));
} finally {app?.kill();vite?.kill();}
