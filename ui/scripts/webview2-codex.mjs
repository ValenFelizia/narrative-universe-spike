import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  cdpCommand,
  editorImports,
  evaluate,
  resizeWindow,
  spawnDesktopApp,
  spikeExe,
  startVite,
  stopAppAndWait,
  uiDir,
  waitForTarget,
  waitPageReady,
} from "./webview2-harness.mjs";

const evidence = fileURLToPath(new URL("../../docs/evidence/codex-native/", import.meta.url));
mkdirSync(evidence, { recursive: true });

const workDir = mkdtempSync(join(tmpdir(), "codex-native-"));
const emptyJson = join(workDir, "empty-universe.json");
const emptyDb = join(workDir, "empty-universe.sqlite");
const revealDb = join(workDir, "reveal-fixture.sqlite");
const webviewProfile = mkdtempSync(join(tmpdir(), "codex-webview-"));

writeFileSync(
  emptyJson,
  JSON.stringify({
    format_version: 3,
    universe_id: "00000000-0000-4000-8000-000000000080",
    pages: [],
    types: [],
    entities: [],
    definitions: [],
    assertions: [],
  }),
);
execFileSync(spikeExe, ["import-new", emptyDb, emptyJson]);
execFileSync(spikeExe, [
  "import-new",
  revealDb,
  fileURLToPath(new URL("../../tests/fixtures/reveal-facts.json", import.meta.url)),
]);

let vite;
let app;
const port = 9336;
const imports = editorImports(uiDir);

