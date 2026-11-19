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
  assert.equal(ui.doc.querySelector("#codex-detail-meta .codex-meta-item")!.textContent, "Untyped");
  const untypedIdDisclosure = ui.doc.querySelector<HTMLDetailsElement>("#codex-detail-meta .codex-id-disclosure")!;
  assert.equal(untypedIdDisclosure.open, false);
  assert.match(untypedIdDisclosure.querySelector(".codex-disclosure-body")!.textContent!, new RegExp(entities[0]!.id));
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
  assert.equal(ui.doc.querySelector("#codex-detail-meta .codex-meta-item")!.textContent, "House");
  const typedIdDisclosure = ui.doc.querySelector<HTMLDetailsElement>("#codex-detail-meta .codex-id-disclosure")!;
  assert.equal(typedIdDisclosure.open, false);
  assert.match(typedIdDisclosure.querySelector(".codex-disclosure-body")!.textContent!, new RegExp(types[0]!.id));
  ui.close();
});

test("search, duplicate disambiguation, editorial Codex entry hierarchy, and progressive disclosures work cleanly", async () => {
  const TYPE_A = "33333333-3333-4333-8333-333333333331";
  const TYPE_B = "33333333-3333-4333-8333-333333333332";
  const TYPE_EMPTY = "33333333-3333-4333-8333-333333333333";
  const ENT_A = "55555555-5555-4555-8555-555555555551";
  const ENT_B = "55555555-5555-4555-8555-555555555552";
  const ENT_C = "55555555-5555-4555-8555-555555555553";
  const DEF_ALLEGIANCE = "44444444-4444-4444-8444-000000000001";
  const DEF_OATH = "44444444-4444-4444-8444-000000000003";
  const DEF_MENTION = "44444444-4444-4444-8444-000000000004";
  const CLAIM_WARDENS = "66666666-6666-4666-8666-000000000001";
  const CLAIM_MYSTERY = "66666666-6666-4666-8666-000000000002";
  const CLAIM_NO = "66666666-6666-4666-8666-000000000003";
  const CLAIM_INCOMING = "66666666-6666-4666-8666-000000000004";
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
              id: DEF_ALLEGIANCE,
              ownerKind: "type",
              ownerId: TYPE_A,
              propertyLabel: "Allegiance",
              valueClass: "text",
              cardinality: "singular",
              archived: false,
            },
            claims: [
              {
                id: CLAIM_WARDENS,
                value: { kind: "text", text: "Wardens" },
                canon: "ACORDADO",
                sourceStatusRaw: "draft-note",
                provenance: { externalRef: "Ledger", locator: "p.4", fragment: null, sourceRevision: null },
                scopeNote: "Harbor chapters",
                archived: false,
              },
              {
                id: CLAIM_MYSTERY,
                value: { kind: "unknown" },
                canon: "MISTERIO_INTENCIONAL",
                sourceStatusRaw: null,
                provenance: { externalRef: null, locator: null, fragment: null, sourceRevision: null },
                scopeNote: null,
                archived: false,
              },
            ],
            hasArchivedClaims: false,
          },
          {
            definition: {
              id: DEF_OATH,
              ownerKind: "entity",
              ownerId: ENT_A,
              propertyLabel: "Oath sworn",
              valueClass: "boolean",
              cardinality: "singular",
              archived: false,
            },
            claims: [],
            hasArchivedClaims: false,
          },
          {
            definition: {
              id: DEF_MENTION,
              ownerKind: "entity",
              ownerId: ENT_A,
              propertyLabel: "Broke seal",
              valueClass: "boolean",
              cardinality: "singular",
              archived: false,
            },
            claims: [
              {
                id: CLAIM_NO,
                value: { kind: "boolean", value: false },
                canon: "PROVISIONAL",
                sourceStatusRaw: null,
                provenance: { externalRef: null, locator: null, fragment: null, sourceRevision: null },
                scopeNote: null,
                archived: false,
              },
            ],
            hasArchivedClaims: false,
          },
        ],
        incomingGroups: [
          {
            subject: { id: ENT_C, label: "Ash Lantern" },
            definition: {
              id: "44444444-4444-4444-8444-000000000005",
              ownerKind: "entity",
              ownerId: ENT_C,
              propertyLabel: "Bearer",
              valueClass: "entity",
              cardinality: "singular",
              archived: false,
            },
            claims: [
              {
                id: CLAIM_INCOMING,
                value: { kind: "reference", target: { id: ENT_A, label: "Mara Vey" } },
                canon: "ABIERTO",
                sourceStatusRaw: null,
                provenance: { externalRef: null, locator: null, fragment: null, sourceRevision: null },
                scopeNote: null,
                archived: false,
              },
            ],
          },
        ],
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

  // Quiet idle status on initial load
  assert.equal(ui.doc.querySelector("#codex-status")!.textContent, "");

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
  assert.match(ui.doc.querySelector("#codex-list-empty-text")!.textContent!, /No entities in this type/);

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

  // Inspect ENT_A: read-only metadata, value-first assertions, one human canon gloss, hidden full UUIDs until opened.
  ui.doc.querySelector<HTMLButtonElement>(`.codex-entity-item[data-entity-id="${ENT_A}"]`)!.click();
  await tick();
  assert.equal(ui.doc.querySelector("#codex-detail-title")!.textContent, "Mara Vey · …5551");
  const metaEl = ui.doc.querySelector("#codex-detail-meta")!;
  assert.match(metaEl.textContent!, /Primary document: Harbor Note/);

  const ctx = ui.doc.querySelector("#codex-detail-context")!;
  // Banner removed; section names simplified; outgoing assertions don't repeat entity/predicate on every row
  assert.doesNotMatch(ctx.textContent!, /Persisted knowledge · point-in-time read/);
  assert.match(ctx.textContent!, /Facts/);
  assert.match(ctx.textContent!, /Referenced by/);
  const allegianceGroup = ctx.querySelector<HTMLElement>(`.codex-prop-group[data-definition-id="${DEF_ALLEGIANCE}"]`)!;
  const valueText = allegianceGroup.querySelector(".codex-assertion-value")!.textContent!;
  assert.equal(valueText, "Wardens");
  assert.equal(allegianceGroup.querySelector(".codex-canon-tag")!.textContent, "Agreed");

  // Full UUIDs, stored canon codes, and technical schema metadata stay tucked inside closed disclosures by default
  const idDisclosure = metaEl.querySelector<HTMLDetailsElement>(".codex-id-disclosure")!;
  const propDisclosure = allegianceGroup.querySelector<HTMLDetailsElement>(".codex-prop-disclosure")!;
  const claimDisclosure = allegianceGroup.querySelector<HTMLDetailsElement>(".codex-claim-disclosure")!;
  assert.equal(idDisclosure.open, false);
  assert.equal(propDisclosure.open, false);
  assert.equal(claimDisclosure.open, false);
  assert.match(propDisclosure.querySelector(".codex-disclosure-body")!.textContent!, new RegExp(`Type-owned · text · singular · Definition ID: ${DEF_ALLEGIANCE}`));
  assert.match(claimDisclosure.querySelector(".codex-disclosure-body")!.textContent!, /Source: Ledger · Locator: p\.4 · Source status: draft-note · Scope: Harbor chapters · Stored canon: ACORDADO/);
  assert.match(claimDisclosure.querySelector(".codex-disclosure-body")!.textContent!, new RegExp(CLAIM_WARDENS));

  // Distinguish Not recorded vs Unknown (recorded) vs No
  const oathGroup = ctx.querySelector<HTMLElement>(`.codex-prop-group[data-definition-id="${DEF_OATH}"]`)!;
  assert.equal(oathGroup.querySelector(".codex-unrecorded")!.textContent, "Not recorded");
  assert.ok([...allegianceGroup.querySelectorAll(".codex-assertion-value")].some(n => n.textContent === "Unknown (recorded)"));
  const boolGroup = ctx.querySelector<HTMLElement>(`.codex-prop-group[data-definition-id="${DEF_MENTION}"]`)!;
  assert.equal(boolGroup.querySelector(".codex-assertion-value")!.textContent, "No");
  assert.equal(boolGroup.querySelector(".codex-canon-tag")!.textContent, "Provisional");

  // Incoming reference preserves subject -> target orientation
  const incomingGroup = ctx.querySelector<HTMLElement>(".codex-incoming-group")!;
  assert.equal(incomingGroup.querySelector(".codex-prop-name")!.textContent, "Bearer");
  assert.equal(incomingGroup.querySelector(".codex-assertion-value")!.textContent, "Ash Lantern → Mara Vey · …5551");
  assert.equal(incomingGroup.querySelector(".codex-canon-tag")!.textContent, "Open");

  assert.equal(ctx.querySelector<HTMLDetailsElement>(".context-archived")!.open, false);
  assert.match(ctx.querySelector(".context-archived > summary")!.textContent!, /Archived \(1\)/);

  // Inspect ENT_B: bounded context error shown honestly without partial claims.
  ui.doc.querySelector<HTMLButtonElement>("#codex-clear-filters")!.click();
  ui.doc.querySelector<HTMLButtonElement>(`.codex-entity-item[data-entity-id="${ENT_B}"]`)!.click();
  await tick();
  assert.match(ui.doc.querySelector("#codex-detail-context")!.textContent!, /Context too large/);
  assert.deepEqual(inspected, [ENT_A, ENT_B]);
  ui.close();
});

