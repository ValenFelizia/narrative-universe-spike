import {
  renderCodexEntrySections,
  suffixed,
  type ContextDefinition,
  type ContextOutgoingGroup,
  type EntityContext,
  type RevealProvenance,
} from "./connect.ts";

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

export type CanonLiteral = "ACORDADO" | "PROVISIONAL" | "ABIERTO" | "MISTERIO_INTENCIONAL";
export type ValueClassLiteral = "text" | "boolean" | "reference";
export type CardinalityLiteral = "singular" | "multiple";
export type OwnerKindLiteral = "type" | "entity";

export type AssertionValueInput =
  | { kind: "text"; text: string }
  | { kind: "boolean"; value: boolean }
  | { kind: "reference"; targetEntityId: string }
  | { kind: "unknown" };

export type NewDefinitionInput = {
  definitionId: string;
  ownerKind: OwnerKindLiteral;
  ownerId: string;
  label: string;
  valueClass: ValueClassLiteral;
  cardinality: CardinalityLiteral;
};

export type NewAssertionInput = {
  assertionId: string;
  subjectId: string;
  definitionId: string;
  value: AssertionValueInput;
  canon: CanonLiteral;
  sourceStatusRaw: string | null;
  provenance: RevealProvenance;
  scopeNote: string | null;
};

export type CreateDefinitionRequest = {
  universeToken: string;
  operationId: string;
  definition: NewDefinitionInput;
};

export type CreateAssertionRequest = {
  universeToken: string;
  operationId: string;
  assertion: NewAssertionInput;
};

export type CreateDefinitionWithAssertionRequest = {
  universeToken: string;
  operationId: string;
  definition: NewDefinitionInput;
  assertion: NewAssertionInput;
};

export type PersistedAssertion = {
  id: string;
  subjectId: string;
  definitionId: string;
  value: AssertionValueInput;
  canon: CanonLiteral;
  sourceStatusRaw: string | null;
  provenance: RevealProvenance;
  scopeNote: string | null;
  archived: boolean;
};

export type CreateDefinitionWithAssertionResponse = {
  definition: ContextDefinition;
  assertion: PersistedAssertion;
};

export type ReconcileCodexWriteRequest = {
  universeToken: string;
  operationId: string;
  definitionId: string | null;
  assertionId: string | null;
};

export type ReconcileCodexWriteResponse = {
  operationId: string;
  finalized: boolean;
  definition: ContextDefinition | null;
  assertion: PersistedAssertion | null;
};

type PendingCodexCreate = {
  token: string;
  kind: "type" | "entity";
  id: string;
  label: string;
  notice: string;
};

type PendingKnowledgeWrite = {
  token: string;
  epoch: number;
  operationId: string;
  subjectId: string;
  kind: "definition" | "assertion" | "definition_with_assertion";
  expectedDefinition: ContextDefinition | null;
  expectedAssertion: PersistedAssertion | null;
  summaryLabel: string;
  notice: string;
};

const MAX_CODEX_LABEL_SCALARS = 120;
const MAX_AUTHORED_TEXT_BYTES = 16_384;
const MAX_AUTHORED_SINGLE_LINE_META_BYTES = 1_024;
const MAX_AUTHORED_MULTILINE_META_BYTES = 4_096;
const CONTROL_CHAR_RE = /[\u0000-\u001F\u007F-\u009F]/;
const MULTILINE_CONTROL_CHAR_RE = /[\u0000-\u0008\u000B-\u001F\u007F-\u009F]/;
const SURROUNDING_WHITESPACE_RE = /^\s|\s$/u;
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

/**
 * Validates a property definition label strictly according to DefinitionLabel:
 * 1–120 Unicode scalar values, no leading or trailing Unicode whitespace, and
 * no control characters. Invalid labels are rejected rather than trimmed.
 */
export function validatePropertyLabel(raw: string): { ok: true; label: string } | { ok: false; message: string } {
  if (!raw.length || !raw.trim().length) {
    return { ok: false, message: "Enter a property name." };
  }
  if (SURROUNDING_WHITESPACE_RE.test(raw)) {
    return { ok: false, message: "Remove leading or trailing spaces from the property name." };
  }
  if (CONTROL_CHAR_RE.test(raw)) {
    return { ok: false, message: "Property names must stay on a single line without control characters." };
  }
  if (Array.from(raw).length > MAX_CODEX_LABEL_SCALARS) {
    return { ok: false, message: `Property names cannot exceed ${MAX_CODEX_LABEL_SCALARS} characters.` };
  }
  return { ok: true, label: raw };
}

export function validateAuthoredText(raw: string): { ok: true; text: string } | { ok: false; message: string } {
  if (!raw.trim().length) {
    return { ok: false, message: "Enter nonblank fact text, or choose Record as Unknown." };
  }
  if (MULTILINE_CONTROL_CHAR_RE.test(raw)) {
    return { ok: false, message: "Fact text cannot contain carriage returns, NUL, or control characters." };
  }
  if (utf8Encoder.encode(raw).length > MAX_AUTHORED_TEXT_BYTES) {
    return { ok: false, message: `Fact text cannot exceed ${MAX_AUTHORED_TEXT_BYTES} UTF-8 bytes.` };
  }
  return { ok: true, text: raw };
}

export function validateOptionalMeta(
  raw: string,
  fieldLabel: string,
  maxBytes: number,
  multiline: boolean,
): { ok: true; value: string | null } | { ok: false; message: string } {
  if (!raw.trim().length) {
    return { ok: true, value: null };
  }
  const re = multiline ? MULTILINE_CONTROL_CHAR_RE : CONTROL_CHAR_RE;
  if (re.test(raw)) {
    return {
      ok: false,
      message: multiline
        ? `${fieldLabel} cannot contain carriage returns, NUL, or control characters.`
        : `${fieldLabel} must stay on a single line without control characters.`,
    };
  }
  if (utf8Encoder.encode(raw).length > maxBytes) {
    return { ok: false, message: `${fieldLabel} cannot exceed ${maxBytes} UTF-8 bytes.` };
  }
  return { ok: true, value: raw };
}

function sameDefinition(actual: ContextDefinition | null, expected: ContextDefinition): boolean {
  if (!actual) return false;
  return (
    actual.id === expected.id &&
    actual.ownerKind === expected.ownerKind &&
    actual.ownerId === expected.ownerId &&
    actual.propertyLabel === expected.propertyLabel &&
    actual.valueClass === expected.valueClass &&
    actual.cardinality === expected.cardinality &&
    actual.archived === expected.archived
  );
}

function sameAssertionValue(a: AssertionValueInput, b: AssertionValueInput): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === "text" && b.kind === "text") return a.text === b.text;
  if (a.kind === "boolean" && b.kind === "boolean") return a.value === b.value;
  if (a.kind === "reference" && b.kind === "reference") return a.targetEntityId === b.targetEntityId;
  return true;
}

