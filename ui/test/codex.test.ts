import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";
import { EditorView } from "@codemirror/view";
import { initDesktop } from "../desktop/main.ts";

const tick = () => new Promise(resolve => setTimeout(resolve, 10));
function deferred<T = unknown>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((a, b) => {
    resolve = a;
    reject = b;
  });
  return { promise, resolve, reject };
}

function setup(
  invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown> | unknown,
) {
  const dom = new JSDOM(
    readFileSync(new URL("../desktop/index.html", import.meta.url), "utf8"),
    { pretendToBeVisual: true },
  );
  Object.assign(globalThis, {
    window: dom.window,
    document: dom.window.document,
    Window: dom.window.Window,
    MutationObserver: dom.window.MutationObserver,
    requestAnimationFrame: dom.window.requestAnimationFrame.bind(dom.window),
    cancelAnimationFrame: dom.window.cancelAnimationFrame.bind(dom.window),
  });
  dom.window.HTMLElement.prototype.scrollIntoView = () => {};
  dom.window.HTMLDialogElement.prototype.showModal = function () {
    this.open = true;
  };
  dom.window.HTMLDialogElement.prototype.close = function () {
    this.open = false;
  };
  const doc = dom.window.document;
  const controller = initDesktop((async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === "open_universe") {
      await invoke(cmd, args);
      return { token: (args?.databasePath as string) ?? "tok-1", pages: await invoke("page_index") };
    }
    return invoke(cmd, args);
  }) as unknown as typeof import("@tauri-apps/api/core").invoke);

  return {
    doc,
    win: dom.window,
    controller,
    open(path: string) {
      doc.querySelector<HTMLInputElement>("#database-path")!.value = path;
      doc
        .querySelector<HTMLFormElement>("#open-form")!
        .dispatchEvent(new dom.window.Event("submit", { cancelable: true }));
    },
    view() {
      const node = doc.querySelector(".cm-editor");
      return node ? EditorView.findFromDOM(node) : null;
    },
    close() {
      dom.window.close();
    },
  };
}