test("validation rejects invalid labels and ambiguous create failures lock creation until verified against codex_index", async () => {
  let typeCreates = 0;
  let entityCreates = 0;
  let failNextIndex = false;
  const serverTypes: { id: string; label: string }[] = [];
  const serverEntities: {
    id: string;
    label: string;
    typeId: string | null;
    typeLabel: string | null;
    primaryPageTitle: string | null;
  }[] = [];
  let entityOutcome: "ambiguous_committed" | "ambiguous_uncommitted" = "ambiguous_committed";
  let typeOutcome: "definite_busy" | "ambiguous_committed" = "definite_busy";

  const ui = setup(async (cmd, args) => {
    if (cmd === "page_index") return [];
    if (cmd === "codex_index") {
      if (failNextIndex) {
        failNextIndex = false;
        throw { kind: "busy" };
      }
      return { types: [...serverTypes], entities: [...serverEntities] };
    }
    if (cmd === "create_entity_type") {
      typeCreates++;
      if (typeOutcome === "definite_busy") throw { kind: "busy" };
      // Ambiguous failure AFTER server actually committed the row (response lost!)
      serverTypes.push({ id: String(args!.typeId), label: String(args!.label) });
      throw {};
    }
    if (cmd === "create_entity") {
      entityCreates++;
      if (entityOutcome === "ambiguous_committed") {
        serverEntities.push({
          id: String(args!.entityId),
          label: String(args!.label),
          typeId: null,
          typeLabel: null,
          primaryPageTitle: null,
        });
      }
      throw {};
    }
    if (cmd === "entity_context") {
      const found = serverEntities.find(e => e.id === args!.entityId)!;
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

  ui.open("val.sqlite");
  await tick();
  ui.doc.querySelector<HTMLButtonElement>("#mode-codex")!.click();
  await tick();

  const newTypeBtn = ui.doc.querySelector<HTMLButtonElement>("#codex-new-type")!;
  const newEntityBtn = ui.doc.querySelector<HTMLButtonElement>("#codex-new-entity")!;
  const refreshBtn = ui.doc.querySelector<HTMLButtonElement>("#codex-refresh")!;
  const statusEl = ui.doc.querySelector("#codex-status")!;

  // Client-side validation rejects blank and >120 scalars before IPC.
  newTypeBtn.click();
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

  // Definite non-commit keeps dialog open and entered value intact without locking creations.
  typeInput.value = "Faction";
  ui.doc.querySelector<HTMLFormElement>("#codex-type-form")!.dispatchEvent(new ui.win.Event("submit", { cancelable: true }));
  await tick();
  assert.equal(typeCreates, 1);
  assert.equal(typeDialog.open, true);
  assert.equal(typeInput.value, "Faction");
  assert.match(ui.doc.querySelector("#codex-type-error")!.textContent!, /busy/i);

  // Ambiguous type outcome after commit locks BOTH create buttons until verified!
  typeOutcome = "ambiguous_committed";
  failNextIndex = true;
  ui.doc.querySelector<HTMLFormElement>("#codex-type-form")!.dispatchEvent(new ui.win.Event("submit", { cancelable: true }));
  await tick();
  assert.equal(typeCreates, 2);
  assert.equal(typeDialog.open, false);
  assert.equal(newTypeBtn.disabled, true);
  assert.equal(newEntityBtn.disabled, true);
  assert.equal(refreshBtn.hidden, false);
  assert.match(statusEl.textContent!, /Refresh Codex/);

  // Failed verification keeps pending creation locked and reports "still unconfirmed" without auto-retrying
  refreshBtn.click();
  await tick();
  assert.equal(typeCreates, 2);
  assert.equal(newTypeBtn.disabled, true);
  assert.equal(newEntityBtn.disabled, true);
  assert.match(statusEl.textContent!, /still unconfirmed/);

  // Successful verification confirms the committed-but-response-lost type and unlocks creation
  refreshBtn.click();
  await tick();
  assert.equal(typeCreates, 2);
  assert.equal(newTypeBtn.disabled, false);
  assert.equal(newEntityBtn.disabled, false);
  assert.equal(refreshBtn.hidden, true);
  assert.match(statusEl.textContent!, /Confirmed type "Faction" was saved\./);
  assert.ok([...ui.doc.querySelectorAll<HTMLOptionElement>("#codex-type-filter option")].some(o => o.textContent === "Faction (0)"));

  // Ambiguous entity NOT saved: locks until verified, then unlocks and reports "was not saved"
  entityOutcome = "ambiguous_uncommitted";
  newEntityBtn.click();
  const entityDialog = ui.doc.querySelector<HTMLDialogElement>("#codex-entity-dialog")!;
  ui.doc.querySelector<HTMLInputElement>("#codex-entity-label-input")!.value = "Uncertain Bell";
  ui.doc.querySelector<HTMLFormElement>("#codex-entity-form")!.dispatchEvent(new ui.win.Event("submit", { cancelable: true }));
  await tick();
  assert.equal(entityCreates, 1);
  assert.equal(entityDialog.open, false);
  assert.equal(newEntityBtn.disabled, true);
  assert.equal(newTypeBtn.disabled, true);
  assert.equal(refreshBtn.hidden, false);

  refreshBtn.click();
  await tick();
  assert.equal(entityCreates, 1);
  assert.equal(newEntityBtn.disabled, false);
  assert.equal(newTypeBtn.disabled, false);
  assert.equal(refreshBtn.hidden, true);
  assert.match(statusEl.textContent!, /was not saved/);

  // Committed-but-response-lost entity: locks until verified, then confirms, selects, and inspects it!
  entityOutcome = "ambiguous_committed";
  newEntityBtn.click();
  ui.doc.querySelector<HTMLInputElement>("#codex-entity-label-input")!.value = "Lantern Echo";
  ui.doc.querySelector<HTMLFormElement>("#codex-entity-form")!.dispatchEvent(new ui.win.Event("submit", { cancelable: true }));
  await tick();
  assert.equal(entityCreates, 2);
  assert.equal(newEntityBtn.disabled, true);

  refreshBtn.click();
  await tick();
  assert.equal(entityCreates, 2);
  assert.equal(newEntityBtn.disabled, false);
  assert.equal(refreshBtn.hidden, true);
  assert.match(statusEl.textContent!, /Confirmed entity "Lantern Echo" was saved\./);
  assert.equal(ui.doc.querySelector("#codex-detail-title")!.textContent, "Lantern Echo");

  // Switching universes clears unresolved pendingCreate and never leaks across tokens
  newEntityBtn.click();
  ui.doc.querySelector<HTMLInputElement>("#codex-entity-label-input")!.value = "Other Echo";
  ui.doc.querySelector<HTMLFormElement>("#codex-entity-form")!.dispatchEvent(new ui.win.Event("submit", { cancelable: true }));
  await tick();
  assert.equal(newEntityBtn.disabled, true);

  ui.open("second.sqlite");
  await tick();
  assert.equal(newEntityBtn.disabled, false);
  assert.equal(newTypeBtn.disabled, false);
  assert.equal(refreshBtn.hidden, true);
  ui.close();
});

test("opening a universe while in Codex preserves Codex workspace header and focus", async () => {
  let currentDb = "first.sqlite";
  const ui = setup(async (cmd, args) => {
    if (cmd === "open_universe") {
      currentDb = String(args!.databasePath);
      return undefined;
    }
    if (cmd === "page_index") {
      return currentDb === "first.sqlite"
        ? [{ id: "p1", title: "Chapter One" }]
        : [{ id: "p2", title: "Second Chronicle" }];
    }
    if (cmd === "read_page") {
      return currentDb === "first.sqlite"
        ? { id: "p1", title: "Chapter One", body: "Alpha.", revision: 0, mentions: [] }
        : { id: "p2", title: "Second Chronicle", body: "Beta.", revision: 0, mentions: [] };
    }
    if (cmd === "codex_index") {
      return {
        types: [],
        entities: [{ id: "e-next", label: currentDb, typeId: null, typeLabel: null, primaryPageTitle: null }],
      };
    }
    return undefined;
  });

  // Narrow viewport (<900px) so selectPage would call editor.focus() / toggleLibrary.focus() if unguarded
  Object.defineProperty(ui.win, "innerWidth", { value: 780, configurable: true });

  ui.open("first.sqlite");
  await tick();
  ui.doc.querySelector<HTMLButtonElement>("#mode-codex")!.click();
  await tick();
  assert.equal(ui.doc.querySelector("#context-name")!.textContent, "Codex");

  const searchInput = ui.doc.querySelector<HTMLInputElement>("#codex-search")!;
  searchInput.focus();

  ui.open("second.sqlite");
  await tick();
  assert.equal(ui.doc.querySelector("#context-name")!.textContent, "Codex");
  assert.equal(ui.doc.querySelector("#mode-codex")!.getAttribute("aria-pressed"), "true");
  assert.equal(ui.doc.activeElement, searchInput);

  // Switching back to Write restores the opened universe's selected document title cleanly.
  ui.doc.querySelector<HTMLButtonElement>("#mode-write")!.click();
  await tick();
  assert.equal(ui.doc.querySelector("#context-name")!.textContent, "Second Chronicle");
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