try {
  vite = await startVite({ cwd: uiDir });
  app = spawnDesktopApp({ port, userDataDir: webviewProfile, cwd: uiDir });
  let url = await waitForTarget(port);
  await waitPageReady(url);

  const run = (body) =>
    evaluate(
      url,
      `(async()=>{${imports} const wait=()=>new Promise(r=>setTimeout(r,250));const assert=(v,m)=>{if(!v)throw Error(m)}; ${body} })()`,
    );
  const screenshot = async (name) => {
    const png = await cdpCommand(url, "Page.captureScreenshot", { format: "png" });
    writeFileSync(join(evidence, name), Buffer.from(png.data, "base64"));
  };

  // 1. Empty-document synthetic universe at 1120x780.
  const win1120 = await resizeWindow(url, app.pid, 1120, 780, { verify: true });
  await run(`
    document.querySelector('#database-path').value=${JSON.stringify(emptyDb)};
    document.querySelector('#open-form').requestSubmit();
    await wait();await wait();
    assert(window.__TAURI_INTERNALS__,'actual native bridge');
    assert(document.querySelector('#page-body').textContent.includes('no pages'),'zero-page empty universe');
    assert(!document.querySelector('#mode-codex').disabled,'Codex switch enabled after open');
    document.querySelector('#mode-codex').click();
    await wait();await wait();
    assert(!document.querySelector('#codex-workspace').hidden,'Codex workspace visible');
    assert(document.querySelector('#library-toggle').disabled,'Library toggle disabled in Codex');
    assert(!document.querySelector('#codex-list-empty').hidden,'empty Codex list state shown');
    assert(document.documentElement.scrollWidth<=innerWidth,'no horizontal overflow at 1120 empty');
  `);
  await screenshot("codex-1120-empty.png");

  const emptyResults = await run(`
    document.querySelector('#codex-new-type').click();
    document.querySelector('#codex-type-label-input').value='   ';
    document.querySelector('#codex-type-form').requestSubmit();
    await wait();
    assert(document.querySelector('#codex-type-dialog').open,'blank type keeps dialog open');
    assert(!document.querySelector('#codex-type-error').hidden,'blank type shows validation error');

    document.querySelector('#codex-type-label-input').value='  House  ';
    document.querySelector('#codex-type-form').requestSubmit();
    await wait();await wait();
    assert(!document.querySelector('#codex-type-dialog').open,'type dialog closes on create');
    const filterOpts=[...document.querySelectorAll('#codex-type-filter option')].map(o=>o.textContent);
    assert(filterOpts.includes('House (0)')&&filterOpts.includes('Untyped (0)'),'zero-entity custom type and Untyped in filter');

    document.querySelector('#codex-new-entity').click();
    assert(document.querySelector('#codex-entity-type-select').value==='','New entity defaults to Untyped');
    document.querySelector('#codex-entity-label-input').value='Ashfall Bell';
    document.querySelector('#codex-entity-form').requestSubmit();
    await wait();await wait();
    assert(document.querySelector('#codex-detail-title').textContent==='Ashfall Bell','created untyped entity selected');
    assert(document.querySelector('#codex-detail-meta').textContent.includes('Type: Untyped'),'created entity is Untyped');

    const houseOpt=[...document.querySelectorAll('#codex-type-filter option')].find(o=>o.textContent==='House (0)');
    document.querySelector('#codex-new-entity').click();
    document.querySelector('#codex-entity-label-input').value='Mara Vey';
    document.querySelector('#codex-entity-type-select').value=houseOpt.value;
    document.querySelector('#codex-entity-form').requestSubmit();
    await wait();await wait();

    document.querySelector('#codex-new-entity').click();
    document.querySelector('#codex-entity-label-input').value='Mara Vey';
    document.querySelector('#codex-entity-form').requestSubmit();
    await wait();await wait();

    const labels=[...document.querySelectorAll('.codex-entity-label')].map(n=>n.textContent);
    assert(labels.filter(l=>l.startsWith('Mara Vey · …')).length===2,'duplicate labels disambiguated by ID suffix');
    assert(document.querySelector('#mode-write').disabled===false,'can return to Write');
    document.querySelector('#mode-write').click();
    await wait();
    assert(document.querySelector('#page-body').textContent.includes('no pages'),'still 0 pages in Write');
    return {emptyLabels:labels,emptyFilterOpts:filterOpts};
  `);

  // 2. Rich fixture universe across 1600x900, 1120x780, 800x600 + draft preservation + Connect + relaunch.
  const win1600 = await resizeWindow(url, app.pid, 1600, 900, { verify: true });
  await run(`
    document.querySelector('#database-path').value=${JSON.stringify(revealDb)};
    document.querySelector('#open-form').requestSubmit();
    await wait();await wait();
    const view=EditorView.findFromDOM(document.querySelector('.cm-editor'));
    assert(view,'Write editor ready');
    const titleInput=document.querySelector('#title-input');
    titleInput.value='Night at the ford (draft)';
    titleInput.dispatchEvent(new Event('input',{bubbles:true}));
    const filler='\\n\\nShe watched the rain sweep across the harbor steps.'.repeat(25)+'\\n\\nUnsaved mist hides the Sable Lantern.';
    view.dispatch({changes:{from:view.state.doc.length,insert:filler}});
    const pane=document.querySelector('.reading-area');
    pane.scrollTop=220;
    await wait();
    assert(pane.scrollTop===220,'Write pane scrolled before Codex switch');
    assert(document.querySelector('#save-status').textContent==='Unsaved','Write is dirty');

    document.querySelector('#mode-codex').click();
    await wait();await wait();
    document.dispatchEvent(new KeyboardEvent('keydown',{key:'s',ctrlKey:true,bubbles:true,cancelable:true}));
    await wait();

    const maraBtn=[...document.querySelectorAll('.codex-entity-item')].find(b=>b.querySelector('.codex-entity-label')?.textContent==='Mara Vey');
    assert(maraBtn,'Mara Vey listed in Codex');
    maraBtn.click();
    await wait();await wait();
    assert(document.querySelector('#codex-detail-title').textContent==='Mara Vey','Mara detail title');
    assert(document.querySelector('#codex-detail-meta').textContent.includes('Primary document: Night at the ford'),'primary document metadata shown');
    const ctxText=document.querySelector('#codex-detail-context').textContent;
    assert(ctxText.includes('member of · …0031')&&ctxText.includes('member of · …0038'),'disambiguated definitions');
    assert(ctxText.includes('oath sworn')&&ctxText.includes('Not recorded'),'zero-claim applicable property');
    const archived=document.querySelector('#codex-detail-context .context-archived');
    assert(archived&&archived.open===false,'archived closed by default');
    archived.open=true;archived.dispatchEvent(new Event('toggle'));
    const prov=document.querySelector('#codex-detail-context .fact-row details');
    prov.open=true;prov.dispatchEvent(new Event('toggle'));
    await wait();
    assert(document.documentElement.scrollWidth<=innerWidth,'no horizontal overflow at 1600');
  `);
  await screenshot("codex-1600-mara.png");

  const created = await run(`
    document.querySelector('#codex-new-type').click();
    document.querySelector('#codex-type-label-input').value='Chronicle';
    document.querySelector('#codex-type-form').requestSubmit();
    await wait();await wait();
    const chronicleOpt=[...document.querySelectorAll('#codex-type-filter option')].find(o=>o.textContent==='Chronicle (0)');
    assert(chronicleOpt,'Chronicle type created with 0 entities');

    document.querySelector('#codex-new-entity').click();
    document.querySelector('#codex-entity-label-input').value='Sable Lantern';
    document.querySelector('#codex-entity-type-select').value=chronicleOpt.value;
    await wait();
    return {chronicleId:chronicleOpt.value};
  `);
  await screenshot("codex-1600-entity-dialog.png");

  const entityCreated = await run(`
    document.querySelector('#codex-entity-form').requestSubmit();
    await wait();await wait();
    const lanternBtn=[...document.querySelectorAll('.codex-entity-item')].find(b=>b.querySelector('.codex-entity-label')?.textContent==='Sable Lantern');
    assert(lanternBtn,'Sable Lantern listed after create');
    assert(document.querySelector('#codex-detail-title').textContent==='Sable Lantern','Sable Lantern selected');
    assert(document.querySelector('#codex-detail-meta').textContent.includes('Type: Chronicle'),'Sable Lantern typed as Chronicle');
    return {sableLanternId:lanternBtn.dataset.entityId};
  `);

  // Inspect Ashfall Order at 1120x780 via search.
  await resizeWindow(url, app.pid, 1120, 780, { verify: true });
  await run(`
    const search=document.querySelector('#codex-search');
    search.value='Ashfall';
    search.dispatchEvent(new Event('input',{bubbles:true}));
    await wait();
    const ashfallBtn=[...document.querySelectorAll('.codex-entity-item')].find(b=>b.querySelector('.codex-entity-label')?.textContent==='Ashfall Order');
    assert(ashfallBtn,'search finds Ashfall Order');
    ashfallBtn.click();
    await wait();await wait();
    const text=document.querySelector('#codex-detail-context').textContent;
    assert(text.includes('Incoming references')&&text.includes('Mara Vey · member of · …0031 → Ashfall Order'),'Ashfall Order incoming references shown');
    assert(document.documentElement.scrollWidth<=innerWidth,'no horizontal overflow at 1120');
  `);
  await screenshot("codex-1120-ashfall.png");

  // Responsive check at 800x600 with filter active.
  const win800 = await resizeWindow(url, app.pid, 800, 600, { verify: true });
  await run(`
    const search=document.querySelector('#codex-search');
    search.value='';
    search.dispatchEvent(new Event('input',{bubbles:true}));
    const filter=document.querySelector('#codex-type-filter');
    filter.value=${JSON.stringify(created.chronicleId)};
    filter.dispatchEvent(new Event('change'));
    await wait();
    const rows=[...document.querySelectorAll('.codex-entity-item')];
    assert(rows.length===1&&rows[0].dataset.entityId===${JSON.stringify(entityCreated.sableLanternId)},'type filter narrows to Chronicle');
    rows[0].click();
    await wait();
    assert(document.documentElement.scrollWidth<=innerWidth,'no horizontal overflow at 800x600');
  `);
  await screenshot("codex-800-stacked.png");

  // Return to Write, check preserved draft/scroll/undo, link newly created Sable Lantern via Connect, Save.
  await resizeWindow(url, app.pid, 1120, 780, { verify: true });
  const writeVerify = await run(`
    document.querySelector('#mode-write').click();
    await wait();await wait();
    const view=EditorView.findFromDOM(document.querySelector('.cm-editor'));
    const pane=document.querySelector('.reading-area');
    const titleInput=document.querySelector('#title-input');
    assert(titleInput.value==='Night at the ford (draft)','draft title preserved');
    assert(document.querySelector('#context-name').textContent==='Night at the ford (draft)','context-name restored to draft title');
    assert(view.state.doc.toString().includes('Unsaved mist hides the Sable Lantern.'),'draft prose preserved');
    assert(pane.scrollTop===220,'Write scrollTop preserved across Codex');
    assert(document.querySelector('#save-status').textContent==='Unsaved','Write stayed dirty (Ctrl+S in Codex ignored)');

    view.focus();
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown',{key:'z',code:'KeyZ',ctrlKey:true,bubbles:true,cancelable:true}));
    await wait();
    const afterUndo=view.state.doc.toString();
    assert(!afterUndo.includes('Unsaved mist hides the Sable Lantern.'),'Undo works after Codex round-trip');
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown',{key:'y',code:'KeyY',ctrlKey:true,bubbles:true,cancelable:true}));
    await wait();
    assert(view.state.doc.toString().includes('Unsaved mist hides the Sable Lantern.'),'Redo restores draft');

    const full=view.state.doc.toString();
    const from=full.lastIndexOf('Sable Lantern');
    view.dispatch({selection:{anchor:from,head:from+13}});
    await wait();
    document.querySelector('.link-entity-action').click();
    await wait();
    const option=[...document.querySelectorAll('#entity-results button')].find(b=>b.dataset.entityId===${JSON.stringify(entityCreated.sableLanternId)});
    assert(option&&option.textContent.includes('Sable Lantern')&&option.textContent.includes('Chronicle'),'Connect immediately lists Codex-created entity');
    option.click();
    await wait();
    document.querySelector('#save').click();
    await wait();await wait();
    assert(document.querySelector('#save-status').textContent==='Saved','Write save succeeds with link to Codex entity');
    return {linkedMentionCount:view.state.field(mentionField).length,mentionIds:view.state.field(mentionField).map(m=>m.entityId)};
  `);

  // Relaunch desktop binary to verify SQLite persistence of type, entity, and linked Write page.
  await stopAppAndWait(app);
  app = spawnDesktopApp({ port, userDataDir: webviewProfile, cwd: uiDir });
  url = await waitForTarget(port);
  await waitPageReady(url);
  await resizeWindow(url, app.pid, 1120, 780, { verify: true });

  const relaunchVerify = await evaluate(
    url,
    `(async()=>{${imports}
      const wait=()=>new Promise(r=>setTimeout(r,250));const assert=(v,m)=>{if(!v)throw Error(m)};
      document.querySelector('#database-path').value=${JSON.stringify(revealDb)};
      document.querySelector('#open-form').requestSubmit();
      await wait();await wait();
      const view=EditorView.findFromDOM(document.querySelector('.cm-editor'));
      assert(document.querySelector('#title-input').value==='Night at the ford (draft)','relaunch keeps saved title');
      assert(view.state.doc.toString().includes('Unsaved mist hides the Sable Lantern.'),'relaunch keeps saved prose');
      assert(view.state.field(mentionField).some(m=>m.entityId===${JSON.stringify(entityCreated.sableLanternId)}),'relaunch preserves mention to Codex-created entity');

      document.querySelector('#mode-codex').click();
      await wait();await wait();
      const filterOpts=[...document.querySelectorAll('#codex-type-filter option')].map(o=>({value:o.value,text:o.textContent}));
      assert(filterOpts.some(o=>o.value===${JSON.stringify(created.chronicleId)}&&o.text==='Chronicle (1)'),'relaunch keeps exact type UUID');
      const lantern=[...document.querySelectorAll('.codex-entity-item')].find(b=>b.dataset.entityId===${JSON.stringify(entityCreated.sableLanternId)});
      assert(lantern,'relaunch keeps exact entity UUID');
      lantern.click();
      await wait();
      assert(document.querySelector('#codex-detail-meta').textContent.includes(${JSON.stringify(entityCreated.sableLanternId)}),'detail header displays exact persisted entity ID');
      return {userAgent:navigator.userAgent,persistedTypeId:${JSON.stringify(created.chronicleId)},persistedEntityId:${JSON.stringify(entityCreated.sableLanternId)}};
    })()`,
  );

  const report = {
    synthetic: true,
    native: true,
    userAgent: relaunchVerify.userAgent,
    viewports: { win1600, win1120, win800 },
    emptyUniverse: emptyResults,
    createdIds: {
      chronicleId: created.chronicleId,
      sableLanternId: entityCreated.sableLanternId,
    },
    writeVerify,
    relaunchVerify,
    screenshots: [
      "codex-1120-empty.png",
      "codex-1600-mara.png",
      "codex-1600-entity-dialog.png",
      "codex-1120-ashfall.png",
      "codex-800-stacked.png",
    ],
    checks: [
      "empty-document synthetic database opens Codex with 0 pages and supports creating custom types and untyped/typed/duplicate entities",
      "client-side label validation rejects blank labels without closing dialog",
      "EntityContext inspection in Codex shows disambiguated definitions, zero-claim applicable properties, incoming references, and closed-by-default archives",
      "Write dirty title/prose/scroll/undo survives Codex creation and Ctrl+S guard",
      "Connect (Ctrl+K / contextual action) immediately offers Codex-created entity in Write",
      "full native process relaunch preserves exact type ID, entity ID, saved page prose, and mention link",
      "responsive layout fits 1600x900, 1120x780, and 800x600 without horizontal overflow",
    ],
  };

  writeFileSync(join(evidence, "result.json"), JSON.stringify(report, null, 2) + "\n");
  console.log(JSON.stringify(report, null, 2));
} finally {
  await stopAppAndWait(app);
  vite?.kill();
}