test("empty-document universe opens Codex without creating a page and supports custom types and entities", async () => {
  const types: { id: string; label: string }[] = [];
  const entities: {
    id: string;
    label: string;
    typeId: string | null;
    typeLabel: string | null;
    primaryPageTitle: string | null;
  }[] = [];
  const calls: string[] = [];

  const ui = setup(async (cmd, args) => {
    calls.push(cmd);
    if (cmd === "page_index") return [];
    if (cmd === "codex_index") return { types: [...types], entities: [...entities] };
    if (cmd === "create_entity_type") {
      const entry = { id: String(args!.typeId), label: String(args!.label) };
      types.push(entry);
      return entry;
    }
    if (cmd === "create_entity") {
      const typeId = (args!.typeId as string | null) ?? null;
      const typeLabel = typeId ? (types.find(t => t.id === typeId)?.label ?? null) : null;
      const entry = {
        id: String(args!.entityId),
        label: String(args!.label),
        typeId,
        typeLabel,
        primaryPageTitle: null,
      };
      entities.push(entry);
      return entry;
    }
    if (cmd === "entity_context") {
      const id = String(args!.entityId);
      const found = entities.find(e => e.id === id)!;
      return {
        entity: { id: found.id, label: found.label },
        typeLabel: found.typeLabel,
        outgoingGroups: [],
        incomingGroups: [],
        archivedOutgoingGroups: [],
        archivedIncomingGroups: [],
      };
    }
    return undefined;
  });

  // Before opening universe, Codex mode switch is disabled.
  assert.equal(ui.doc.querySelector<HTMLButtonElement>("#mode-codex")!.disabled, true);

  ui.open("empty.sqlite");
  await tick();
  assert.equal(ui.doc.querySelector<HTMLButtonElement>("#mode-codex")!.disabled, false);
  assert.equal(calls.includes("codex_index"), false);

  ui.doc.querySelector<HTMLButtonElement>("#mode-codex")!.click();
  await tick();
  assert.equal(ui.doc.querySelector("#context-name")!.textContent, "Codex");
  assert.equal(ui.doc.querySelector<HTMLElement>("#codex-workspace")!.hidden, false);
  assert.equal(ui.doc.querySelector<HTMLElement>("#codex-list-empty")!.hidden, false);
  assert.match(ui.doc.querySelector("#codex-list-empty-text")!.textContent!, /No entities/);

  // Create custom type "House"
  ui.doc.querySelector<HTMLButtonElement>("#codex-new-type")!.click();
  ui.doc.querySelector<HTMLInputElement>("#codex-type-label-input")!.value = "  House  ";
  ui.doc
    .querySelector<HTMLFormElement>("#codex-type-form")!
    .dispatchEvent(new ui.win.Event("submit", { cancelable: true }));
  await tick();
  assert.equal(types.length, 1);
  assert.equal(types[0]!.label, "House");

  // Type filter shows 0-entity type alongside Untyped (0)
  const filterOptions = [...ui.doc.querySelectorAll<HTMLOptionElement>("#codex-type-filter option")].map(
    o => o.textContent,
  );
  assert.deepEqual(filterOptions, ["All types (0)", "House (0)", "Untyped (0)"]);

  // Open New Entity dialog: default selection is always Untyped ("")
  ui.doc.querySelector<HTMLButtonElement>("#codex-new-entity")!.click();
  assert.equal(ui.doc.querySelector<HTMLSelectElement>("#codex-entity-type-select")!.value, "");
  ui.doc.querySelector<HTMLInputElement>("#codex-entity-label-input")!.value = "Ashfall Bell";
  ui.doc
    .querySelector<HTMLFormElement>("#codex-entity-form")!
    .dispatchEvent(new ui.win.Event("submit", { cancelable: true }));
  await tick();

  // Created entity is untyped, selected, and inspected immediately; no pages were created.
  assert.equal(entities[0]!.typeId, null);
  assert.equal(ui.doc.querySelector("#codex-detail-title")!.textContent, "Ashfall Bell");
  assert.match(ui.doc.querySelector("#codex-detail-meta")!.textContent!, /Type: Untyped/);
  assert.equal(calls.includes("create_page"), false);

  // Create typed entity
  ui.doc.querySelector<HTMLButtonElement>("#codex-new-entity")!.click();
  ui.doc.querySelector<HTMLInputElement>("#codex-entity-label-input")!.value = "Mara Vey";
  ui.doc.querySelector<HTMLSelectElement>("#codex-entity-type-select")!.value = types[0]!.id;
  ui.doc
    .querySelector<HTMLFormElement>("#codex-entity-form")!
    .dispatchEvent(new ui.win.Event("submit", { cancelable: true }));
  await tick();

  assert.equal(entities[1]!.typeId, types[0]!.id);
  assert.equal(ui.doc.querySelector("#codex-detail-title")!.textContent, "Mara Vey");
  assert.match(ui.doc.querySelector("#codex-detail-meta")!.textContent!, /Type: House/);
  ui.close();
});

