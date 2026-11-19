import { renderCodexEntrySections, suffixed, type EntityContext } from "./connect.ts";

export type TypeIndexEntry = { id: string; label: string };
export type CodexEntityEntry = {
  id: string;
  label: string;
  typeId: string | null;
  typeLabel: string | null;
  primaryPageTitle: string | null;
};
export type CodexIndex = {
  types: TypeIndexEntry[];
  entities: CodexEntityEntry[];
};

type PendingCodexCreate = {
  token: string;
  kind: "type" | "entity";
  id: string;
  label: string;
};

const MAX_CODEX_LABEL_SCALARS = 120;
const CONTROL_CHAR_RE = /[\u0000-\u001F\u007F-\u009F]/;
const utf8Encoder = new TextEncoder();

export function compareUtf8Binary(a: string, b: string): number {
  const left = utf8Encoder.encode(a);
  const right = utf8Encoder.encode(b);
  const limit = Math.min(left.length, right.length);
  for (let i = 0; i < limit; i++) {
    if (left[i] !== right[i]) return left[i]! - right[i]!;
  }
  return left.length - right.length;
}

export function validateCodexInput(raw: string): { ok: true; label: string } | { ok: false; message: string } {
  const trimmed = raw.trim();
  if (!trimmed.length) {
    return { ok: false, message: "Enter a nonblank label." };
  }
  if (CONTROL_CHAR_RE.test(trimmed)) {
    return { ok: false, message: "Labels must stay on a single line without control characters." };
  }
  if (Array.from(trimmed).length > MAX_CODEX_LABEL_SCALARS) {
    return { ok: false, message: `Labels cannot exceed ${MAX_CODEX_LABEL_SCALARS} characters.` };
  }
  return { ok: true, label: trimmed };
}

function errorKind(error: unknown): string | undefined {
  if (typeof error === "object" && error !== null && "kind" in error) {
    const raw = (error as { kind?: unknown }).kind;
    return typeof raw === "string" ? raw : undefined;
  }
  return undefined;
}

function isDefiniteNonCommit(error: unknown): boolean {
  switch (errorKind(error)) {
    case "busy":
    case "invalid_input":
    case "duplicate_id":
    case "type_missing":
    case "universe_changed":
    case "universe_not_open":
      return true;
    default:
      return false;
  }
}

function describeCreateError(error: unknown, target: "type" | "entity", label?: string): string {
  switch (errorKind(error)) {
    case "busy":
      return "Database is busy. Nothing was saved; try again.";
    case "invalid_input":
      return "Enter a valid nonblank label (1–120 characters, single line, no control characters).";
    case "duplicate_id":
      return target === "type"
        ? "That generated type ID already exists. Try again."
        : "That generated entity ID already exists. Try again.";
    case "type_missing":
      return "The chosen entity type is no longer available. Refresh Codex and pick a type again.";
    case "universe_changed":
    case "universe_not_open":
      return "The active universe changed. Reopen the universe before creating records.";
    default: {
      const subject = label ? `${target === "type" ? "Type" : "Entity"} "${label}" creation` : `${target === "type" ? "Type" : "Entity"} creation`;
      return `${subject} could not be confirmed. Use Refresh Codex before creating another.`;
    }
  }
}

export type CodexController = {
  activate: (token: string) => Promise<void>;
  deactivate: () => void;
  resetForUniverse: (token: string) => void;
  isBusy: () => boolean;
};

