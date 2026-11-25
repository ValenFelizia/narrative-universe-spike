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

const evidence = fileURLToPath(new URL("../../docs/evidence/authoring-native/", import.meta.url));
mkdirSync(evidence, { recursive: true });

const workDir = mkdtempSync(join(tmpdir(), "authoring-native-"));
const synthJson = join(workDir, "ka2-fresh.json");
const synthDb = join(workDir, "ka2-fresh.sqlite");
const webviewProfile = mkdtempSync(join(tmpdir(), "authoring-webview-"));

writeFileSync(
  synthJson,
  JSON.stringify({
    format_version: 3,
    universe_id: "00000000-0000-4000-8000-000000000092",
    pages: [
      {
        id: "22222222-2222-4222-8222-000000000092",
        title: "Harbor Ledger",
        body: "Kestrel met Sable at the Ash Spire before the tide turned.",
        mentions: [],
      },
    ],
    types: [],
    entities: [],
    definitions: [],
    assertions: [],
  }),
);
execFileSync(spikeExe, ["import-new", synthDb, synthJson]);

let vite;
let app;
const port = 9337;
const imports = editorImports(uiDir);

try {
  vite = await startVite({ cwd: uiDir });
  app = spawnDesktopApp({ port, userDataDir: webviewProfile, cwd: uiDir });
  let url = await waitForTarget(port);
  await waitPageReady(url);

  const run = (body) =>
    evaluate(
      url,
      `(async()=>{${imports} const wait=()=>new Promise(r=>setTimeout(r,260));const assert=(v,m)=>{if(!v)throw Error(m)}; ${body} })()`,
    );
  const screenshot = async (name) => {
    const png = await cdpCommand(url, "Page.captureScreenshot", { format: "png" });
    writeFileSync(join(evidence, name), Buffer.from(png.data, "base64"));
  };

  // 1. 1600x900: Open synthetic universe with 0 preloaded knowledge, dirty Write, switch to Codex.
  const win1600 = await resizeWindow(url, app.pid, 1600, 900, { verify: true });
  const setupIds = await run(`
    document.querySelector('#database-path').value=${JSON.stringify(synthDb)};
    document.querySelector('#open-form').requestSubmit();
    await wait();await wait();
    assert(window.__TAURI_INTERNALS__,'native Tauri bridge active');
    const view=EditorView.findFromDOM(document.querySelector('.cm-editor'));
    assert(view,'Write editor ready');
    const titleInput=document.querySelector('#title-input');
    titleInput.value='Harbor Ledger (draft)';
    titleInput.dispatchEvent(new Event('input',{bubbles:true}));
    view.dispatch({changes:{from:view.state.doc.length,insert:'\\n\\nUnsaved lantern note.'}});
    assert(document.querySelector('#save-status').textContent==='Unsaved','Write is dirty');

    document.querySelector('#mode-codex').click();
    await wait();await wait();
    // Ctrl+S in Codex must not save Write draft.
    document.dispatchEvent(new KeyboardEvent('keydown',{key:'s',ctrlKey:true,bubbles:true,cancelable:true}));
    await wait();

    // Create custom type Order + typed Kestrel & Sable + untyped Ash Spire.
    document.querySelector('#codex-new-type').click();
    document.querySelector('#codex-type-label-input').value='Order';
    document.querySelector('#codex-type-form').requestSubmit();
    await wait();await wait();
    const orderOpt=[...document.querySelectorAll('#codex-type-filter option')].find(o=>o.textContent==='Order (0)');
    assert(orderOpt,'Order type created');

    const createEnt=async (label,typeId)=>{
      document.querySelector('#codex-new-entity').click();
      document.querySelector('#codex-entity-label-input').value=label;
      document.querySelector('#codex-entity-type-select').value=typeId||'';
      document.querySelector('#codex-entity-form').requestSubmit();
      await wait();await wait();
      const item=[...document.querySelectorAll('.codex-entity-item')].find(b=>b.querySelector('.codex-entity-label')?.textContent===label);
      assert(item,'created '+label);
      return item.dataset.entityId;
    };
    const kestrelId=await createEnt('Kestrel',orderOpt.value);
    const sableId=await createEnt('Sable',orderOpt.value);
    const spireId=await createEnt('Ash Spire','');

    // Select Kestrel and open New property dialog.
    document.querySelector('.codex-entity-item[data-entity-id="'+kestrelId+'"]').click();
    await wait();await wait();
    document.querySelector('#codex-new-property').click();
    await wait();
    // Verify strict rejection of surrounding whitespace (no silent trim).
    document.querySelector('#codex-prop-label-input').value='  Motto  ';
    document.querySelector('#codex-prop-val-text').value='Hold the lamp';
    document.querySelector('#codex-prop-form').requestSubmit();
    await wait();
    assert(document.querySelector('#codex-property-dialog').open,'surrounding whitespace rejected');
    assert(!document.querySelector('#codex-prop-error').hidden,'error shown for surrounding whitespace');

    // Fill valid type-owned Motto + provenance + scope for screenshot.
    document.querySelector('#codex-prop-label-input').value='Motto';
    document.querySelector('#codex-prop-owner-type').checked=true;
    document.querySelector('#codex-prop-val-text').value='Hold the lamp until dawn';
    document.querySelector('#codex-prop-source-details').open=true;
    document.querySelector('#codex-prop-src-ref').value='Draft Bible v2';
    document.querySelector('#codex-prop-src-locator').value='Ch. 1';
    document.querySelector('#codex-prop-src-scope').value='Before the Siege of Ashfall';
    await wait();
    return {orderId:orderOpt.value,kestrelId,sableId,spireId};
  `);
  await screenshot("authoring-1600-new-property-dialog.png");

  const authoredIds = await run(`
    // Submit atomic Motto property + first fact.
    document.querySelector('#codex-prop-form').requestSubmit();
    await wait();await wait();
    assert(!document.querySelector('#codex-property-dialog').open,'property dialog closed on commit');
    assert(document.querySelector('#codex-entry-status').textContent.includes('Saved property "Motto"'),'saved banner shown');

    // Create intentionally empty entity-owned Boolean property "Sworn".
    document.querySelector('#codex-new-property').click();
    document.querySelector('#codex-prop-label-input').value='Sworn';
    document.querySelector('#codex-prop-value-class').value='boolean';
    document.querySelector('#codex-prop-value-class').dispatchEvent(new Event('change'));
    document.querySelector('#codex-prop-include-fact').checked=false;
    document.querySelector('#codex-prop-include-fact').dispatchEvent(new Event('change'));
    document.querySelector('#codex-prop-form').requestSubmit();
    await wait();await wait();
    assert(document.querySelector('#codex-detail-context').textContent.includes('Not recorded'),'empty property shows Not recorded');

    // Add value: Boolean(false) -> "No" (Agreed) on Sworn.
    const swornGroup=[...document.querySelectorAll('.codex-prop-group')].find(g=>g.querySelector('.codex-prop-name')?.textContent==='Sworn');
    assert(swornGroup,'Sworn group rendered');
    const swornDefId=swornGroup.dataset.definitionId;
    swornGroup.querySelector('.codex-add-value-btn').click();
    document.querySelector('#codex-fact-val-bool-no').checked=true;
    document.querySelector('#codex-fact-canon').value='ACORDADO';
    document.querySelector('#codex-fact-form').requestSubmit();
    await wait();await wait();

    // Add competing value: explicit Unknown (Intentional mystery) on singular Sworn.
    document.querySelector('.codex-add-value-btn[data-definition-id="'+swornDefId+'"]').click();
    assert(!document.querySelector('#codex-fact-competing-note').hidden,'competing singular note shown');
    document.querySelector('#codex-fact-val-mode-unknown').checked=true;
    document.querySelector('#codex-fact-val-mode-unknown').dispatchEvent(new Event('change'));
    document.querySelector('#codex-fact-canon').value='MISTERIO_INTENCIONAL';
    document.querySelector('#codex-fact-form').requestSubmit();
    await wait();await wait();

    // Create entity-owned Reference property "Allied with" -> Ash Spire, then add self-reference Kestrel.
    document.querySelector('#codex-new-property').click();
    document.querySelector('#codex-prop-label-input').value='Allied with';
    document.querySelector('#codex-prop-value-class').value='reference';
    document.querySelector('#codex-prop-value-class').dispatchEvent(new Event('change'));
    document.querySelector('#codex-prop-card-multiple').checked=true;
    await wait();
    const spireChoice=document.querySelector('#codex-prop-val-ref-list .codex-ref-option[data-entity-id=${JSON.stringify(setupIds.spireId)}]');
    assert(spireChoice,'Ash Spire selectable in reference picker');
    spireChoice.click();
    document.querySelector('#codex-prop-form').requestSubmit();
    await wait();await wait();

    const alliedGroup=[...document.querySelectorAll('.codex-prop-group')].find(g=>g.querySelector('.codex-prop-name')?.textContent==='Allied with');
    assert(alliedGroup,'Allied with group rendered');
    const alliedDefId=alliedGroup.dataset.definitionId;
    alliedGroup.querySelector('.codex-add-value-btn').click();
    await wait();
    const selfChoice=document.querySelector('#codex-fact-val-ref-list .codex-ref-option[data-entity-id=${JSON.stringify(setupIds.kestrelId)}]');
    assert(selfChoice&&selfChoice.textContent.includes('(this entity)'),'self-reference labeled and selectable');
    selfChoice.click();
    document.querySelector('#codex-fact-canon').value='ABIERTO';
    document.querySelector('#codex-fact-form').requestSubmit();
    await wait();await wait();

    const ctxText=document.querySelector('#codex-detail-context').textContent;
    assert(ctxText.includes('Hold the lamp until dawn'),'Motto text shown');
    assert(ctxText.includes('No')&&ctxText.includes('Unknown (recorded)'),'Boolean false and Unknown distinct');
    assert(ctxText.includes('Ash Spire')&&ctxText.includes('Kestrel → Kestrel'),'reference and self-reference shown');
    assert(document.documentElement.scrollWidth<=innerWidth,'no horizontal overflow at 1600');
    const defIds=[...document.querySelectorAll('.codex-prop-group[data-definition-id]')].map(g=>g.dataset.definitionId);
    const assIds=[...document.querySelectorAll('.codex-assertion[data-assertion-id]')].map(a=>a.dataset.assertionId);
    return {swornDefId,alliedDefId,defIds,assIds};
  `);
  await screenshot("authoring-1600-kestrel-facts.png");

  // Verify shared type-owned Motto on sibling Sable, and incoming relation on target Ash Spire.
  await run(`
    document.querySelector('.codex-entity-item[data-entity-id=${JSON.stringify(setupIds.sableId)}]').click();
    await wait();await wait();
    const sableText=document.querySelector('#codex-detail-context').textContent;
    assert(sableText.includes('Motto')&&sableText.includes('Not recorded'),'shared type-owned property visible on sibling');

    document.querySelector('.codex-entity-item[data-entity-id=${JSON.stringify(setupIds.spireId)}]').click();
    await wait();await wait();
    const spireText=document.querySelector('#codex-detail-context').textContent;
    assert(spireText.includes('Referenced by')&&spireText.includes('Kestrel → Ash Spire'),'incoming relation visible on target');
    assert(document.querySelectorAll('.codex-incoming-group .codex-add-value-btn').length===0,'no authoring action on incoming group');
    document.querySelector('#codex-new-property').click();
    assert(document.querySelector('#codex-prop-owner-type-wrap').hidden,'untyped entity hides type ownership');
    document.querySelector('#codex-prop-cancel').click();
  `);

  // 2. 1120x780: Add value dialog on Kestrel's Sworn showing competing-claim note.
  const win1120 = await resizeWindow(url, app.pid, 1120, 780, { verify: true });
  await run(`
    document.querySelector('.codex-entity-item[data-entity-id=${JSON.stringify(setupIds.kestrelId)}]').click();
    await wait();await wait();
    document.querySelector('.codex-add-value-btn[data-definition-id=${JSON.stringify(authoredIds.swornDefId)}]').click();
    await wait();
    assert(document.documentElement.scrollWidth<=innerWidth,'no horizontal overflow at 1120');
  `);
  await screenshot("authoring-1120-add-value-dialog.png");
  await run(`document.querySelector('#codex-fact-cancel').click();`);

  // 3. 800x600 & 600x720 narrow checks.
  const win800 = await resizeWindow(url, app.pid, 800, 600, { verify: true });
  await run(`await wait(); assert(document.documentElement.scrollWidth<=innerWidth,'no horizontal overflow at 800x600');`);
  await screenshot("authoring-800-narrow.png");

  const win600 = await resizeWindow(url, app.pid, 600, 720, { verify: true });
  await run(`await wait(); assert(document.documentElement.scrollWidth<=innerWidth,'no horizontal overflow at 600x720');`);
  await screenshot("authoring-600-stacked.png");

  // 4. Return to Write at 1120x780: verify dirty draft/Undo preserved, link Kestrel, open Write Reveal!
  await resizeWindow(url, app.pid, 1120, 780, { verify: true });
  await run(`
    document.querySelector('#mode-write').click();
    await wait();await wait();
    const view=EditorView.findFromDOM(document.querySelector('.cm-editor'));
    assert(document.querySelector('#title-input').value==='Harbor Ledger (draft)','dirty title preserved');
    assert(view.state.doc.toString().includes('Unsaved lantern note.'),'dirty body preserved');
    assert(document.querySelector('#save-status').textContent==='Unsaved','still dirty after Codex authoring');

    // Link "Kestrel" and inspect in Write Reveal.
    view.dispatch({selection:{anchor:0,head:7}});
    await wait();
    document.querySelector('.link-entity-action').click();
    await wait();
    const kestrelOpt=[...document.querySelectorAll('#entity-results button')].find(b=>b.dataset.entityId===${JSON.stringify(setupIds.kestrelId)});
    assert(kestrelOpt,'Kestrel listed in Connect');
    kestrelOpt.click();
    await wait();
    document.querySelector('#save').click();
    await wait();await wait();
    assert(document.querySelector('#save-status').textContent==='Saved','Write save succeeds');

    // Open Write Reveal on linked Kestrel.
    view.dispatch({selection:{anchor:2,head:2}});
    await wait();
    document.querySelector('.link-entity-action').click();
    await wait();
    const showFacts=[...document.querySelectorAll('.entity-picker button')].find(b=>b.textContent==='Show facts');
    assert(showFacts,'Show facts button available');
    showFacts.click();
    await wait();await wait();
    const peekText=document.querySelector('.entity-picker.facts').textContent;
    assert(peekText.includes('Hold the lamp until dawn')||peekText.includes('Motto')||peekText.includes('Sworn'),'Write Reveal shows newly authored facts');
  `);
  await screenshot("authoring-1120-write-reveal-parity.png");

  // 5. Relaunch native desktop binary and verify exact persisted definition/assertion IDs!
  await stopAppAndWait(app);
  app = spawnDesktopApp({ port, userDataDir: webviewProfile, cwd: uiDir });
  url = await waitForTarget(port);
  await waitPageReady(url);
  await resizeWindow(url, app.pid, 1120, 780, { verify: true });

  const relaunchVerify = await evaluate(
    url,
    `(async()=>{${imports}
      const wait=()=>new Promise(r=>setTimeout(r,260));const assert=(v,m)=>{if(!v)throw Error(m)};
      document.querySelector('#database-path').value=${JSON.stringify(synthDb)};
      document.querySelector('#open-form').requestSubmit();
      await wait();await wait();
      document.querySelector('#mode-codex').click();
      await wait();await wait();
      document.querySelector('.codex-entity-item[data-entity-id=${JSON.stringify(setupIds.kestrelId)}]').click();
      await wait();await wait();
      const defIds=[...document.querySelectorAll('.codex-prop-group[data-definition-id]')].map(g=>g.dataset.definitionId);
      const assIds=[...document.querySelectorAll('.codex-assertion[data-assertion-id]')].map(a=>a.dataset.assertionId);
      for (const id of ${JSON.stringify(authoredIds.defIds)}) assert(defIds.includes(id),'relaunch preserved def '+id);
      for (const id of ${JSON.stringify(authoredIds.assIds)}) assert(assIds.includes(id),'relaunch preserved ass '+id);
      return {userAgent:navigator.userAgent,reopenedDefCount:defIds.length,reopenedAssCount:assIds.length};
    })()`,
  );

  const report = {
    synthetic: true,
    native: true,
    note: "CDP-dispatched DOM/keyboard events and programmatic resize; does not replace physical Windows keyboard/IME/DPI QA.",
    userAgent: relaunchVerify.userAgent,
    viewports: { win1600, win1120, win800, win600 },
    setupIds,
    authoredIds,
    relaunchVerify,
    screenshots: [
      "authoring-1600-new-property-dialog.png",
      "authoring-1600-kestrel-facts.png",
      "authoring-1120-add-value-dialog.png",
      "authoring-800-narrow.png",
      "authoring-600-stacked.png",
      "authoring-1120-write-reveal-parity.png",
    ],
  };

  writeFileSync(join(evidence, "result.json"), JSON.stringify(report, null, 2) + "\n");
  console.log(JSON.stringify(report, null, 2));
} finally {
  await stopAppAndWait(app);
  vite?.kill();
}