test("search, duplicate type/entity identity disambiguation, and EntityContext inspection work cleanly", async () => {
  const TYPE_A = "33333333-3333-4333-8333-333333333331";
  const TYPE_B = "33333333-3333-4333-8333-333333333332";
  const TYPE_EMPTY = "33333333-3333-4333-8333-333333333333";
  const ENT_A = "55555555-5555-4555-8555-555555555551";
  const ENT_B = "55555555-5555-4555-8555-555555555552";
  const ENT_C = "55555555-5555-4555-8555-555555555553";
  const inspected: string[] = [];

  const ui = setup(async (cmd, args) => {
    if (cmd === "page_index") return [];
    if (cmd === "codex_index") {
      return {
        types: [
          { id: TYPE_A, label: "Order" },
          { id: TYPE_B, label: "Order" },
          { id: TYPE_EMPTY, label: "Relic" },
        ],
        entities: [
          { id: ENT_A, label: "Mara Vey", typeId: TYPE_A, typeLabel: "Order", primaryPageTitle: "Harbor Note" },
          { id: ENT_B, label: "Mara Vey", typeId: TYPE_B, typeLabel: "Order", primaryPageTitle: null },
          { id: ENT_C, label: "Ash Lantern", typeId: null, typeLabel: null, primaryPageTitle: null },
        ],
      };
    }
    if (cmd === "entity_context") {
      const entityId = String(args!.entityId);
      inspected.push(entityId);
      if (entityId === ENT_B) throw { kind: "context_too_large" };
      return {
        entity: { id: ENT_A, label: "Mara Vey" },
        typeLabel: "Order",
        outgoingGroups: [
          {
            definition: {
              id: "44444444-4444-4444-8444-000000000001",
              ownerKind: "type",
              ownerId: TYPE_A,
              propertyLabel: "Allegiance",
              valueClass: "text",
              cardinality: "singular",
              archived: false,
            },
            claims: [
              {
                id: "66666666-6666-4666-8666-000000000001",
                value: { kind: "text", text: "Wardens" },
                canon: "ACORDADO",
                sourceStatusRaw: null,
                provenance: { externalRef: "Ledger", locator: "p.4", fragment: null, sourceRevision: null },
                scopeNote: null,
                archived: false,
              },
            ],
            hasArchivedClaims: false,
          },
        ],
        incomingGroups: [],
        archivedOutgoingGroups: [
          {
            definition: {
              id: "44444444-4444-4444-8444-000000000002",
              ownerKind: "entity",
              ownerId: ENT_A,
              propertyLabel: "Former Title",
              valueClass: "text",
              cardinality: "singular",
              archived: true,
            },
            claims: [],
            hasArchivedClaims: false,
          },
        ],
        archivedIncomingGroups: [],
      };
    }
    return undefined;
  });

  ui.open("synth.sqlite");
  await tick();
  ui.doc.querySelector<HTMLButtonElement>("#mode-codex")!.click();
  await tick();

  // Duplicate entity and type labels both disambiguated deterministically.
  const rows = [...ui.doc.querySelectorAll<HTMLButtonElement>(".codex-entity-item")];
  assert.equal(rows.length, 3);
  assert.equal(rows[0]!.querySelector(".codex-entity-label")!.textContent, "Ash Lantern");
  assert.equal(rows[1]!.querySelector(".codex-entity-label")!.textContent, "Mara Vey · …5551");
  assert.equal(rows[2]!.querySelector(".codex-entity-label")!.textContent, "Mara Vey · …5552");

  // Type filter distinguishes duplicate Order types and selects each independently.
  const typeSelect = ui.doc.querySelector<HTMLSelectElement>("#codex-type-filter")!;
  assert.ok([...typeSelect.options].some(o => o.value === TYPE_A && o.textContent === "Order · …3331 (1)"));
  assert.ok([...typeSelect.options].some(o => o.value === TYPE_B && o.textContent === "Order · …3332 (1)"));
  assert.ok([...typeSelect.options].some(o => o.value === TYPE_EMPTY && o.textContent === "Relic (0)"));

  typeSelect.value = TYPE_B;
  typeSelect.dispatchEvent(new ui.win.Event("change"));
  assert.deepEqual(
    [...ui.doc.querySelectorAll<HTMLButtonElement>(".codex-entity-item")].map(b => b.dataset.entityId),
    [ENT_B],
  );

  // Empty custom type shows explicit empty state without failing.
  typeSelect.value = TYPE_EMPTY;
  typeSelect.dispatchEvent(new ui.win.Event("change"));
  assert.equal(ui.doc.querySelector<HTMLElement>("#codex-list-empty")!.hidden, false);
  assert.match(ui.doc.querySelector("#codex-list-empty-text")!.textContent!, /No entities belong to this type/);

  // Clear filters and search with Enter key (must not create anything).
  ui.doc.querySelector<HTMLButtonElement>("#codex-clear-filters")!.click();
  const search = ui.doc.querySelector<HTMLInputElement>("#codex-search")!;
  search.value = "Harbor Note";
  search.dispatchEvent(new ui.win.Event("input"));
  search.dispatchEvent(new ui.win.KeyboardEvent("keydown", { key: "Enter", cancelable: true }));
  assert.deepEqual(
    [...ui.doc.querySelectorAll<HTMLButtonElement>(".codex-entity-item")].map(b => b.dataset.entityId),
    [ENT_A],
  );

  // Inspect ENT_A: read-only Primary document metadata, grouped claims, and collapsed archived section.
  ui.doc.querySelector<HTMLButtonElement>(`.codex-entity-item[data-entity-id="${ENT_A}"]`)!.click();
  await tick();
  assert.equal(ui.doc.querySelector("#codex-detail-title")!.textContent, "Mara Vey · …5551");
  assert.match(ui.doc.querySelector("#codex-detail-meta")!.textContent!, /Primary document: Harbor Note/);
  assert.match(ui.doc.querySelector("#codex-detail-context")!.textContent!, /Allegiance/);
  assert.match(ui.doc.querySelector("#codex-detail-context")!.textContent!, /Wardens/);
  assert.equal(ui.doc.querySelector<HTMLDetailsElement>(".context-archived")!.open, false);

  // Inspect ENT_B: bounded context error shown honestly without partial claims.
  ui.doc.querySelector<HTMLButtonElement>("#codex-clear-filters")!.click();
  ui.doc.querySelector<HTMLButtonElement>(`.codex-entity-item[data-entity-id="${ENT_B}"]`)!.click();
  await tick();
  assert.match(ui.doc.querySelector("#codex-detail-context")!.textContent!, /Context too large/);
  assert.deepEqual(inspected, [ENT_A, ENT_B]);
  ui.close();
});