export function createCodexController(options: {
  root: HTMLElement;
  activeToken: () => string;
  isCodexActive: () => boolean;
  loadIndex: (token: string) => Promise<CodexIndex>;
  loadContext: (token: string, entityId: string) => Promise<EntityContext>;
  createType: (token: string, typeId: string, label: string) => Promise<TypeIndexEntry>;
  createEntity: (
    token: string,
    entityId: string,
    label: string,
    typeId: string | null,
  ) => Promise<CodexEntityEntry>;
}): CodexController {
  const newEntityButton = options.root.querySelector<HTMLButtonElement>("#codex-new-entity")!;
  const newTypeButton = options.root.querySelector<HTMLButtonElement>("#codex-new-type")!;
  const refreshButton = options.root.querySelector<HTMLButtonElement>("#codex-refresh")!;
  const searchInput = options.root.querySelector<HTMLInputElement>("#codex-search")!;
  const typeFilterSelect = options.root.querySelector<HTMLSelectElement>("#codex-type-filter")!;
  const countEl = options.root.querySelector<HTMLElement>("#codex-entity-count")!;
  const statusEl = options.root.querySelector<HTMLElement>("#codex-status")!;
  const entityList = options.root.querySelector<HTMLUListElement>("#codex-list")!;
  const listEmptyEl = options.root.querySelector<HTMLElement>("#codex-list-empty")!;
  const listEmptyText = options.root.querySelector<HTMLElement>("#codex-list-empty-text")!;
  const clearFiltersButton = options.root.querySelector<HTMLButtonElement>("#codex-clear-filters")!;
  const detailTitle = options.root.querySelector<HTMLElement>("#codex-detail-title")!;
  const detailMeta = options.root.querySelector<HTMLElement>("#codex-detail-meta")!;
  const detailContext = options.root.querySelector<HTMLElement>("#codex-detail-context")!;

  const typeDialog = document.querySelector<HTMLDialogElement>("#codex-type-dialog")!;
  const typeForm = document.querySelector<HTMLFormElement>("#codex-type-form")!;
  const typeLabelInput = document.querySelector<HTMLInputElement>("#codex-type-label-input")!;
  const typeErrorEl = document.querySelector<HTMLElement>("#codex-type-error")!;
  const typeCancelButton = document.querySelector<HTMLButtonElement>("#codex-type-cancel")!;
  const typeSubmitButton = document.querySelector<HTMLButtonElement>("#codex-type-submit")!;

  const entityDialog = document.querySelector<HTMLDialogElement>("#codex-entity-dialog")!;
  const entityForm = document.querySelector<HTMLFormElement>("#codex-entity-form")!;
  const entityLabelInput = document.querySelector<HTMLInputElement>("#codex-entity-label-input")!;
  const entityTypeSelect = document.querySelector<HTMLSelectElement>("#codex-entity-type-select")!;
  const entityErrorEl = document.querySelector<HTMLElement>("#codex-entity-error")!;
  const entityCancelButton = document.querySelector<HTMLButtonElement>("#codex-entity-cancel")!;
  const entitySubmitButton = document.querySelector<HTMLButtonElement>("#codex-entity-submit")!;

  let loadedToken = "";
  let types: TypeIndexEntry[] = [];
  let entities: CodexEntityEntry[] = [];
  let searchQuery = "";
  let typeFilter = "__all__";
  let selectedEntityId: string | null = null;
  let indexLoaded = false;
  let loadingIndex = false;
  let indexVersion = 0;
  let universeEpoch = 0;
  let contextVersion = 0;
  let creatingType = false;
  let creatingEntity = false;
  let pendingCreate: PendingCodexCreate | null = null;
  let pendingRefreshAfterWrite: string | null = null;

  function sortIndexData(raw: CodexIndex): CodexIndex {
    return {
      types: [...raw.types].sort(
        (a, b) => compareUtf8Binary(a.label, b.label) || compareUtf8Binary(a.id, b.id),
      ),
      entities: [...raw.entities].sort(
        (a, b) => compareUtf8Binary(a.label, b.label) || compareUtf8Binary(a.id, b.id),
      ),
    };
  }

  function typeIdMap(): Map<string, string[]> {
    const map = new Map<string, string[]>();
    for (const entry of types) {
      const group = map.get(entry.label);
      if (group) group.push(entry.id);
      else map.set(entry.label, [entry.id]);
    }
    return map;
  }

  function entityIdMap(): Map<string, string[]> {
    const map = new Map<string, string[]>();
    for (const entry of entities) {
      const group = map.get(entry.label);
      if (group) group.push(entry.id);
      else map.set(entry.label, [entry.id]);
    }
    return map;
  }

  function displayTypeLabel(typeId: string, fallbackLabel: string, groups: Map<string, string[]>): string {
    return suffixed(fallbackLabel, typeId, groups.get(fallbackLabel) ?? [typeId]);
  }

  function formatEntityTypeText(entity: CodexEntityEntry, typeGroups: Map<string, string[]>): string {
    if (!entity.typeId || entity.typeLabel === null) return "Untyped";
    const match = types.find(entry => entry.id === entity.typeId);
    return displayTypeLabel(entity.typeId, match?.label ?? entity.typeLabel, typeGroups);
  }

  function filteredEntities(): CodexEntityEntry[] {
    const q = searchQuery.trim().toLocaleLowerCase();
    return entities.filter(entity => {
      if (typeFilter === "__untyped__") {
        if (entity.typeId !== null) return false;
      } else if (typeFilter !== "__all__") {
        if (entity.typeId !== typeFilter) return false;
      }
      if (!q.length) return true;
      const haystack = [
        entity.label,
        entity.typeLabel ?? "Untyped",
        entity.primaryPageTitle ?? "",
        entity.id,
      ]
        .join("\n")
        .toLocaleLowerCase();
      return haystack.includes(q);
    });
  }

  function setControlsDisabled(): void {
    const hasToken = !!options.activeToken();
    const busy = loadingIndex || creatingType || creatingEntity;
    const creationLocked = busy || !!pendingCreate;
    newEntityButton.disabled = !hasToken || !indexLoaded || creationLocked;
    newTypeButton.disabled = !hasToken || !indexLoaded || creationLocked;
    refreshButton.disabled = !hasToken || busy;
    searchInput.disabled = !hasToken || !indexLoaded;
    typeFilterSelect.disabled = !hasToken || !indexLoaded;
  }

  function renderTypeFilterOptions(): void {
    const groups = typeIdMap();
    const countsByType = new Map<string, number>();
    let untypedCount = 0;
    for (const entity of entities) {
      if (entity.typeId === null) {
        untypedCount++;
      } else {
        countsByType.set(entity.typeId, (countsByType.get(entity.typeId) ?? 0) + 1);
      }
    }
    if (typeFilter !== "__all__" && typeFilter !== "__untyped__" && !types.some(t => t.id === typeFilter)) {
      typeFilter = "__all__";
    }
    typeFilterSelect.replaceChildren();
    const allOpt = document.createElement("option");
    allOpt.value = "__all__";
    allOpt.textContent = `All types (${entities.length})`;
    typeFilterSelect.append(allOpt);
    for (const typeEntry of types) {
      const opt = document.createElement("option");
      opt.value = typeEntry.id;
      opt.textContent = `${displayTypeLabel(typeEntry.id, typeEntry.label, groups)} (${countsByType.get(typeEntry.id) ?? 0})`;
      typeFilterSelect.append(opt);
    }
    const untypedOpt = document.createElement("option");
    untypedOpt.value = "__untyped__";
    untypedOpt.textContent = `Untyped (${untypedCount})`;
    typeFilterSelect.append(untypedOpt);
    typeFilterSelect.value = typeFilter;
  }

  function renderEntityList(): void {
    const entityGroups = entityIdMap();
    const typeGroups = typeIdMap();
    const visible = filteredEntities();
    countEl.textContent =
      visible.length === entities.length
        ? `${entities.length}`
        : `${visible.length} / ${entities.length}`;
    entityList.replaceChildren();
    if (!indexLoaded) {
      listEmptyEl.hidden = true;
      return;
    }
    if (!visible.length) {
      listEmptyEl.hidden = false;
      const hasFilters = searchQuery.trim().length > 0 || typeFilter !== "__all__";
      clearFiltersButton.hidden = !hasFilters;
      if (!entities.length) {
        listEmptyText.textContent = "No entities yet.";
      } else if (searchQuery.trim().length > 0) {
        listEmptyText.textContent = "No matching entities.";
      } else if (typeFilter === "__untyped__") {
        listEmptyText.textContent = "No untyped entities.";
      } else {
        listEmptyText.textContent = "No entities in this type.";
      }
      return;
    }
    listEmptyEl.hidden = true;
    for (const entity of visible) {
      const li = document.createElement("li");
      const button = document.createElement("button");
      button.type = "button";
      button.className = "codex-entity-item";
      button.dataset.entityId = entity.id;
      button.setAttribute("aria-current", String(selectedEntityId === entity.id));

      const primary = document.createElement("span");
      primary.className = "codex-entity-label";
      primary.textContent = suffixed(entity.label, entity.id, entityGroups.get(entity.label) ?? [entity.id]);

      const secondary = document.createElement("span");
      secondary.className = "codex-entity-sub";
      const typeText = formatEntityTypeText(entity, typeGroups);
      secondary.textContent = entity.primaryPageTitle
        ? `${typeText} · ${entity.primaryPageTitle}`
        : typeText;

      button.append(primary, secondary);
      button.addEventListener("click", () => {
        void selectEntity(entity.id);
      });
      li.append(button);
      entityList.append(li);
    }
  }

  function renderDetailEmpty(): void {
    detailTitle.textContent = entities.length ? "Select an entity" : "Codex";
    detailMeta.replaceChildren();
    detailContext.replaceChildren();
    const prompt = document.createElement("p");
    prompt.className = "codex-detail-placeholder";
    if (!indexLoaded) {
      prompt.textContent = loadingIndex ? "Loading Codex…" : "Open a universe to browse the Codex.";
    } else if (!entities.length) {
      prompt.textContent = "No entities yet. Use New entity or New type to start building this universe.";
    } else if (!filteredEntities().length) {
      prompt.textContent = "No matching entities. Adjust search or type filter.";
    } else {
      prompt.textContent = "Select an entity to inspect its entry.";
    }
    detailContext.append(prompt);
  }

  function renderDetailHeader(entity: CodexEntityEntry): void {
    const entityGroups = entityIdMap();
    const typeGroups = typeIdMap();
    detailTitle.textContent = suffixed(
      entity.label,
      entity.id,
      entityGroups.get(entity.label) ?? [entity.id],
    );
    detailMeta.replaceChildren();

    const typeSpan = document.createElement("span");
    typeSpan.className = entity.typeId ? "codex-meta-item" : "codex-meta-item codex-meta-untyped";
    typeSpan.textContent = formatEntityTypeText(entity, typeGroups);
    detailMeta.append(typeSpan);

    if (entity.primaryPageTitle !== null) {
      const sep = document.createElement("span");
      sep.className = "codex-meta-sep";
      sep.setAttribute("aria-hidden", "true");
      sep.textContent = "·";
      const pageSpan = document.createElement("span");
      pageSpan.className = "codex-meta-item";
      pageSpan.textContent = `Primary document: ${entity.primaryPageTitle}`;
      detailMeta.append(sep, pageSpan);
    }

    const idDisclosure = document.createElement("details");
    idDisclosure.className = "codex-disclosure codex-id-disclosure";
    const idSummary = document.createElement("summary");
    idSummary.textContent = "Identifier";
    const idBody = document.createElement("p");
    idBody.className = "codex-disclosure-body";
    const entityIdSpan = document.createElement("span");
    entityIdSpan.className = "entity-id";
    entityIdSpan.textContent = `Entity ID: ${entity.id}`;
    idBody.append(entityIdSpan);
    if (entity.typeId) {
      const sep = document.createTextNode(" · ");
      const typeIdSpan = document.createElement("span");
      typeIdSpan.className = "entity-id";
      typeIdSpan.textContent = `Type ID: ${entity.typeId}`;
      idBody.append(sep, typeIdSpan);
    }
    idDisclosure.append(idSummary, idBody);
    detailMeta.append(idDisclosure);
  }

  async function loadSelectedContext(entity: CodexEntityEntry): Promise<void> {
    const token = options.activeToken();
    if (!token || !options.isCodexActive()) return;
    const reqVersion = ++contextVersion;
    renderDetailHeader(entity);
    detailContext.replaceChildren();
    const loadingMsg = document.createElement("p");
    loadingMsg.setAttribute("role", "status");
    loadingMsg.className = "codex-detail-placeholder";
    loadingMsg.textContent = "Loading entry…";
    detailContext.append(loadingMsg);
    try {
      const data = await options.loadContext(token, entity.id);
      if (
        reqVersion !== contextVersion ||
        options.activeToken() !== token ||
        !options.isCodexActive() ||
        selectedEntityId !== entity.id
      ) {
        return;
      }
      if (data.entity.id !== entity.id) throw new Error("Entity identity mismatch");
      detailContext.replaceChildren();
      renderCodexEntrySections(detailContext, data, entities);
    } catch (error) {
      if (
        reqVersion !== contextVersion ||
        options.activeToken() !== token ||
        !options.isCodexActive() ||
        selectedEntityId !== entity.id
      ) {
        return;
      }
      detailContext.replaceChildren();
      const errBox = document.createElement("div");
      errBox.className = "codex-detail-error";
      const errMsg = document.createElement("p");
      errMsg.setAttribute("role", "alert");
      const kind = errorKind(error);
      if (kind === "context_too_large") {
        errMsg.textContent =
          "Context too large. This entity exceeds the bounded context read limit.";
        errBox.append(errMsg);
      } else if (kind === "entity_missing") {
        errMsg.textContent =
          "This entity could not be found in the universe. Refresh Codex to update the list.";
        errBox.append(errMsg);
      } else {
        errMsg.textContent =
          kind === "busy"
            ? "Database is busy; persisted knowledge could not be read right now."
            : "Persisted knowledge could not be read. Try again.";
        const retry = document.createElement("button");
        retry.type = "button";
        retry.textContent = "Retry inspection";
        retry.addEventListener("click", () => {
          void selectEntity(entity.id);
        });
        errBox.append(errMsg, retry);
      }
      detailContext.append(errBox);
    }
  }

  async function selectEntity(entityId: string): Promise<void> {
    const entity = entities.find(entry => entry.id === entityId);
    if (!entity) return;
    selectedEntityId = entity.id;
    renderEntityList();
    await loadSelectedContext(entity);
  }

  function renderAll(): void {
    setControlsDisabled();
    renderTypeFilterOptions();
    renderEntityList();
    const selected = selectedEntityId ? entities.find(e => e.id === selectedEntityId) ?? null : null;
    if (!selected) {
      selectedEntityId = null;
      renderDetailEmpty();
    } else {
      renderDetailHeader(selected);
    }
  }

  async function refreshIndex(preferSelectId?: string, preserveStatus?: string): Promise<void> {
    const token = options.activeToken();
    if (!token) return;
    const reqVersion = ++indexVersion;
    const epoch = universeEpoch;
    loadingIndex = true;
    setControlsDisabled();
    if (!indexLoaded) {
      statusEl.textContent = "Loading Codex…";
      renderDetailEmpty();
    }
    try {
      const raw = await options.loadIndex(token);
      if (reqVersion !== indexVersion || epoch !== universeEpoch || options.activeToken() !== token) return;
      const sorted = sortIndexData(raw);
      loadedToken = token;
      types = sorted.types;
      entities = sorted.entities;
      indexLoaded = true;
      pendingRefreshAfterWrite = null;
      refreshButton.hidden = true;

      const reconciling = pendingCreate && pendingCreate.token === token ? pendingCreate : null;
      if (pendingCreate && pendingCreate.token !== token) {
        pendingCreate = null;
      }

      let reconciledEntityId: string | undefined;
      if (reconciling) {
        pendingCreate = null;
        if (reconciling.kind === "entity") {
          const matchedEntity = entities.find(e => e.id === reconciling.id);
          if (matchedEntity) {
            reconciledEntityId = matchedEntity.id;
            statusEl.textContent = `Confirmed entity "${matchedEntity.label}" was saved.`;
          } else {
            statusEl.textContent = `Verified Codex index: entity "${reconciling.label}" was not saved. You may try creating it again.`;
          }
        } else {
          const matchedType = types.find(t => t.id === reconciling.id);
          if (matchedType) {
            statusEl.textContent = `Confirmed type "${matchedType.label}" was saved.`;
          } else {
            statusEl.textContent = `Verified Codex index: type "${reconciling.label}" was not saved. You may try creating it again.`;
          }
        }
      } else {
        statusEl.textContent = preserveStatus ?? "";
      }

      const targetSelectId = preferSelectId ?? reconciledEntityId;
      if (targetSelectId && entities.some(e => e.id === targetSelectId)) {
        selectedEntityId = targetSelectId;
        const createdEntity = entities.find(e => e.id === targetSelectId)!;
        const matchesCurrentFilter = filteredEntities().some(e => e.id === targetSelectId);
        if (!matchesCurrentFilter) {
          searchQuery = "";
          searchInput.value = "";
          typeFilter = "__all__";
        }
        renderAll();
        if (options.isCodexActive()) {
          await loadSelectedContext(createdEntity);
        }
      } else {
        if (selectedEntityId && !entities.some(e => e.id === selectedEntityId)) {
          selectedEntityId = null;
        }
        renderAll();
        if (selectedEntityId && options.isCodexActive()) {
          const current = entities.find(e => e.id === selectedEntityId);
          if (current) await loadSelectedContext(current);
        }
      }
    } catch (error) {
      if (reqVersion !== indexVersion || epoch !== universeEpoch || options.activeToken() !== token) return;
      const kind = errorKind(error);
      refreshButton.hidden = false;
      const pendingForToken = pendingCreate && pendingCreate.token === token ? pendingCreate : null;
      if (pendingForToken) {
        const noun = pendingForToken.kind === "type" ? "type" : "entity";
        statusEl.textContent =
          kind === "busy"
            ? `Database is busy; ${noun} "${pendingForToken.label}" creation is still unconfirmed. Use Refresh Codex to verify.`
            : `Verification failed; ${noun} "${pendingForToken.label}" creation is still unconfirmed. Use Refresh Codex to verify.`;
      } else if (pendingRefreshAfterWrite) {
        statusEl.textContent = `${pendingRefreshAfterWrite} Use Refresh Codex to reload the list.`;
      } else if (!indexLoaded) {
        statusEl.textContent =
          kind === "busy"
            ? "Database is busy. Codex could not be loaded; use Refresh Codex to try again."
            : "Codex could not be loaded. Use Refresh Codex to try again.";
        detailTitle.textContent = "Codex unavailable";
        detailMeta.replaceChildren();
        detailContext.replaceChildren();
        const err = document.createElement("p");
        err.setAttribute("role", "alert");
        err.className = "codex-detail-error";
        err.textContent = statusEl.textContent;
        detailContext.append(err);
      } else {
        statusEl.textContent =
          kind === "busy"
            ? "Database is busy; showing the last loaded Codex list. Use Refresh Codex to update."
            : "Codex could not be refreshed; showing the last loaded list.";
      }
    } finally {
      if (reqVersion === indexVersion && epoch === universeEpoch) {
        loadingIndex = false;
        setControlsDisabled();
      }
    }
  }

  function populateEntityTypeSelect(): void {
    const groups = typeIdMap();
    entityTypeSelect.replaceChildren();
    const untyped = document.createElement("option");
    untyped.value = "";
    untyped.textContent = "Untyped";
    entityTypeSelect.append(untyped);
    for (const typeEntry of types) {
      const opt = document.createElement("option");
      opt.value = typeEntry.id;
      opt.textContent = displayTypeLabel(typeEntry.id, typeEntry.label, groups);
      entityTypeSelect.append(opt);
    }
    entityTypeSelect.value = "";
  }

  function openTypeDialog(): void {
    if (!indexLoaded || creatingType || creatingEntity || loadingIndex || pendingCreate) return;
    typeLabelInput.value = "";
    typeErrorEl.textContent = "";
    typeErrorEl.hidden = true;
    typeSubmitButton.disabled = false;
    typeCancelButton.disabled = false;
    typeDialog.oncancel = event => {
      if (creatingType) {
        event.preventDefault();
        return;
      }
      typeDialog.close();
    };
    typeDialog.showModal();
    typeLabelInput.focus();
  }

  function openEntityDialog(): void {
    if (!indexLoaded || creatingType || creatingEntity || loadingIndex || pendingCreate) return;
    entityLabelInput.value = "";
    populateEntityTypeSelect();
    entityErrorEl.textContent = "";
    entityErrorEl.hidden = true;
    entitySubmitButton.disabled = false;
    entityCancelButton.disabled = false;
    entityDialog.oncancel = event => {
      if (creatingEntity) {
        event.preventDefault();
        return;
      }
      entityDialog.close();
    };
    entityDialog.showModal();
    entityLabelInput.focus();
  }

  newTypeButton.addEventListener("click", openTypeDialog);
  newEntityButton.addEventListener("click", openEntityDialog);
  typeCancelButton.addEventListener("click", () => {
    if (!creatingType) typeDialog.close();
  });
  entityCancelButton.addEventListener("click", () => {
    if (!creatingEntity) entityDialog.close();
  });

  refreshButton.addEventListener("click", () => {
    void refreshIndex();
  });

  searchInput.addEventListener("input", () => {
    searchQuery = searchInput.value;
    renderAll();
  });
  searchInput.addEventListener("keydown", event => {
    if (event.key === "Enter") {
      event.preventDefault();
    }
  });

  typeFilterSelect.addEventListener("change", () => {
    typeFilter = typeFilterSelect.value || "__all__";
    renderAll();
  });

  clearFiltersButton.addEventListener("click", () => {
    searchQuery = "";
    searchInput.value = "";
    typeFilter = "__all__";
    renderAll();
    searchInput.focus();
  });

  typeForm.addEventListener("submit", event => {
    event.preventDefault();
    if (creatingType || pendingCreate) return;
    const validation = validateCodexInput(typeLabelInput.value);
    if (!validation.ok) {
      typeErrorEl.textContent = validation.message;
      typeErrorEl.hidden = false;
      typeLabelInput.focus();
      return;
    }
    const token = options.activeToken();
    if (!token) return;
    const epoch = universeEpoch;
    const typeId = crypto.randomUUID();
    creatingType = true;
    typeErrorEl.hidden = true;
    typeSubmitButton.disabled = true;
    typeCancelButton.disabled = true;
    setControlsDisabled();

    void (async () => {
      try {
        const created = await options.createType(token, typeId, validation.label);
        if (epoch !== universeEpoch || options.activeToken() !== token) {
          typeDialog.close();
          return;
        }
        if (created.id !== typeId) {
          throw new Error("Created type identity mismatch");
        }
        pendingCreate = null;
        typeDialog.close();
        pendingRefreshAfterWrite = `Type "${created.label}" was saved, but Codex failed to refresh.`;
        await refreshIndex(undefined, `Created type "${created.label}".`);
      } catch (error) {
        if (epoch !== universeEpoch || options.activeToken() !== token) {
          typeDialog.close();
          return;
        }
        const message = describeCreateError(error, "type", validation.label);
        if (isDefiniteNonCommit(error)) {
          typeErrorEl.textContent = message;
          typeErrorEl.hidden = false;
          typeLabelInput.focus();
        } else {
          pendingCreate = { token, kind: "type", id: typeId, label: validation.label };
          typeDialog.close();
          refreshButton.hidden = false;
          statusEl.textContent = message;
        }
      } finally {
        if (epoch === universeEpoch) {
          creatingType = false;
          typeSubmitButton.disabled = false;
          typeCancelButton.disabled = false;
          setControlsDisabled();
        }
      }
    })();
  });

  entityForm.addEventListener("submit", event => {
    event.preventDefault();
    if (creatingEntity || pendingCreate) return;
    const validation = validateCodexInput(entityLabelInput.value);
    if (!validation.ok) {
      entityErrorEl.textContent = validation.message;
      entityErrorEl.hidden = false;
      entityLabelInput.focus();
      return;
    }
    const token = options.activeToken();
    if (!token) return;
    const epoch = universeEpoch;
    const entityId = crypto.randomUUID();
    const chosenTypeId = entityTypeSelect.value ? entityTypeSelect.value : null;
    creatingEntity = true;
    entityErrorEl.hidden = true;
    entitySubmitButton.disabled = true;
    entityCancelButton.disabled = true;
    setControlsDisabled();

    void (async () => {
      try {
        const created = await options.createEntity(token, entityId, validation.label, chosenTypeId);
        if (epoch !== universeEpoch || options.activeToken() !== token) {
          entityDialog.close();
          return;
        }
        if (created.id !== entityId) {
          throw new Error("Created entity identity mismatch");
        }
        pendingCreate = null;
        entityDialog.close();
        pendingRefreshAfterWrite = `Entity "${created.label}" was saved, but Codex failed to refresh.`;
        await refreshIndex(created.id, `Created entity "${created.label}".`);
      } catch (error) {
        if (epoch !== universeEpoch || options.activeToken() !== token) {
          entityDialog.close();
          return;
        }
        const message = describeCreateError(error, "entity", validation.label);
        if (isDefiniteNonCommit(error)) {
          entityErrorEl.textContent = message;
          entityErrorEl.hidden = false;
          entityLabelInput.focus();
        } else {
          pendingCreate = { token, kind: "entity", id: entityId, label: validation.label };
          entityDialog.close();
          refreshButton.hidden = false;
          statusEl.textContent = message;
        }
      } finally {
        if (epoch === universeEpoch) {
          creatingEntity = false;
          entitySubmitButton.disabled = false;
          entityCancelButton.disabled = false;
          setControlsDisabled();
        }
      }
    })();
  });

  return {
    async activate(token: string): Promise<void> {
      if (!token) return;
      if (loadedToken !== token) {
        indexLoaded = false;
      }
      await refreshIndex();
    },
    deactivate(): void {
      contextVersion++;
    },
    resetForUniverse(_token: string): void {
      universeEpoch++;
      indexVersion++;
      contextVersion++;
      loadedToken = "";
      types = [];
      entities = [];
      searchQuery = "";
      searchInput.value = "";
      typeFilter = "__all__";
      selectedEntityId = null;
      indexLoaded = false;
      loadingIndex = false;
      creatingType = false;
      creatingEntity = false;
      pendingCreate = null;
      pendingRefreshAfterWrite = null;
      refreshButton.hidden = true;
      statusEl.textContent = "";
      typeSubmitButton.disabled = false;
      typeCancelButton.disabled = false;
      entitySubmitButton.disabled = false;
      entityCancelButton.disabled = false;
      if (typeDialog.open) typeDialog.close();
      if (entityDialog.open) entityDialog.close();
      renderAll();
    },
    isBusy(): boolean {
      return creatingType || creatingEntity || typeDialog.open || entityDialog.open;
    },
  };
}