function sameAssertion(actual: PersistedAssertion | null, expected: PersistedAssertion): boolean {
  if (!actual) return false;
  return (
    actual.id === expected.id &&
    actual.subjectId === expected.subjectId &&
    actual.definitionId === expected.definitionId &&
    sameAssertionValue(actual.value, expected.value) &&
    actual.canon === expected.canon &&
    actual.sourceStatusRaw === expected.sourceStatusRaw &&
    actual.scopeNote === expected.scopeNote &&
    actual.archived === expected.archived &&
    actual.provenance.externalRef === expected.provenance.externalRef &&
    actual.provenance.locator === expected.provenance.locator &&
    actual.provenance.fragment === expected.provenance.fragment &&
    actual.provenance.sourceRevision === expected.provenance.sourceRevision
  );
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
    case "entity_missing":
    case "target_missing":
    case "definition_missing":
    case "definition_archived":
    case "not_applicable":
    case "value_mismatch":
    case "operation_cancelled":
    case "universe_changed":
    case "universe_not_open":
      return true;
    default:
      return false;
  }
}

function describeKnowledgeError(error: unknown, summaryLabel: string): string {
  switch (errorKind(error)) {
    case "busy":
      return "Database is busy. Nothing was saved; try again.";
    case "invalid_input":
      return "Input did not pass validation. Check field lengths, whitespace, and control characters.";
    case "duplicate_id":
      return "Generated record ID already exists. Try again.";
    case "entity_missing":
      return "This entity is no longer available. Refresh Codex to update.";
    case "type_missing":
      return "The owning entity type is no longer available. Refresh Codex to update.";
    case "target_missing":
      return "The selected target entity is no longer available. Refresh Codex and pick another target.";
    case "definition_missing":
      return "This property is no longer available. Refresh Codex to update.";
    case "definition_archived":
      return "This property was archived and cannot accept new facts. Refresh Codex to update.";
    case "not_applicable":
      return "This property does not apply to this entity. Refresh Codex to update.";
    case "value_mismatch":
      return "The fact value does not match this property's value kind.";
    case "operation_cancelled":
      return "This write was superseded by verification and did not execute.";
    case "universe_changed":
    case "universe_not_open":
      return "The active universe changed. Reopen the universe before saving.";
    default:
      return `Could not confirm whether "${summaryLabel}" was saved. Use Verify write before making more changes.`;
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
  hasInFlightWrite: () => boolean;
  unresolvedCloseWarning: () => string | null;
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
  createDefinition: (request: CreateDefinitionRequest) => Promise<ContextDefinition>;
  createAssertion: (request: CreateAssertionRequest) => Promise<PersistedAssertion>;
  createDefinitionWithAssertion: (
    request: CreateDefinitionWithAssertionRequest,
  ) => Promise<CreateDefinitionWithAssertionResponse>;
  reconcileWrite: (request: ReconcileCodexWriteRequest) => Promise<ReconcileCodexWriteResponse>;
}): CodexController {
  const newEntityButton = options.root.querySelector<HTMLButtonElement>("#codex-new-entity")!;
  const newTypeButton = options.root.querySelector<HTMLButtonElement>("#codex-new-type")!;
  const refreshButton = options.root.querySelector<HTMLButtonElement>("#codex-refresh")!;
  const verifyWriteButton = options.root.querySelector<HTMLButtonElement>("#codex-verify-write")!;
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
  const entryStatusEl = options.root.querySelector<HTMLElement>("#codex-entry-status")!;
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

  const propDialog = document.querySelector<HTMLDialogElement>("#codex-property-dialog")!;
  const propForm = document.querySelector<HTMLFormElement>("#codex-prop-form")!;
  const propSubtitle = document.querySelector<HTMLElement>("#codex-prop-subtitle")!;
  const propLabelInput = document.querySelector<HTMLInputElement>("#codex-prop-label-input")!;
  const propDupHint = document.querySelector<HTMLElement>("#codex-prop-dup-hint")!;
  const propOwnerEntityRadio = document.querySelector<HTMLInputElement>("#codex-prop-owner-entity")!;
  const propOwnerEntityText = document.querySelector<HTMLElement>("#codex-prop-owner-entity-text")!;
  const propOwnerTypeWrap = document.querySelector<HTMLElement>("#codex-prop-owner-type-wrap")!;
  const propOwnerTypeRadio = document.querySelector<HTMLInputElement>("#codex-prop-owner-type")!;
  const propOwnerTypeText = document.querySelector<HTMLElement>("#codex-prop-owner-type-text")!;
  const propOwnerUntypedNote = document.querySelector<HTMLElement>("#codex-prop-owner-untyped-note")!;
  const propValueClassSelect = document.querySelector<HTMLSelectElement>("#codex-prop-value-class")!;
  const propCardSingular = document.querySelector<HTMLInputElement>("#codex-prop-card-singular")!;
  const propCardMultiple = document.querySelector<HTMLInputElement>("#codex-prop-card-multiple")!;
  const propIncludeFactCheck = document.querySelector<HTMLInputElement>("#codex-prop-include-fact")!;
  const propFactFields = document.querySelector<HTMLElement>("#codex-prop-fact-fields")!;
  const propValModeValue = document.querySelector<HTMLInputElement>("#codex-prop-val-mode-value")!;
  const propValModeUnknown = document.querySelector<HTMLInputElement>("#codex-prop-val-mode-unknown")!;
  const propValTextWrap = document.querySelector<HTMLElement>("#codex-prop-val-text-wrap")!;
  const propValText = document.querySelector<HTMLTextAreaElement>("#codex-prop-val-text")!;
  const propValBoolWrap = document.querySelector<HTMLElement>("#codex-prop-val-bool-wrap")!;
  const propValBoolYes = document.querySelector<HTMLInputElement>("#codex-prop-val-bool-yes")!;
  const propValBoolNo = document.querySelector<HTMLInputElement>("#codex-prop-val-bool-no")!;
  const propValRefWrap = document.querySelector<HTMLElement>("#codex-prop-val-ref-wrap")!;
  const propValRefSearch = document.querySelector<HTMLInputElement>("#codex-prop-val-ref-search")!;
  const propValRefList = document.querySelector<HTMLElement>("#codex-prop-val-ref-list")!;
  const propValRefSelected = document.querySelector<HTMLElement>("#codex-prop-val-ref-selected")!;
  const propCanonSelect = document.querySelector<HTMLSelectElement>("#codex-prop-canon")!;
  const propSourceDetails = document.querySelector<HTMLDetailsElement>("#codex-prop-source-details")!;
  const propSrcRef = document.querySelector<HTMLInputElement>("#codex-prop-src-ref")!;
  const propSrcLocator = document.querySelector<HTMLInputElement>("#codex-prop-src-locator")!;
  const propSrcRev = document.querySelector<HTMLInputElement>("#codex-prop-src-rev")!;
  const propSrcStatus = document.querySelector<HTMLInputElement>("#codex-prop-src-status")!;
  const propSrcFragment = document.querySelector<HTMLTextAreaElement>("#codex-prop-src-fragment")!;
  const propSrcScope = document.querySelector<HTMLTextAreaElement>("#codex-prop-src-scope")!;
  const propErrorEl = document.querySelector<HTMLElement>("#codex-prop-error")!;
  const propCancelButton = document.querySelector<HTMLButtonElement>("#codex-prop-cancel")!;
  const propSubmitButton = document.querySelector<HTMLButtonElement>("#codex-prop-submit")!;

  const factDialog = document.querySelector<HTMLDialogElement>("#codex-fact-dialog")!;
  const factForm = document.querySelector<HTMLFormElement>("#codex-fact-form")!;
  const factSubtitle = document.querySelector<HTMLElement>("#codex-fact-subtitle")!;
  const factCompetingNote = document.querySelector<HTMLElement>("#codex-fact-competing-note")!;
  const factValModeValue = document.querySelector<HTMLInputElement>("#codex-fact-val-mode-value")!;
  const factValModeUnknown = document.querySelector<HTMLInputElement>("#codex-fact-val-mode-unknown")!;
  const factValTextWrap = document.querySelector<HTMLElement>("#codex-fact-val-text-wrap")!;
  const factValText = document.querySelector<HTMLTextAreaElement>("#codex-fact-val-text")!;
  const factValBoolWrap = document.querySelector<HTMLElement>("#codex-fact-val-bool-wrap")!;
  const factValBoolYes = document.querySelector<HTMLInputElement>("#codex-fact-val-bool-yes")!;
  const factValBoolNo = document.querySelector<HTMLInputElement>("#codex-fact-val-bool-no")!;
  const factValRefWrap = document.querySelector<HTMLElement>("#codex-fact-val-ref-wrap")!;
  const factValRefSearch = document.querySelector<HTMLInputElement>("#codex-fact-val-ref-search")!;
  const factValRefList = document.querySelector<HTMLElement>("#codex-fact-val-ref-list")!;
  const factValRefSelected = document.querySelector<HTMLElement>("#codex-fact-val-ref-selected")!;
  const factCanonSelect = document.querySelector<HTMLSelectElement>("#codex-fact-canon")!;
  const factSourceDetails = document.querySelector<HTMLDetailsElement>("#codex-fact-source-details")!;
  const factSrcRef = document.querySelector<HTMLInputElement>("#codex-fact-src-ref")!;
  const factSrcLocator = document.querySelector<HTMLInputElement>("#codex-fact-src-locator")!;
  const factSrcRev = document.querySelector<HTMLInputElement>("#codex-fact-src-rev")!;
  const factSrcStatus = document.querySelector<HTMLInputElement>("#codex-fact-src-status")!;
  const factSrcFragment = document.querySelector<HTMLTextAreaElement>("#codex-fact-src-fragment")!;
  const factSrcScope = document.querySelector<HTMLTextAreaElement>("#codex-fact-src-scope")!;
  const factErrorEl = document.querySelector<HTMLElement>("#codex-fact-error")!;
  const factCancelButton = document.querySelector<HTMLButtonElement>("#codex-fact-cancel")!;
  const factSubmitButton = document.querySelector<HTMLButtonElement>("#codex-fact-submit")!;

  let loadedToken = "";
  let types: TypeIndexEntry[] = [];
  let entities: CodexEntityEntry[] = [];
  let searchQuery = "";
  let typeFilter = "__all__";
  let selectedEntityId: string | null = null;
  let currentContext: EntityContext | null = null;
  let indexLoaded = false;
  let loadingIndex = false;
  let indexVersion = 0;
  let universeEpoch = 0;
  let contextVersion = 0;
  let creatingType = false;
  let creatingEntity = false;
  let authoringWrite = false;
  let verifyingWrite = false;
  let pendingCreate: PendingCodexCreate | null = null;
  let pendingKnowledgeWrite: PendingKnowledgeWrite | null = null;
  let integrityAnomaly: string | null = null;
  let pendingRefreshAfterWrite: string | null = null;
  let propTargetEntity: CodexEntityEntry | null = null;
  let propSelectedRefId: string | null = null;
  let propReturnFocus: HTMLElement | null = null;
  let propDialogSeq = 0;
  let factTargetEntity: CodexEntityEntry | null = null;
  let factTargetGroup: ContextOutgoingGroup | null = null;
  let factTargetLabel = "";
  let factSelectedRefId: string | null = null;
  let factReturnFocus: HTMLElement | null = null;
  let factDialogSeq = 0;

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

  function persistentLockNotice(): string | null {
    if (integrityAnomaly) return integrityAnomaly;
    if (pendingKnowledgeWrite) return pendingKnowledgeWrite.notice;
    if (pendingCreate) return pendingCreate.notice;
    return null;
  }

  function syncPersistentNotices(): void {
    const notice = persistentLockNotice();
    if (notice) {
      statusEl.textContent = notice;
      entryStatusEl.textContent = notice;
      entryStatusEl.dataset.severity = "warning";
    } else if (entryStatusEl.dataset.severity === "warning") {
      entryStatusEl.textContent = "";
      delete entryStatusEl.dataset.severity;
    }
  }

  function isWriteLocked(): boolean {
    const busy = loadingIndex || creatingType || creatingEntity || authoringWrite || verifyingWrite;
    return busy || !!pendingCreate || !!pendingKnowledgeWrite || !!integrityAnomaly;
  }

  function setControlsDisabled(): void {
    const hasToken = !!options.activeToken();
    const busy = loadingIndex || creatingType || creatingEntity || authoringWrite || verifyingWrite;
    const creationLocked = isWriteLocked();
    newEntityButton.disabled = !hasToken || !indexLoaded || creationLocked;
    newTypeButton.disabled = !hasToken || !indexLoaded || creationLocked;
    refreshButton.disabled = !hasToken || busy;
    verifyWriteButton.hidden = !pendingKnowledgeWrite;
    verifyWriteButton.disabled = !hasToken || busy;
    searchInput.disabled = !hasToken || !indexLoaded;
    typeFilterSelect.disabled = !hasToken || !indexLoaded;
    for (const btn of detailContext.querySelectorAll<HTMLButtonElement>("#codex-new-property, .codex-add-value-btn")) {
      btn.disabled = !hasToken || !indexLoaded || creationLocked;
    }
    syncPersistentNotices();
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
    currentContext = null;
    detailTitle.textContent = entities.length ? "Select an entity" : "Codex";
    detailMeta.replaceChildren();
    const lockNotice = persistentLockNotice();
    if (lockNotice) {
      entryStatusEl.textContent = lockNotice;
      entryStatusEl.dataset.severity = "warning";
    } else {
      entryStatusEl.textContent = "";
      delete entryStatusEl.dataset.severity;
    }
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

  async function loadSelectedContext(
    entity: CodexEntityEntry,
    opts?: { preserveEntryStatus?: boolean },
  ): Promise<void> {
    const token = options.activeToken();
    if (!token || !options.isCodexActive()) return;
    const reqVersion = ++contextVersion;
    currentContext = null;
    if (!opts?.preserveEntryStatus) {
      const lockNotice = persistentLockNotice();
      if (lockNotice) {
        entryStatusEl.textContent = lockNotice;
        entryStatusEl.dataset.severity = "warning";
      } else {
        entryStatusEl.textContent = "";
        delete entryStatusEl.dataset.severity;
      }
    }
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
      currentContext = data;
      detailContext.replaceChildren();
      renderCodexEntrySections(detailContext, data, entities, undefined, {
        disabled: isWriteLocked(),
        onNewProperty: trigger => openPropertyDialog(entity, trigger),
        onAddValue: (group, trigger) => openFactDialog(entity, group, trigger),
      });
    } catch (error) {
      if (
        reqVersion !== contextVersion ||
        options.activeToken() !== token ||
        !options.isCodexActive() ||
        selectedEntityId !== entity.id
      ) {
        return;
      }
      currentContext = null;
      detailContext.replaceChildren();
      const errBox = document.createElement("div");
      errBox.className = "codex-detail-error";
      const errMsg = document.createElement("p");
      errMsg.setAttribute("role", "alert");
      const kind = errorKind(error);
      if (kind === "context_too_large") {
        errMsg.textContent =
          "Context too large. This entity exceeds the bounded context read limit (128 groups, 500 assertions, or 2 MiB); knowledge authoring is unavailable while full context cannot be displayed.";
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
          void loadSelectedContext(entity, { preserveEntryStatus: !!entryStatusEl.textContent });
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
        statusEl.textContent = persistentLockNotice() ?? preserveStatus ?? "";
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
        pendingForToken.notice =
          kind === "busy"
            ? `Database is busy; ${noun} "${pendingForToken.label}" creation is still unconfirmed. Use Refresh Codex to verify.`
            : `Verification failed; ${noun} "${pendingForToken.label}" creation is still unconfirmed. Use Refresh Codex to verify.`;
        statusEl.textContent = pendingForToken.notice;
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
    if (!indexLoaded || isWriteLocked()) return;
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
    if (!indexLoaded || isWriteLocked()) return;
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
    if (isWriteLocked()) return;
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
          pendingCreate = { token, kind: "type", id: typeId, label: validation.label, notice: message };
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
    if (isWriteLocked()) return;
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
          pendingCreate = { token, kind: "entity", id: entityId, label: validation.label, notice: message };
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

  function formatEntityDisplay(entity: CodexEntityEntry): string {
    const entityGroups = entityIdMap();
    return suffixed(entity.label, entity.id, entityGroups.get(entity.label) ?? [entity.id]);
  }

  function formatDefinitionDisplay(def: ContextDefinition, context: EntityContext): string {
    const allGroups = [
      ...context.outgoingGroups,
      ...context.incomingGroups,
      ...context.archivedOutgoingGroups,
      ...context.archivedIncomingGroups,
    ];
    const targetLabel = def.propertyLabel ?? "Unnamed property";
    const sameLabelIds = new Set<string>([def.id]);
    for (const group of allGroups) {
      if ((group.definition.propertyLabel ?? "Unnamed property") === targetLabel) {
        sameLabelIds.add(group.definition.id);
      }
    }
    return suffixed(targetLabel, def.id, [...sameLabelIds]);
  }

  function updateReferenceSelectionInPlace(opts: {
    listEl: HTMLElement;
    selectedEl: HTMLElement;
    selectedId: string | null;
  }): void {
    for (const btn of opts.listEl.querySelectorAll<HTMLButtonElement>(".codex-ref-option")) {
      btn.setAttribute("aria-pressed", String(btn.dataset.entityId === opts.selectedId));
    }
    const chosen = opts.selectedId ? entities.find(e => e.id === opts.selectedId) : null;
    opts.selectedEl.textContent = chosen
      ? `Selected target: ${formatEntityDisplay(chosen)}`
      : "No target entity selected.";
  }

  function renderReferencePicker(opts: {
    listEl: HTMLElement;
    selectedEl: HTMLElement;
    query: string;
    subjectId: string;
    selectedId: string | null;
    onSelect: (id: string) => void;
  }): void {
    const entityGroups = entityIdMap();
    const typeGroups = typeIdMap();
    const q = opts.query.trim().toLocaleLowerCase();
    const matches = entities.filter(entity => {
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
    opts.listEl.replaceChildren();
    if (!matches.length) {
      const empty = document.createElement("p");
      empty.className = "codex-ref-empty";
      empty.textContent = "No matching entities.";
      opts.listEl.append(empty);
    } else {
      for (const candidate of matches) {
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = "codex-ref-option";
        btn.dataset.entityId = candidate.id;
        btn.setAttribute("aria-pressed", String(opts.selectedId === candidate.id));

        const primary = document.createElement("span");
        primary.className = "codex-ref-option-label";
        const disp = suffixed(
          candidate.label,
          candidate.id,
          entityGroups.get(candidate.label) ?? [candidate.id],
        );
        primary.textContent = candidate.id === opts.subjectId ? `${disp} (this entity)` : disp;

        const secondary = document.createElement("span");
        secondary.className = "codex-ref-option-sub";
        const typeText = formatEntityTypeText(candidate, typeGroups);
        secondary.textContent = candidate.primaryPageTitle
          ? `${typeText} · ${candidate.primaryPageTitle}`
          : typeText;

        btn.append(primary, secondary);
        btn.addEventListener("click", () => opts.onSelect(candidate.id));
        opts.listEl.append(btn);
      }
    }
    updateReferenceSelectionInPlace({
      listEl: opts.listEl,
      selectedEl: opts.selectedEl,
      selectedId: opts.selectedId,
    });
  }

  function syncValueControls(opts: {
    valueClass: ValueClassLiteral;
    isUnknown: boolean;
    textWrap: HTMLElement;
    boolWrap: HTMLElement;
    refWrap: HTMLElement;
  }): void {
    opts.textWrap.hidden = opts.isUnknown || opts.valueClass !== "text";
    opts.boolWrap.hidden = opts.isUnknown || opts.valueClass !== "boolean";
    opts.refWrap.hidden = opts.isUnknown || opts.valueClass !== "reference";
  }

  function updatePropDuplicateHint(): void {
    const raw = propLabelInput.value;
    if (!raw.length || !currentContext) {
      propDupHint.hidden = true;
      propDupHint.textContent = "";
      return;
    }
    const duplicate = currentContext.outgoingGroups.some(g => g.definition.propertyLabel === raw);
    if (duplicate) {
      propDupHint.textContent = `An active property named "${raw}" already applies here. Creating another keeps them separate.`;
      propDupHint.hidden = false;
    } else {
      propDupHint.hidden = true;
      propDupHint.textContent = "";
    }
  }

  function syncPropDialogState(): void {
    const includeFact = propIncludeFactCheck.checked;
    propFactFields.hidden = !includeFact;
    propSubmitButton.textContent = includeFact ? "Create & save" : "Create property";
    const valueClass = propValueClassSelect.value as ValueClassLiteral;
    syncValueControls({
      valueClass,
      isUnknown: propValModeUnknown.checked,
      textWrap: propValTextWrap,
      boolWrap: propValBoolWrap,
      refWrap: propValRefWrap,
    });
    if (valueClass === "reference" && propTargetEntity) {
      renderReferencePicker({
        listEl: propValRefList,
        selectedEl: propValRefSelected,
        query: propValRefSearch.value,
        subjectId: propTargetEntity.id,
        selectedId: propSelectedRefId,
        onSelect: id => {
          propSelectedRefId = id;
          updateReferenceSelectionInPlace({
            listEl: propValRefList,
            selectedEl: propValRefSelected,
            selectedId: propSelectedRefId,
          });
        },
      });
    }
  }

  function syncFactDialogState(): void {
    if (!factTargetGroup || !factTargetEntity) return;
    const valueClass = factTargetGroup.definition.valueClass as ValueClassLiteral;
    syncValueControls({
      valueClass,
      isUnknown: factValModeUnknown.checked,
      textWrap: factValTextWrap,
      boolWrap: factValBoolWrap,
      refWrap: factValRefWrap,
    });
    if (valueClass === "reference") {
      renderReferencePicker({
        listEl: factValRefList,
        selectedEl: factValRefSelected,
        query: factValRefSearch.value,
        subjectId: factTargetEntity.id,
        selectedId: factSelectedRefId,
        onSelect: id => {
          factSelectedRefId = id;
          updateReferenceSelectionInPlace({
            listEl: factValRefList,
            selectedEl: factValRefSelected,
            selectedId: factSelectedRefId,
          });
        },
      });
    }
  }

  function restoreFocus(el: HTMLElement | null): void {
    if (el && el.isConnected) {
      el.focus();
    }
  }

  function openPropertyDialog(entity: CodexEntityEntry, trigger: HTMLElement): void {
    if (!indexLoaded || !currentContext || currentContext.entity.id !== entity.id || isWriteLocked()) return;
    propDialogSeq++;
    propTargetEntity = entity;
    propSelectedRefId = null;
    propReturnFocus = trigger;

    const entityDisp = formatEntityDisplay(entity);
    const typeGroups = typeIdMap();
    const typeDisp = formatEntityTypeText(entity, typeGroups);
    propSubtitle.textContent = `For ${entityDisp} (${typeDisp})`;
    propLabelInput.value = "";
    propDupHint.hidden = true;
    propDupHint.textContent = "";

    propOwnerEntityRadio.checked = true;
    propOwnerEntityText.textContent = `Only ${entityDisp}`;
    if (entity.typeId) {
      propOwnerTypeWrap.hidden = false;
      propOwnerTypeRadio.disabled = false;
      propOwnerTypeRadio.checked = false;
      propOwnerTypeText.textContent = `All ${typeDisp} entities (including future entities)`;
      propOwnerUntypedNote.hidden = true;
    } else {
      propOwnerTypeWrap.hidden = true;
      propOwnerTypeRadio.disabled = true;
      propOwnerTypeRadio.checked = false;
      propOwnerUntypedNote.hidden = false;
    }

    propValueClassSelect.value = "text";
    propCardSingular.checked = true;
    propCardMultiple.checked = false;
    propIncludeFactCheck.checked = true;
    propValModeValue.checked = true;
    propValModeUnknown.checked = false;
    propValText.value = "";
    propValBoolYes.checked = false;
    propValBoolNo.checked = false;
    propValRefSearch.value = "";
    propCanonSelect.value = "PROVISIONAL";
    propSourceDetails.open = false;
    propSrcRef.value = "";
    propSrcLocator.value = "";
    propSrcRev.value = "";
    propSrcStatus.value = "";
    propSrcFragment.value = "";
    propSrcScope.value = "";
    propErrorEl.textContent = "";
    propErrorEl.hidden = true;
    propSubmitButton.disabled = false;
    propCancelButton.disabled = false;
    syncPropDialogState();

    propDialog.oncancel = event => {
      if (authoringWrite) {
        event.preventDefault();
        return;
      }
      propDialog.close();
      restoreFocus(propReturnFocus);
    };
    propDialog.showModal();
    propLabelInput.focus();
  }

  function openFactDialog(entity: CodexEntityEntry, group: ContextOutgoingGroup, trigger: HTMLElement): void {
    if (!indexLoaded || !currentContext || currentContext.entity.id !== entity.id || isWriteLocked()) return;
    factDialogSeq++;
    factTargetEntity = entity;
    factTargetGroup = group;
    factSelectedRefId = null;
    factReturnFocus = trigger;

    const entityDisp = formatEntityDisplay(entity);
    const propLabel = formatDefinitionDisplay(group.definition, currentContext);
    factTargetLabel = propLabel;
    const ownerText =
      group.definition.ownerKind === "type"
        ? `All ${formatEntityTypeText(entity, typeIdMap())} entities`
        : `Only ${entityDisp}`;
    const classLabel =
      group.definition.valueClass === "boolean"
        ? "Yes / No"
        : group.definition.valueClass === "reference"
          ? "Entity link"
          : "Text";
    factSubtitle.textContent = `${entityDisp} · ${propLabel} (${ownerText} · ${classLabel})`;
    factCompetingNote.hidden = !(
      group.definition.cardinality === "singular" && group.claims.length > 0
    );

    factValModeValue.checked = true;
    factValModeUnknown.checked = false;
    factValText.value = "";
    factValBoolYes.checked = false;
    factValBoolNo.checked = false;
    factValRefSearch.value = "";
    factCanonSelect.value = "PROVISIONAL";
    factSourceDetails.open = false;
    factSrcRef.value = "";
    factSrcLocator.value = "";
    factSrcRev.value = "";
    factSrcStatus.value = "";
    factSrcFragment.value = "";
    factSrcScope.value = "";
    factErrorEl.textContent = "";
    factErrorEl.hidden = true;
    factSubmitButton.disabled = false;
    factCancelButton.disabled = false;
    syncFactDialogState();

    factDialog.oncancel = event => {
      if (authoringWrite) {
        event.preventDefault();
        return;
      }
      factDialog.close();
      restoreFocus(factReturnFocus);
    };
    factDialog.showModal();
    if (group.definition.valueClass === "text") {
      factValText.focus();
    } else if (group.definition.valueClass === "reference") {
      factValRefSearch.focus();
    } else {
      factValBoolYes.focus();
    }
  }

  function collectFactInputs(opts: {
    valueClass: ValueClassLiteral;
    isUnknown: boolean;
    textValue: string;
    boolYes: boolean;
    boolNo: boolean;
    selectedRefId: string | null;
    canon: string;
    srcRef: string;
    srcLocator: string;
    srcRev: string;
    srcStatus: string;
    srcFragment: string;
    srcScope: string;
  }):
    | {
        ok: true;
        value: AssertionValueInput;
        canon: CanonLiteral;
        sourceStatusRaw: string | null;
        provenance: RevealProvenance;
        scopeNote: string | null;
      }
    | { ok: false; message: string } {
    let value: AssertionValueInput;
    if (opts.isUnknown) {
      value = { kind: "unknown" };
    } else if (opts.valueClass === "text") {
      const checked = validateAuthoredText(opts.textValue);
      if (!checked.ok) return checked;
      value = { kind: "text", text: checked.text };
    } else if (opts.valueClass === "boolean") {
      if (!opts.boolYes && !opts.boolNo) {
        return { ok: false, message: "Choose Yes, No, or Record as Unknown." };
      }
      value = { kind: "boolean", value: opts.boolYes };
    } else {
      if (!opts.selectedRefId) {
        return { ok: false, message: "Select a target entity, or choose Record as Unknown." };
      }
      value = { kind: "reference", targetEntityId: opts.selectedRefId };
    }

    const externalRef = validateOptionalMeta(
      opts.srcRef,
      "Source",
      MAX_AUTHORED_SINGLE_LINE_META_BYTES,
      false,
    );
    if (!externalRef.ok) return externalRef;
    const locator = validateOptionalMeta(
      opts.srcLocator,
      "Locator",
      MAX_AUTHORED_SINGLE_LINE_META_BYTES,
      false,
    );
    if (!locator.ok) return locator;
    const sourceRevision = validateOptionalMeta(
      opts.srcRev,
      "Source revision",
      MAX_AUTHORED_SINGLE_LINE_META_BYTES,
      false,
    );
    if (!sourceRevision.ok) return sourceRevision;
    const sourceStatusRaw = validateOptionalMeta(
      opts.srcStatus,
      "Source status",
      MAX_AUTHORED_SINGLE_LINE_META_BYTES,
      false,
    );
    if (!sourceStatusRaw.ok) return sourceStatusRaw;
    const fragment = validateOptionalMeta(
      opts.srcFragment,
      "Excerpt / fragment",
      MAX_AUTHORED_MULTILINE_META_BYTES,
      true,
    );
    if (!fragment.ok) return fragment;
    const scopeNote = validateOptionalMeta(
      opts.srcScope,
      "Narrative scope",
      MAX_AUTHORED_MULTILINE_META_BYTES,
      true,
    );
    if (!scopeNote.ok) return scopeNote;

    return {
      ok: true,
      value,
      canon: opts.canon as CanonLiteral,
      sourceStatusRaw: sourceStatusRaw.value,
      provenance: {
        externalRef: externalRef.value,
        locator: locator.value,
        fragment: fragment.value,
        sourceRevision: sourceRevision.value,
      },
      scopeNote: scopeNote.value,
    };
  }

  function recordIntegrityAnomaly(message: string): void {
    pendingKnowledgeWrite = null;
    integrityAnomaly = message;
    statusEl.textContent = message;
    entryStatusEl.textContent = message;
    setControlsDisabled();
  }

  async function refreshSelectedEntityAfterCommit(
    subjectId: string,
    confirmationText: string,
  ): Promise<void> {
    statusEl.textContent = confirmationText;
    if (selectedEntityId === subjectId) {
      entryStatusEl.textContent = confirmationText;
      const entity = entities.find(e => e.id === subjectId);
      if (entity && options.isCodexActive()) {
        await loadSelectedContext(entity, { preserveEntryStatus: true });
      }
    }
  }

  async function verifyKnowledgeWrite(): Promise<void> {
    if (!pendingKnowledgeWrite || verifyingWrite || authoringWrite || loadingIndex) return;
    const pending = pendingKnowledgeWrite;
    const token = options.activeToken();
    if (!token || token !== pending.token || universeEpoch !== pending.epoch) {
      pendingKnowledgeWrite = null;
      setControlsDisabled();
      return;
    }
    verifyingWrite = true;
    setControlsDisabled();
    let confirmedRefresh: { subjectId: string; text: string } | null = null;
    try {
      const res = await options.reconcileWrite({
        universeToken: pending.token,
        operationId: pending.operationId,
        definitionId:
          pending.kind === "assertion" ? null : (pending.expectedDefinition?.id ?? null),
        assertionId:
          pending.kind === "definition" ? null : (pending.expectedAssertion?.id ?? null),
      });
      if (universeEpoch !== pending.epoch || options.activeToken() !== pending.token) {
        return;
      }
      if (res.operationId !== pending.operationId) {
        recordIntegrityAnomaly(
          `Integrity anomaly: verification reply did not match operation for "${pending.summaryLabel}". Writes remain locked for this session; reopen or inspect the database before retrying.`,
        );
        return;
      }

      if (pending.kind === "assertion") {
        const expected = pending.expectedAssertion!;
        if (res.assertion) {
          if (sameAssertion(res.assertion, expected)) {
            pendingKnowledgeWrite = null;
            confirmedRefresh = {
              subjectId: pending.subjectId,
              text: `Confirmed: "${pending.summaryLabel}" was saved.`,
            };
          } else {
            recordIntegrityAnomaly(
              `Integrity anomaly: stored record for "${pending.summaryLabel}" does not match the attempted fact. No repair was attempted; writes remain locked for this session. Reopen or inspect the database.`,
            );
          }
        } else if (res.finalized) {
          pendingKnowledgeWrite = null;
          statusEl.textContent = `Verified: "${pending.summaryLabel}" was not saved. You may try again.`;
        } else {
          pending.notice = `Outcome for "${pending.summaryLabel}" is still unconfirmed because the earlier request could not be guaranteed finished. Writes remain locked; verify again or reopen the universe.`;
          statusEl.textContent = pending.notice;
        }
      } else if (pending.kind === "definition") {
        const expected = pending.expectedDefinition!;
        if (res.definition) {
          if (sameDefinition(res.definition, expected)) {
            pendingKnowledgeWrite = null;
            confirmedRefresh = {
              subjectId: pending.subjectId,
              text: `Confirmed: property "${pending.summaryLabel}" was saved.`,
            };
          } else {
            recordIntegrityAnomaly(
              `Integrity anomaly: stored property "${pending.summaryLabel}" does not match the attempted definition. No repair was attempted; writes remain locked for this session. Reopen or inspect the database.`,
            );
          }
        } else if (res.finalized) {
          pendingKnowledgeWrite = null;
          statusEl.textContent = `Verified: property "${pending.summaryLabel}" was not saved. You may try again.`;
        } else {
          pending.notice = `Outcome for property "${pending.summaryLabel}" is still unconfirmed because the earlier request could not be guaranteed finished. Writes remain locked; verify again or reopen the universe.`;
          statusEl.textContent = pending.notice;
        }
      } else {
        const expectedDef = pending.expectedDefinition!;
        const expectedAss = pending.expectedAssertion!;
        const hasDef = res.definition !== null;
        const hasAss = res.assertion !== null;
        if (hasDef && hasAss) {
          const validPair =
            sameDefinition(res.definition, expectedDef) &&
            sameAssertion(res.assertion, expectedAss) &&
            res.assertion!.definitionId === res.definition!.id;
          if (validPair) {
            pendingKnowledgeWrite = null;
            confirmedRefresh = {
              subjectId: pending.subjectId,
              text: `Confirmed: property and fact for "${pending.summaryLabel}" were saved.`,
            };
          } else {
            recordIntegrityAnomaly(
              `Integrity anomaly: stored property/fact for "${pending.summaryLabel}" does not match the attempted write. No repair was attempted; writes remain locked for this session. Reopen or inspect the database.`,
            );
          }
        } else if (!hasDef && !hasAss) {
          if (res.finalized) {
            pendingKnowledgeWrite = null;
            statusEl.textContent = `Verified: nothing was saved for "${pending.summaryLabel}". You may try again.`;
          } else {
            pending.notice = `Outcome for "${pending.summaryLabel}" is still unconfirmed because the earlier request could not be guaranteed finished. Writes remain locked; verify again or reopen the universe.`;
            statusEl.textContent = pending.notice;
          }
        } else {
          const detail = hasDef
            ? "the property exists without its first fact"
            : "the fact exists without its property";
          recordIntegrityAnomaly(
            `Integrity anomaly: unexpected partial state for "${pending.summaryLabel}" (${detail}). Nothing was deleted or auto-repaired; writes remain locked for this session. Reopen or inspect the database.`,
          );
        }
      }
    } catch (error) {
      if (universeEpoch !== pending.epoch || options.activeToken() !== pending.token) {
        return;
      }
      const kind = errorKind(error);
      pending.notice =
        kind === "busy"
          ? `Database is busy; "${pending.summaryLabel}" is still unconfirmed. Use Verify write to try again.`
          : `Verification could not read the database; "${pending.summaryLabel}" is still unconfirmed. Use Verify write to try again.`;
      statusEl.textContent = pending.notice;
    } finally {
      if (universeEpoch === pending.epoch) {
        verifyingWrite = false;
        setControlsDisabled();
      }
    }
    if (confirmedRefresh && universeEpoch === pending.epoch && options.activeToken() === pending.token) {
      await refreshSelectedEntityAfterCommit(confirmedRefresh.subjectId, confirmedRefresh.text);
    }
  }

  propLabelInput.addEventListener("input", updatePropDuplicateHint);
  propValueClassSelect.addEventListener("change", syncPropDialogState);
  propIncludeFactCheck.addEventListener("change", syncPropDialogState);
  propValModeValue.addEventListener("change", syncPropDialogState);
  propValModeUnknown.addEventListener("change", syncPropDialogState);
  propValRefSearch.addEventListener("input", syncPropDialogState);
  propValRefSearch.addEventListener("keydown", event => {
    if (event.key === "Enter") event.preventDefault();
  });
  propCancelButton.addEventListener("click", () => {
    if (!authoringWrite) {
      propDialog.close();
      restoreFocus(propReturnFocus);
    }
  });

  factValModeValue.addEventListener("change", syncFactDialogState);
  factValModeUnknown.addEventListener("change", syncFactDialogState);
  factValRefSearch.addEventListener("input", syncFactDialogState);
  factValRefSearch.addEventListener("keydown", event => {
    if (event.key === "Enter") event.preventDefault();
  });
  factCancelButton.addEventListener("click", () => {
    if (!authoringWrite) {
      factDialog.close();
      restoreFocus(factReturnFocus);
    }
  });

  verifyWriteButton.addEventListener("click", () => {
    void verifyKnowledgeWrite();
  });

  propForm.addEventListener("submit", event => {
    event.preventDefault();
    if (isWriteLocked() || !propTargetEntity) return;

    const labelCheck = validatePropertyLabel(propLabelInput.value);
    if (!labelCheck.ok) {
      propErrorEl.textContent = labelCheck.message;
      propErrorEl.hidden = false;
      propLabelInput.focus();
      return;
    }

    const ownerKind: OwnerKindLiteral =
      propOwnerTypeRadio.checked && propTargetEntity.typeId ? "type" : "entity";
    const ownerId =
      ownerKind === "type" ? propTargetEntity.typeId! : propTargetEntity.id;
    const valueClass = propValueClassSelect.value as ValueClassLiteral;
    const cardinality: CardinalityLiteral = propCardMultiple.checked ? "multiple" : "singular";
    const includeFact = propIncludeFactCheck.checked;

    let factInputs: ReturnType<typeof collectFactInputs> | null = null;
    if (includeFact) {
      factInputs = collectFactInputs({
        valueClass,
        isUnknown: propValModeUnknown.checked,
        textValue: propValText.value,
        boolYes: propValBoolYes.checked,
        boolNo: propValBoolNo.checked,
        selectedRefId: propSelectedRefId,
        canon: propCanonSelect.value,
        srcRef: propSrcRef.value,
        srcLocator: propSrcLocator.value,
        srcRev: propSrcRev.value,
        srcStatus: propSrcStatus.value,
        srcFragment: propSrcFragment.value,
        srcScope: propSrcScope.value,
      });
      if (!factInputs.ok) {
        propErrorEl.textContent = factInputs.message;
        propErrorEl.hidden = false;
        return;
      }
    }

    const token = options.activeToken();
    if (!token) return;
    const epoch = universeEpoch;
    const seq = propDialogSeq;
    const subjectId = propTargetEntity.id;
    const operationId = crypto.randomUUID();
    const definitionId = crypto.randomUUID();
    const expectedDefinition: ContextDefinition = {
      id: definitionId,
      ownerKind,
      ownerId,
      propertyLabel: labelCheck.label,
      valueClass,
      cardinality,
      archived: false,
    };

    const definitionInput: NewDefinitionInput = {
      definitionId,
      ownerKind,
      ownerId,
      label: labelCheck.label,
      valueClass,
      cardinality,
    };

    const assertionId = includeFact ? crypto.randomUUID() : null;
    const expectedAssertion: PersistedAssertion | null =
      includeFact && factInputs && factInputs.ok
        ? {
            id: assertionId!,
            subjectId,
            definitionId,
            value: factInputs.value,
            canon: factInputs.canon,
            sourceStatusRaw: factInputs.sourceStatusRaw,
            provenance: factInputs.provenance,
            scopeNote: factInputs.scopeNote,
            archived: false,
          }
        : null;

    authoringWrite = true;
    propErrorEl.hidden = true;
    propSubmitButton.disabled = true;
    propCancelButton.disabled = true;
    setControlsDisabled();

    void (async () => {
      let confirmedText: string | null = null;
      try {
        if (includeFact && expectedAssertion) {
          const response = await options.createDefinitionWithAssertion({
            universeToken: token,
            operationId,
            definition: definitionInput,
            assertion: {
              assertionId: expectedAssertion.id,
              subjectId,
              definitionId,
              value: expectedAssertion.value,
              canon: expectedAssertion.canon,
              sourceStatusRaw: expectedAssertion.sourceStatusRaw,
              provenance: expectedAssertion.provenance,
              scopeNote: expectedAssertion.scopeNote,
            },
          });
          if (epoch !== universeEpoch || options.activeToken() !== token) return;
          const validReply =
            sameDefinition(response.definition, expectedDefinition) &&
            sameAssertion(response.assertion, expectedAssertion) &&
            response.assertion.definitionId === response.definition.id;
          if (!validReply) {
            if (seq === propDialogSeq && propDialog.open) propDialog.close();
            recordIntegrityAnomaly(
              `Integrity anomaly: acknowledgment for "${labelCheck.label}" did not match the submitted property and fact. Writes remain locked for this session; reopen or inspect the database.`,
            );
            return;
          }
          if (seq === propDialogSeq && propDialog.open) propDialog.close();
          confirmedText = `Saved property "${labelCheck.label}" and its first fact.`;
        } else {
          const response = await options.createDefinition({
            universeToken: token,
            operationId,
            definition: definitionInput,
          });
          if (epoch !== universeEpoch || options.activeToken() !== token) return;
          if (!sameDefinition(response, expectedDefinition)) {
            if (seq === propDialogSeq && propDialog.open) propDialog.close();
            recordIntegrityAnomaly(
              `Integrity anomaly: acknowledgment for property "${labelCheck.label}" did not match the submitted definition. Writes remain locked for this session; reopen or inspect the database.`,
            );
            return;
          }
          if (seq === propDialogSeq && propDialog.open) propDialog.close();
          confirmedText = `Saved empty property "${labelCheck.label}".`;
        }
      } catch (error) {
        if (epoch !== universeEpoch || options.activeToken() !== token) return;
        const message = describeKnowledgeError(error, labelCheck.label);
        if (isDefiniteNonCommit(error)) {
          propErrorEl.textContent = message;
          propErrorEl.hidden = false;
        } else {
          pendingKnowledgeWrite = {
            token,
            epoch,
            operationId,
            subjectId,
            kind: includeFact ? "definition_with_assertion" : "definition",
            expectedDefinition,
            expectedAssertion,
            summaryLabel: labelCheck.label,
            notice: message,
          };
          if (seq === propDialogSeq && propDialog.open) propDialog.close();
          statusEl.textContent = message;
        }
      } finally {
        if (epoch === universeEpoch) {
          authoringWrite = false;
          propSubmitButton.disabled = false;
          propCancelButton.disabled = false;
          setControlsDisabled();
        }
      }
      if (confirmedText && epoch === universeEpoch && options.activeToken() === token) {
        await refreshSelectedEntityAfterCommit(subjectId, confirmedText);
      }
    })();
  });

  factForm.addEventListener("submit", event => {
    event.preventDefault();
    if (isWriteLocked() || !factTargetEntity || !factTargetGroup) return;

    const valueClass = factTargetGroup.definition.valueClass as ValueClassLiteral;
    const factInputs = collectFactInputs({
      valueClass,
      isUnknown: factValModeUnknown.checked,
      textValue: factValText.value,
      boolYes: factValBoolYes.checked,
      boolNo: factValBoolNo.checked,
      selectedRefId: factSelectedRefId,
      canon: factCanonSelect.value,
      srcRef: factSrcRef.value,
      srcLocator: factSrcLocator.value,
      srcRev: factSrcRev.value,
      srcStatus: factSrcStatus.value,
      srcFragment: factSrcFragment.value,
      srcScope: factSrcScope.value,
    });
    if (!factInputs.ok) {
      factErrorEl.textContent = factInputs.message;
      factErrorEl.hidden = false;
      return;
    }

    const token = options.activeToken();
    if (!token) return;
    const epoch = universeEpoch;
    const seq = factDialogSeq;
    const subjectId = factTargetEntity.id;
    const definitionId = factTargetGroup.definition.id;
    const summaryLabel = factTargetLabel || (factTargetGroup.definition.propertyLabel ?? "Unnamed property");
    const operationId = crypto.randomUUID();
    const assertionId = crypto.randomUUID();
    const expectedAssertion: PersistedAssertion = {
      id: assertionId,
      subjectId,
      definitionId,
      value: factInputs.value,
      canon: factInputs.canon,
      sourceStatusRaw: factInputs.sourceStatusRaw,
      provenance: factInputs.provenance,
      scopeNote: factInputs.scopeNote,
      archived: false,
    };

    authoringWrite = true;
    factErrorEl.hidden = true;
    factSubmitButton.disabled = true;
    factCancelButton.disabled = true;
    setControlsDisabled();

    void (async () => {
      let confirmedText: string | null = null;
      try {
        const response = await options.createAssertion({
          universeToken: token,
          operationId,
          assertion: {
            assertionId,
            subjectId,
            definitionId,
            value: expectedAssertion.value,
            canon: expectedAssertion.canon,
            sourceStatusRaw: expectedAssertion.sourceStatusRaw,
            provenance: expectedAssertion.provenance,
            scopeNote: expectedAssertion.scopeNote,
          },
        });
        if (epoch !== universeEpoch || options.activeToken() !== token) return;
        if (!sameAssertion(response, expectedAssertion)) {
          if (seq === factDialogSeq && factDialog.open) factDialog.close();
          recordIntegrityAnomaly(
            `Integrity anomaly: acknowledgment for "${summaryLabel}" did not match the submitted fact. Writes remain locked for this session; reopen or inspect the database.`,
          );
          return;
        }
        if (seq === factDialogSeq && factDialog.open) factDialog.close();
        confirmedText = `Saved fact for "${summaryLabel}".`;
      } catch (error) {
        if (epoch !== universeEpoch || options.activeToken() !== token) return;
        const message = describeKnowledgeError(error, summaryLabel);
        if (isDefiniteNonCommit(error)) {
          factErrorEl.textContent = message;
          factErrorEl.hidden = false;
        } else {
          pendingKnowledgeWrite = {
            token,
            epoch,
            operationId,
            subjectId,
            kind: "assertion",
            expectedDefinition: null,
            expectedAssertion,
            summaryLabel,
            notice: message,
          };
          if (seq === factDialogSeq && factDialog.open) factDialog.close();
          statusEl.textContent = message;
        }
      } finally {
        if (epoch === universeEpoch) {
          authoringWrite = false;
          factSubmitButton.disabled = false;
          factCancelButton.disabled = false;
          setControlsDisabled();
        }
      }
      if (confirmedText && epoch === universeEpoch && options.activeToken() === token) {
        await refreshSelectedEntityAfterCommit(subjectId, confirmedText);
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
      propDialogSeq++;
      factDialogSeq++;
      loadedToken = "";
      types = [];
      entities = [];
      searchQuery = "";
      searchInput.value = "";
      typeFilter = "__all__";
      selectedEntityId = null;
      currentContext = null;
      indexLoaded = false;
      loadingIndex = false;
      creatingType = false;
      creatingEntity = false;
      authoringWrite = false;
      verifyingWrite = false;
      pendingCreate = null;
      pendingKnowledgeWrite = null;
      integrityAnomaly = null;
      pendingRefreshAfterWrite = null;
      propTargetEntity = null;
      propSelectedRefId = null;
      propReturnFocus = null;
      factTargetEntity = null;
      factTargetGroup = null;
      factSelectedRefId = null;
      factReturnFocus = null;
      refreshButton.hidden = true;
      verifyWriteButton.hidden = true;
      statusEl.textContent = "";
      entryStatusEl.textContent = "";
      delete entryStatusEl.dataset.severity;
      typeSubmitButton.disabled = false;
      typeCancelButton.disabled = false;
      entitySubmitButton.disabled = false;
      entityCancelButton.disabled = false;
      propSubmitButton.disabled = false;
      propCancelButton.disabled = false;
      factSubmitButton.disabled = false;
      factCancelButton.disabled = false;
      if (typeDialog.open) typeDialog.close();
      if (entityDialog.open) entityDialog.close();
      if (propDialog.open) propDialog.close();
      if (factDialog.open) factDialog.close();
      renderAll();
    },
    isBusy(): boolean {
      return (
        creatingType ||
        creatingEntity ||
        authoringWrite ||
        verifyingWrite ||
        typeDialog.open ||
        entityDialog.open ||
        propDialog.open ||
        factDialog.open
      );
    },
    hasInFlightWrite(): boolean {
      return creatingType || creatingEntity || authoringWrite || verifyingWrite;
    },
    unresolvedCloseWarning(): string | null {
      const notice = persistentLockNotice();
      if (!notice) return null;
      return `${notice} Closing now ends this session without durable recovery of pending verification state. Close anyway?`;
    },
  };
}