test("validation rejects blank/oversized labels without IPC and preserves form input on backend error", async () => {
  let typeCreates = 0;
  let entityCreates = 0;
  let indexReads = 0;

  const ui = setup(async (cmd, args) => {
    if (cmd === "page_index") return [];
    if (cmd === "codex_index") {
      indexReads++;
      if (indexReads === 2) throw { kind: "busy" };
      return { types: [], entities: [] };
    }
    if (cmd === "create_entity_type") {
      typeCreates++;
      if (typeCreates === 1) throw { kind: "busy" };
      return { id: String(args!.typeId), label: String(args!.label) };
    }
    if (cmd === "create_entity") {
      entityCreates++;
      // Ambiguous transport drop after persistence
      throw {};
    }
    return undefined;
  });

  ui.open("val.sqlite");
  await tick();
  ui.doc.querySelector<HTMLButtonElement>("#mode-codex")!.click();
  await tick();

  // Client-side validation rejects blank and >120 scalars before IPC.
  ui.doc.querySelector<HTMLButtonElement>("#codex-new-type")!.click();
  const typeDialog = ui.doc.querySelector<HTMLDialogElement>("#codex-type-dialog")!;
  const typeInput = ui.doc.querySelector<HTMLInputElement>("#codex-type-label-input")!;
  typeInput.value = "   \u00a0  ";
  ui.doc.querySelector<HTMLFormElement>("#codex-type-form")!.dispatchEvent(new ui.win.Event("submit", { cancelable: true }));
  await tick();
  assert.equal(typeCreates, 0);
  assert.equal(typeDialog.open, true);
  assert.match(ui.doc.querySelector("#codex-type-error")!.textContent!, /nonblank/i);

  typeInput.value = "x".repeat(121);
  ui.doc.querySelector<HTMLFormElement>("#codex-type-form")!.dispatchEvent(new ui.win.Event("submit", { cancelable: true }));
  await tick();
  assert.equal(typeCreates, 0);
  assert.match(ui.doc.querySelector("#codex-type-error")!.textContent!, /120/);

  // Backend busy keeps dialog open and entered value intact; second submit succeeds, then refresh failure shows non-destructive recovery without retrying create.
  typeInput.value = "Faction";
  ui.doc.querySelector<HTMLFormElement>("#codex-type-form")!.dispatchEvent(new ui.win.Event("submit", { cancelable: true }));
  await tick();
  assert.equal(typeCreates, 1);
  assert.equal(typeDialog.open, true);
  assert.equal(typeInput.value, "Faction");
  assert.match(ui.doc.querySelector("#codex-type-error")!.textContent!, /busy/i);

  ui.doc.querySelector<HTMLFormElement>("#codex-type-form")!.dispatchEvent(new ui.win.Event("submit", { cancelable: true }));
  await tick();
  assert.equal(typeCreates, 2);
  assert.equal(typeDialog.open, false);
  assert.equal(ui.doc.querySelector<HTMLButtonElement>("#codex-refresh")!.hidden, false);
  assert.match(ui.doc.querySelector("#codex-status")!.textContent!, /Refresh Codex/);

  // Ambiguous entity creation outcome exposes Refresh Codex without retrying create_entity.
  ui.doc.querySelector<HTMLButtonElement>("#codex-new-entity")!.click();
  const entityDialog = ui.doc.querySelector<HTMLDialogElement>("#codex-entity-dialog")!;
  ui.doc.querySelector<HTMLInputElement>("#codex-entity-label-input")!.value = "Uncertain Bell";
  ui.doc.querySelector<HTMLFormElement>("#codex-entity-form")!.dispatchEvent(new ui.win.Event("submit", { cancelable: true }));
  await tick();
  assert.equal(entityCreates, 1);
  assert.equal(entityDialog.open, false);
  assert.equal(ui.doc.querySelector<HTMLButtonElement>("#codex-refresh")!.hidden, false);
  assert.match(ui.doc.querySelector("#codex-status")!.textContent!, /Refresh Codex/);

  ui.doc.querySelector<HTMLButtonElement>("#codex-refresh")!.click();
  await tick();
  assert.equal(entityCreates, 1);
  ui.close();
});

test("Write <-> Codex preserves dirty draft, title, undo, scroll, close guard, and exposes newly created entities to Connect", async () => {
  const createdEntities: { id: string; label: string; typeId: string | null; typeLabel: string | null; primaryPageTitle: string | null }[] = [];
  let saves = 0;
  let closes = 0;

  const ui = setup(async (cmd, args) => {
    if (cmd === "page_index") return [{ id: "p1", title: "Chapter One" }];
    if (cmd === "read_page") return { id: "p1", title: "Chapter One", body: "Mara entered Harbor.", revision: 0, mentions: [] };
    if (cmd === "codex_index") return { types: [], entities: [...createdEntities] };
    if (cmd === "create_entity") {
      const entry = { id: String(args!.entityId), label: String(args!.label), typeId: null, typeLabel: null, primaryPageTitle: null };
      createdEntities.push(entry);
      return entry;
    }
    if (cmd === "entity_context") {
      const found = createdEntities.find(e => e.id === args!.entityId)!;
      return {
        entity: { id: found.id, label: found.label },
        typeLabel: null,
        outgoingGroups: [],
        incomingGroups: [],
        archivedOutgoingGroups: [],
        archivedIncomingGroups: [],
      };
    }
    if (cmd === "entity_index") {
      return createdEntities.map(e => ({
        id: e.id,
        label: e.label,
        typeLabel: e.typeLabel,
        primaryPageTitle: e.primaryPageTitle,
      }));
    }
    if (cmd === "save_page") {
      saves++;
      return { pageId: "p1", sessionId: args!.request, requestId: 1, title: "Draft Title", body: "", mentions: [], revision: 1 };
    }
    if (cmd === "close_window") {
      closes++;
    }
    return undefined;
  });

  ui.open("draft.sqlite");
  await tick();
  const view = ui.view()!;
  const pane = ui.doc.querySelector<HTMLElement>(".reading-area")!;
  const titleInput = ui.doc.querySelector<HTMLInputElement>("#title-input")!;

  // Dirty both title and body + scroll pane
  titleInput.value = "Draft Title";
  titleInput.dispatchEvent(new ui.win.Event("input"));
  view.dispatch({ changes: { from: view.state.doc.length, insert: " Unsaved dusk." } });
  pane.scrollTop = 140;

  // Switch to Codex -> Ctrl+S must NOT save hidden Write draft
  ui.doc.querySelector<HTMLButtonElement>("#mode-codex")!.click();
  await tick();
  ui.doc.dispatchEvent(new ui.win.KeyboardEvent("keydown", { key: "s", ctrlKey: true, bubbles: true, cancelable: true }));
  await tick();
  assert.equal(saves, 0);

  // Create an entity in Codex while Write draft sits dirty in memory
  ui.doc.querySelector<HTMLButtonElement>("#codex-new-entity")!.click();
  ui.doc.querySelector<HTMLInputElement>("#codex-entity-label-input")!.value = "Red Harbor";
  ui.doc.querySelector<HTMLFormElement>("#codex-entity-form")!.dispatchEvent(new ui.win.Event("submit", { cancelable: true }));
  await tick();
  assert.equal(saves, 0);

  // Window close guard STILL protects the hidden dirty Write draft while in Codex!
  ui.controller.requestClose("close-tok");
  await tick();
  assert.equal(ui.doc.querySelector<HTMLDialogElement>("#guard")!.open, true);
  assert.equal(closes, 0);
  ui.doc.querySelector<HTMLButtonElement>("#continue")!.click();
  await tick();

  // Return to Write: draft, title, scroll, and undo history are all intact
  ui.doc.querySelector<HTMLButtonElement>("#mode-write")!.click();
  await tick();
  assert.equal(ui.doc.querySelector("#context-name")!.textContent, "Draft Title");
  assert.equal(titleInput.value, "Draft Title");
  assert.equal(view.state.doc.toString(), "Mara entered Harbor. Unsaved dusk.");
  assert.equal(pane.scrollTop, 140);
  assert.match(ui.doc.querySelector("#save-status")!.textContent!, /Unsaved/);

  // Connect in Write sees "Red Harbor" immediately without reopen
  view.dispatch({ selection: { anchor: 13, head: 19 } }); // "Harbor"
  await tick();
  ui.doc.querySelector<HTMLButtonElement>(".link-entity-action")!.click();
  await tick();
  const pickerOptions = [...ui.doc.querySelectorAll<HTMLButtonElement>("#entity-results button")].map(b => b.textContent);
  assert.ok(pickerOptions.some(text => text?.includes("Red Harbor")));
  ui.close();
});
