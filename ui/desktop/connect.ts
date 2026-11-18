import { EditorSelection, type Text, type Extension } from "@codemirror/state";
import { Decoration, EditorView, keymap } from "@codemirror/view";
import { undo } from "@codemirror/commands";
import { mentionField, linkSpec, unlinkSpec } from "../src/mention-history.ts";
import { MentionError, sameMentions, validateMentions, type Mention } from "../src/mentions.ts";
import { pendingDrops, type SaveState } from "../src/save-session.ts";
export type EntitySummary = { id: string; label: string; typeLabel: string | null; primaryPageTitle: string | null };
export type RevealIdentity = {id: string; label: string};
export type RevealValue = {kind: "text"; text: string} | {kind: "boolean"; value: boolean} | {kind: "unknown"} | {kind: "reference"; target: RevealIdentity};
export type RevealProvenance = {externalRef: string | null; locator: string | null; fragment: string | null; sourceRevision: string | null};
export type RevealClaim = {
  id: string; definitionId: string; propertyLabel: string | null; valueClass: string; cardinality: string;
  subject: RevealIdentity; value: RevealValue;
  canon: "ACORDADO" | "PROVISIONAL" | "ABIERTO" | "MISTERIO_INTENCIONAL";
  sourceStatusRaw: string | null; scopeNote: string | null;
  provenance: RevealProvenance;
};
export type EntityReveal = {entity: RevealIdentity; typeLabel: string | null; outgoing: RevealClaim[]; incoming: RevealClaim[]};
export type ContextDefinition = {
  id: string; ownerKind: "type" | "entity"; ownerId: string;
  propertyLabel: string | null; valueClass: string; cardinality: string; archived: boolean;
};
export type ContextClaim = {
  id: string; value: RevealValue;
  canon: "ACORDADO" | "PROVISIONAL" | "ABIERTO" | "MISTERIO_INTENCIONAL";
  sourceStatusRaw: string | null; provenance: RevealProvenance; scopeNote: string | null; archived: boolean;
};
export type ContextOutgoingGroup = { definition: ContextDefinition; claims: ContextClaim[]; hasArchivedClaims: boolean };
export type ContextIncomingGroup = { subject: RevealIdentity; definition: ContextDefinition; claims: ContextClaim[] };
export type EntityContext = {
  entity: RevealIdentity; typeLabel: string | null;
  outgoingGroups: ContextOutgoingGroup[]; incomingGroups: ContextIncomingGroup[];
  archivedOutgoingGroups: ContextOutgoingGroup[]; archivedIncomingGroups: ContextIncomingGroup[];
};
type EntityOption = { id: string; caption: string; searchText: string; label: string; suffix: string };
type Capture = { view: EditorView; doc: Text; selection: EditorSelection; mentions: readonly Mention[]; sessionId: string; pageId: string; token: string; scroll: {element: HTMLElement; top: number; left: number}[] };
type FactsSnapshot = {
  mention: Mention; savedLink: boolean; index: readonly EntitySummary[]; caption: string;
  data: EntityReveal | null; errorText: string | null;
  outgoingOffset: number; incomingOffset: number; expandedClaims: Set<string>; scrollTop: number;
};
type Picker = { kind: "closed" } | (Capture & ({ kind: "search"; entities: EntityOption[]; activeId: string | null } | { kind: "inspect"; mention: Mention } | { kind: "facts" } | { kind: "context" } | { kind: "loading" }));
const CANON_GLOSS = {ACORDADO: "Agreed", PROVISIONAL: "Provisional", ABIERTO: "Open", MISTERIO_INTENCIONAL: "Intentional mystery"} as const;
const PROVENANCE_LABELS: Record<string, string> = {externalRef: "Source", locator: "Locator", fragment: "Fragment", sourceRevision: "Source revision"};
export function selectionRefusal(view: EditorView): string | null {
  const ranges = view.state.selection.ranges;
  if (ranges.length !== 1) return "Select one phrase to link.";
  const { from, to } = ranges[0]!;
  if (from === to) return "Select the words to link.";
  const text = view.state.doc.toString();
  if (text.slice(from, to).includes("\n")) return "Select words on one line.";
  try { validateMentions({body: text, mentions: [...view.state.field(mentionField), {id: "selection", entityId: "selection", startUtf16: from, endUtf16: to}]}); }
  catch (error) { return (error instanceof MentionError && error.reason === "overlapping_mentions") ? "Unlink the existing occurrence first." : "Select whole characters."; }
  return null;
}
type SelectionContext = { kind: "inspect"; mention: Mention } | { kind: "link" } | { kind: "refused"; reason: string };
function selectionContext(view: EditorView): SelectionContext {
  const ranges = view.state.selection.ranges;
  if (ranges.length === 1) {
    const range = ranges[0]!;
    const mention = view.state.field(mentionField).find(m => range.empty
      ? range.head >= m.startUtf16 && range.head < m.endUtf16
      : range.from === m.startUtf16 && range.to === m.endUtf16);
    if (mention) return {kind: "inspect", mention};
  }
  const reason = selectionRefusal(view);
  return reason ? {kind: "refused", reason} : {kind: "link"};
}
export function prepareEntityIndex(entities: readonly EntitySummary[]): EntityOption[] {
  const rows = entities.map(e => ({id: e.id, caption: `${e.label} · ${e.typeLabel ?? "Untyped"} · ${e.primaryPageTitle ?? "No primary page"}`}));
  const groups = new Map<string, string[]>();
  for (const row of rows) {
    const group = groups.get(row.caption);
    if (group) group.push(row.id); else groups.set(row.caption, [row.id]);
  }
  const suffixCounts = new Map<string, Map<string, number>>();
  for (const [caption, ids] of groups) {
    const counts = new Map<string, number>();
    if (ids.length > 1) for (const id of new Set(ids)) {
      for (let length = 4; length <= id.length; length++) {
        const suffix = id.slice(-length); counts.set(suffix, (counts.get(suffix) ?? 0) + 1);
      }
    }
    suffixCounts.set(caption, counts);
  }
  return rows.map(row => {
    let length = 4;
    const counts = suffixCounts.get(row.caption)!;
    while (length < row.id.length && (counts.get(row.id.slice(-length)) ?? 0) > 1) length++;
    const caption = row.caption + (groups.get(row.caption)!.length > 1 ? ` · …${row.id.slice(-length)}` : "");
    const split = caption.lastIndexOf(" · …");
    return {id: row.id, caption, searchText: caption.toLocaleLowerCase(), label: split < 0 ? caption : caption.slice(0, split), suffix: split < 0 ? "" : caption.slice(split)};
  });
}
// Same-label identities and definitions get the shortest distinguishing ID suffix (at least 4 characters).
export function suffixed(label: string, id: string, ids: readonly string[]): string {
  if (ids.length < 2) return label;
  let length = 4;
  while (length < id.length && ids.some(other => other !== id && other.slice(-length) === id.slice(-length))) length++;
  return `${label} · …${id.slice(-length)}`;
}
function formatValue(value: RevealValue, name: (identity: RevealIdentity) => string): string {
  return value.kind === "reference" ? name(value.target) : value.kind === "text" ? value.text : value.kind === "boolean" ? (value.value ? "Yes" : "No") : "Unknown (recorded)";
}
function buildProvenanceDetails(
  provenance: RevealProvenance,
  sourceStatusRaw: string | null,
  scopeNote: string | null,
  onToggle?: () => void,
  annotateEmptyMetadata = false,
): HTMLDetailsElement {
  const source = document.createElement("details"), summary = document.createElement("summary"), metadata = document.createElement("p");
  const entries = Object.entries(provenance).filter(([, value]) => value !== null) as [string, string][];
  summary.textContent = entries.length
    ? (provenance.locator ?? entries[0]![1])
    : (annotateEmptyMetadata && (sourceStatusRaw !== null || scopeNote !== null) ? "No source recorded · Scope/status noted" : "No source recorded");
  const parts = [
    ...entries.map(([key, value]) => `${PROVENANCE_LABELS[key]}: ${value}`),
    ...(sourceStatusRaw !== null ? [`Source status: ${sourceStatusRaw}`] : []),
    ...(scopeNote !== null ? [`Scope: ${scopeNote}`] : []),
  ];
  metadata.textContent = parts.length ? parts.join(" · ") : (annotateEmptyMetadata ? "No source, status, or scope metadata recorded." : "");
  if (onToggle) source.addEventListener("toggle", onToggle);
  source.append(summary, metadata);
  return source;
}
export function renderEntityContextSections(
  content: HTMLElement,
  data: EntityContext,
  knownIdentities: readonly RevealIdentity[],
  onToggle?: () => void,
): { entityDisplayName: string } {
  const note = document.createElement("p"); note.className = "context-note"; note.textContent = "Persisted knowledge · point-in-time read"; content.append(note);
  const allGroups = [...data.outgoingGroups, ...data.incomingGroups, ...data.archivedOutgoingGroups, ...data.archivedIncomingGroups];
  const labels = new Map<string, Set<string>>();
  const addIdentity = (identity: RevealIdentity) => { const ids = labels.get(identity.label) ?? new Set<string>(); ids.add(identity.id); labels.set(identity.label, ids); };
  for (const identity of knownIdentities) addIdentity(identity);
  addIdentity(data.entity);
  for (const group of [...data.incomingGroups, ...data.archivedIncomingGroups]) addIdentity(group.subject);
  for (const group of allGroups) for (const claim of group.claims) if (claim.value.kind === "reference") addIdentity(claim.value.target);
  const name = (identity: RevealIdentity) => suffixed(identity.label, identity.id, [...(labels.get(identity.label) ?? [])]);

  const properties = new Map<string, Set<string>>();
  for (const group of allGroups) {
    const label = group.definition.propertyLabel ?? "Unnamed property", ids = properties.get(label) ?? new Set<string>(); ids.add(group.definition.id); properties.set(label, ids);
  }
  const property = (def: ContextDefinition) => { const label = def.propertyLabel ?? "Unnamed property"; return suffixed(label, def.id, [...(properties.get(label) ?? [])]); };
  const defMeta = (def: ContextDefinition) => `${def.ownerKind === "type" ? "Type-owned" : "Entity-owned"} · ${def.valueClass} · ${def.cardinality}`;

  const renderClaim = (subject: RevealIdentity, def: ContextDefinition, claim: ContextClaim, archivedSection: boolean): HTMLElement => {
    const row = document.createElement("div"); row.className = "fact-row"; row.dataset.assertionId = claim.id;
    const line = document.createElement("p"); line.textContent = `${name(subject)} · ${property(def)} → ${formatValue(claim.value, name)}`;
    const state = document.createElement("p"); state.className = "context-canon"; state.textContent = `${claim.canon} · ${CANON_GLOSS[claim.canon]}`;
    row.append(line, state);
    if (archivedSection) {
      const archiveLine = document.createElement("p"); archiveLine.className = "context-archive-state";
      archiveLine.textContent = claim.archived ? "Claim archived" : "Claim unarchived (under archived definition)";
      row.append(archiveLine);
    }
    row.append(buildProvenanceDetails(claim.provenance, claim.sourceStatusRaw, claim.scopeNote, onToggle, true));
    return row;
  };

  const renderOutgoingGroup = (group: ContextOutgoingGroup, archivedSection: boolean): HTMLElement => {
    const article = document.createElement("article"); article.className = "context-group"; article.dataset.definitionId = group.definition.id;
    const title = document.createElement("h4"); title.className = "context-group-title"; title.textContent = property(group.definition);
    const meta = document.createElement("p"); meta.className = "context-meta";
    const archivePrefix = archivedSection ? (group.definition.archived ? "Definition archived · " : "Active definition · Archived assertions · ") : "";
    meta.textContent = `${archivePrefix}${defMeta(group.definition)}`;
    article.append(title, meta);
    if (!group.claims.length) {
      const empty = document.createElement("p"); empty.className = "context-empty";
      empty.textContent = archivedSection ? "Archived · No assertions recorded" : (group.hasArchivedClaims ? "Not recorded · Archived history available" : "Not recorded");
      article.append(empty);
    } else {
      for (const claim of group.claims) article.append(renderClaim(data.entity, group.definition, claim, archivedSection));
    }
    return article;
  };

  const renderIncomingGroup = (group: ContextIncomingGroup, archivedSection: boolean): HTMLElement => {
    const article = document.createElement("article"); article.className = "context-group"; article.dataset.subjectId = group.subject.id; article.dataset.definitionId = group.definition.id;
    const title = document.createElement("h4"); title.className = "context-group-title"; title.textContent = `${name(group.subject)} · ${property(group.definition)} → ${name(data.entity)}`;
    const meta = document.createElement("p"); meta.className = "context-meta";
    const archivePrefix = archivedSection ? (group.definition.archived ? "Definition archived · " : "Active definition · Archived assertions · ") : "";
    meta.textContent = `${archivePrefix}${defMeta(group.definition)}`;
    article.append(title, meta);
    for (const claim of group.claims) article.append(renderClaim(group.subject, group.definition, claim, archivedSection));
    return article;
  };

  const outgoingSection = document.createElement("section");
  const outgoingHeading = document.createElement("h3");
  outgoingHeading.textContent = data.outgoingGroups.length ? `Outgoing properties · ${data.outgoingGroups.length}` : "Outgoing properties · No applicable active properties recorded";
  outgoingSection.append(outgoingHeading);
  for (const group of data.outgoingGroups) outgoingSection.append(renderOutgoingGroup(group, false));
  content.append(outgoingSection);

  const incomingSection = document.createElement("section");
  const incomingHeading = document.createElement("h3");
  incomingHeading.textContent = data.incomingGroups.length ? `Incoming references · ${data.incomingGroups.length}` : "Incoming references · No incoming references";
  incomingSection.append(incomingHeading);
  for (const group of data.incomingGroups) incomingSection.append(renderIncomingGroup(group, false));
  content.append(incomingSection);

  const archivedCount = data.archivedOutgoingGroups.length + data.archivedIncomingGroups.length;
  const archivedDetails = document.createElement("details"); archivedDetails.className = "context-archived"; archivedDetails.open = false;
  const archivedSummary = document.createElement("summary");
  archivedSummary.textContent = archivedCount ? `Archived history (${archivedCount})` : "Archived history (None)";
  const archivedNote = document.createElement("p"); archivedNote.className = "context-note";
  archivedNote.textContent = "Stored archive state only (not a temporal revision timeline).";
  archivedDetails.append(archivedSummary, archivedNote);
  if (!archivedCount) {
    const empty = document.createElement("p"); empty.className = "context-empty"; empty.textContent = "No archived properties or assertions recorded.";
    archivedDetails.append(empty);
  } else {
    if (data.archivedOutgoingGroups.length) {
      const sub = document.createElement("h3"); sub.textContent = `Archived outgoing · ${data.archivedOutgoingGroups.length}`;
      archivedDetails.append(sub);
      for (const group of data.archivedOutgoingGroups) archivedDetails.append(renderOutgoingGroup(group, true));
    }
    if (data.archivedIncomingGroups.length) {
      const sub = document.createElement("h3"); sub.textContent = `Archived incoming · ${data.archivedIncomingGroups.length}`;
      archivedDetails.append(sub);
      for (const group of data.archivedIncomingGroups) archivedDetails.append(renderIncomingGroup(group, true));
    }
  }
  if (onToggle) archivedDetails.addEventListener("toggle", onToggle);
  content.append(archivedDetails);
  return { entityDisplayName: name(data.entity) };
}
export function connectEditor(options: {
  session: () => SaveState | null; view: () => EditorView | null; token: () => string;
  reveal?: (token: string, entityId: string) => Promise<EntityReveal>;
  context?: (token: string, entityId: string) => Promise<EntityContext>;
  allowed: () => boolean; entities: (token: string) => Promise<EntitySummary[]>;
  message: (text: string) => void; host: HTMLElement;
}): { extensions: Extension; refresh: () => void; destroy: () => void } {
  let picker: Picker = {kind: "closed"};
  const popup = document.createElement("div"), action = document.createElement("button"), notice = document.createElement("div");
  popup.className = "entity-picker"; popup.hidden = true; popup.setAttribute("role", "dialog"); popup.setAttribute("aria-label", "Link entity");
  action.title = "Link entity (Ctrl+K)"; action.setAttribute("aria-keyshortcuts", "Control+k"); action.className = "link-entity-action"; action.textContent = "Link entity"; action.type = "button"; action.hidden = true;
  notice.className = "link-drop-notice"; notice.hidden = true; notice.setAttribute("role", "status");
  document.body.append(popup, action); options.host.append(notice);
  function valid(c: Capture): boolean {
    const s = options.session(), v = options.view();
    return options.allowed() && v === c.view && v.state.doc === c.doc && v.state.selection.eq(c.selection) && sameMentions(v.state.field(mentionField), c.mentions) && s?.sessionId === c.sessionId && s.pageId === c.pageId && options.token() === c.token;
  }
  function resetPopupStyles(): void {
    popup.classList.remove("facts", "context", "context-edge", "context-anchored");
    delete popup.dataset.contextLayout;
    popup.style.width = "";
    popup.style.maxHeight = "";
    popup.style.left = "";
    popup.style.top = "";
  }
  function close(focus = false): void {
    const held = picker; picker = {kind: "closed"}; popup.hidden = true; popup.replaceChildren(); resetPopupStyles();
    if (focus && held.kind !== "closed" && valid(held)) { held.view.contentDOM.focus({preventScroll: true}); for (const s of held.scroll) { s.element.scrollTop = s.top; s.element.scrollLeft = s.left; } }
  }
  function position(element: HTMLElement, view: EditorView): void {
    if (element === popup && picker.kind === "context") {
      const EDGE_GAP = 24, EDGE_RIGHT_MARGIN = 16, EDGE_MIN_WIDTH = 300, EDGE_MAX_WIDTH = 360;
      const prose = view.dom.closest(".document") ?? view.contentDOM;
      const proseRect = prose.getBoundingClientRect();
      const hostRect = options.host.getBoundingClientRect();
      const vw = window.innerWidth, vh = window.innerHeight;
      const freeRight = vw - proseRect.right - EDGE_GAP - EDGE_RIGHT_MARGIN;
      if (proseRect.width > 0 && freeRight >= EDGE_MIN_WIDTH) {
        const width = Math.min(EDGE_MAX_WIDTH, Math.floor(freeRight));
        const top = Math.max(8, Math.round(hostRect.top > 0 ? hostRect.top + 16 : 84));
        const bottomLimit = hostRect.bottom > top + 160 ? Math.round(hostRect.bottom - 12) : vh - 16;
        const maxHeight = Math.max(200, bottomLimit - top);
        popup.classList.add("context-edge"); popup.classList.remove("context-anchored"); popup.dataset.contextLayout = "edge";
        popup.style.width = `${width}px`; popup.style.maxHeight = `${maxHeight}px`;
        popup.style.left = `${Math.round(proseRect.right + EDGE_GAP)}px`; popup.style.top = `${top}px`;
        return;
      }
      popup.classList.add("context-anchored"); popup.classList.remove("context-edge"); popup.dataset.contextLayout = "anchored";
      popup.style.width = "";
      let rect: { left: number; right: number; top: number; bottom: number } | null | undefined;
      try { rect = view.coordsAtPos(view.state.selection.main.to); } catch { /* Fall back to viewport corner if coordinates are unavailable. */ }
      const topBound = hostRect.top > 0 ? Math.max(8, Math.round(hostRect.top + 8)) : 8;
      const bottomBound = hostRect.bottom > topBound + 160 ? Math.round(hostRect.bottom - 12) : vh - 12;
      const anchorLeft = rect?.left ?? 16, anchorTop = rect?.top ?? topBound, anchorBottom = rect?.bottom ?? topBound;
      const spaceBelow = bottomBound - (anchorBottom + 6), spaceAbove = (anchorTop - 6) - topBound;
      const placeBelow = !rect || spaceBelow >= 260 || spaceBelow >= spaceAbove;
      const avail = Math.max(160, placeBelow ? spaceBelow : spaceAbove);
      popup.style.maxHeight = `${Math.min(520, avail)}px`;
      const left = Math.max(8, Math.min(anchorLeft, vw - popup.offsetWidth - 12));
      const rawTop = placeBelow ? anchorBottom + 6 : Math.max(topBound, anchorTop - 6 - popup.offsetHeight);
      const top = Math.max(topBound, Math.min(rawTop, Math.max(topBound, bottomBound - popup.offsetHeight)));
      popup.style.left = `${Math.round(left)}px`; popup.style.top = `${Math.round(top)}px`;
      return;
    }
    let rect;
    try { rect = view.coordsAtPos(view.state.selection.main.to); } catch { /* Fall back to the viewport corner if coordinates are unavailable. */ }
    const left = rect?.left ?? 0, top = rect?.bottom ?? 0;
    element.style.left = `${Math.max(8, Math.min(left, window.innerWidth - element.offsetWidth - 12))}px`;
    element.style.top = `${Math.max(8, Math.min(top + 6, window.innerHeight - element.offsetHeight - 12))}px`;
  }
  function confirm(c: Capture, entityId: string): void {
    if (picker.kind !== "search" || !picker.entities.some(e => e.id === entityId) || !valid(c) || selectionContext(c.view).kind !== "link") { close(); options.message("Selection changed. Select the phrase again."); return; }
    const {from, to} = c.selection.main;
    close(); c.view.dispatch(linkSpec({id: crypto.randomUUID(), entityId, startUtf16: from, endUtf16: to})); c.view.focus();
  }
  async function showFacts(capture: Capture, mention: Mention, savedLink: boolean, index: readonly EntitySummary[], caption: string, restore?: FactsSnapshot): Promise<void> {
    if ((restore ? picker.kind !== "context" : picker.kind !== "inspect") || !valid(capture) || (!restore && !options.reveal)) return;
    picker = {...capture, kind: "facts"}; const pending = picker;
    resetPopupStyles(); popup.setAttribute("aria-label", "Entity facts"); popup.replaceChildren(); popup.classList.add("facts");
    const snapshot: FactsSnapshot = restore ?? {
      mention, savedLink, index, caption, data: null, errorText: null,
      outgoingOffset: 0, incomingOffset: 0, expandedClaims: new Set<string>(), scrollTop: 0,
    };
    const closeButton = document.createElement("button"); closeButton.type = "button"; closeButton.textContent = "Close facts · Esc"; closeButton.onclick = () => close(true);
    const exploreButton = options.context ? document.createElement("button") : null;
    if (exploreButton) {
      exploreButton.type = "button"; exploreButton.textContent = "Explore context";
      exploreButton.disabled = !restore;
      exploreButton.onclick = () => { snapshot.scrollTop = popup.scrollTop; void showContext(capture, snapshot); };
    }
    const actions = document.createElement("div"); actions.className = "facts-actions";
    actions.append(closeButton, ...(exploreButton ? [exploreButton] : []));
    const heading = document.createElement("h2"); heading.textContent = caption;
    const content = document.createElement("div"); content.setAttribute("role", "status"); content.textContent = "Loading persisted facts…";
    popup.append(heading, actions, content); closeButton.focus(); position(popup, capture.view);
    function renderRevealData(data: EntityReveal): void {
      content.replaceChildren(); content.removeAttribute("role");
      const note = document.createElement("p"); note.textContent = "Persisted knowledge · point-in-time read"; content.append(note);
      const labels = new Map<string, Set<string>>();
      for (const identity of [...index, data.entity, ...[...data.outgoing, ...data.incoming].flatMap(c => [c.subject, ...(c.value.kind === "reference" ? [c.value.target] : [])])]) {
        const ids = labels.get(identity.label) ?? new Set<string>(); ids.add(identity.id); labels.set(identity.label, ids);
      }
      const name = (identity: RevealIdentity) => suffixed(identity.label, identity.id, [...(labels.get(identity.label) ?? [])]);
      // Separate definitions may share a label (or all be unnamed); never let them read as one property.
      const properties = new Map<string, Set<string>>();
      for (const claim of [...data.outgoing, ...data.incoming]) {
        const label = claim.propertyLabel ?? "Unnamed property", ids = properties.get(label) ?? new Set<string>(); ids.add(claim.definitionId); properties.set(label, ids);
      }
      const property = (claim: RevealClaim) => { const label = claim.propertyLabel ?? "Unnamed property"; return suffixed(label, claim.definitionId, [...(properties.get(label) ?? [])]); };
      const titleText = `${name(data.entity)}${data.typeLabel ? " · " + data.typeLabel : ""} · ${savedLink ? "Saved link" : "Unsaved link"}`;
      heading.textContent = titleText; snapshot.caption = titleText;
      const PAGE = 2; // Rows per section; keeps the peek compact while Next claims reaches every claim deterministically.
      function section(title: string, claims: RevealClaim[], getOffset: () => number, setOffset: (n: number) => void): void {
        const region = document.createElement("section"), sectionHeading = document.createElement("h3"), rows = document.createElement("div");
        region.append(sectionHeading); content.append(region);
        // Empty sections collapse to their heading line to keep the peek compact.
        if (!claims.length) { sectionHeading.textContent = title === "Outgoing facts" ? "Outgoing facts · No recorded facts" : "Incoming references · No incoming references"; return; }
        sectionHeading.textContent = `${title} · ${claims.length} recorded claims`; region.append(rows);
        function render(): void {
          rows.replaceChildren();
          const offset = getOffset();
          for (const claim of claims.slice(offset, offset + PAGE)) {
            const row = document.createElement("article"); row.className = "fact-row"; row.dataset.assertionId = claim.id; row.dataset.definitionId = claim.definitionId;
            const text = document.createElement("p"); text.textContent = `${name(claim.subject)} · ${property(claim)} → ${formatValue(claim.value, name)}`;
            const state = document.createElement("p"); state.textContent = `${claim.canon} · ${CANON_GLOSS[claim.canon]}`;
            const key = `${title}:${claim.id}`;
            const source = buildProvenanceDetails(claim.provenance, claim.sourceStatusRaw, claim.scopeNote, () => {
              if (source.open) snapshot.expandedClaims.add(key); else snapshot.expandedClaims.delete(key);
              if (picker === pending && valid(capture)) position(popup, capture.view);
            });
            if (snapshot.expandedClaims.has(key)) source.open = true;
            row.append(text, state, source); rows.append(row);
          }
          if (claims.length > PAGE) {
            const count = document.createElement("p"); count.textContent = `Showing ${Math.min(PAGE, claims.length - offset)} of ${claims.length} recorded claims · ${offset + 1}–${Math.min(offset + PAGE, claims.length)}`;
            const next = document.createElement("button"); next.type = "button"; next.textContent = "Next claims";
            next.onclick = () => { if (picker !== pending || !valid(capture)) { close(); return; } setOffset(offset + PAGE < claims.length ? offset + PAGE : 0); render(); rows.querySelector("button")?.focus(); position(popup, capture.view); };
            rows.append(count, next);
          }
        }
        render();
      }
      section("Outgoing facts", data.outgoing, () => snapshot.outgoingOffset, n => { snapshot.outgoingOffset = n; });
      section("Incoming references", data.incoming, () => snapshot.incomingOffset, n => { snapshot.incomingOffset = n; });
    }
    if (restore) {
      if (restore.data) renderRevealData(restore.data);
      else { content.removeAttribute("role"); content.textContent = restore.errorText ?? "Facts unavailable. The entity may be unavailable or the database could not be read. Your draft is preserved."; }
      if (exploreButton) { exploreButton.disabled = false; exploreButton.focus(); }
      position(popup, capture.view);
      popup.scrollTop = restore.scrollTop;
      return;
    }
    try {
      const data = await options.reveal!(capture.token, mention.entityId);
      if (picker !== pending) return;
      if (!valid(capture)) { close(); return; }
      if (data.entity.id !== mention.entityId) throw new Error("Identity mismatch");
      snapshot.data = data;
      renderRevealData(data);
    } catch {
      if (picker !== pending) return;
      if (!valid(capture)) { close(); return; }
      const errorText = "Facts unavailable. The entity may be unavailable or the database could not be read. Your draft is preserved.";
      snapshot.errorText = errorText;
      content.textContent = errorText;
    }
    if (picker === pending) {
      if (exploreButton) exploreButton.disabled = false;
      position(popup, capture.view);
    }
  }
  async function showContext(capture: Capture, snapshot: FactsSnapshot): Promise<void> {
    if (picker.kind !== "facts" || !valid(capture) || !options.context) return;
    picker = {...capture, kind: "context"}; const pending = picker;
    resetPopupStyles(); popup.setAttribute("aria-label", "Entity context"); popup.replaceChildren(); popup.classList.add("context");
    const header = document.createElement("header"); header.className = "context-header";
    const heading = document.createElement("h2"); heading.textContent = snapshot.caption;
    const actions = document.createElement("div"); actions.className = "context-actions";
    const backButton = document.createElement("button"); backButton.type = "button"; backButton.textContent = "Back to facts";
    backButton.onclick = () => { if (picker !== pending || !valid(capture)) { close(); return; } void showFacts(capture, snapshot.mention, snapshot.savedLink, snapshot.index, snapshot.caption, snapshot); };
    const closeButton = document.createElement("button"); closeButton.type = "button"; closeButton.textContent = "Close · Esc"; closeButton.onclick = () => close(true);
    actions.append(backButton, closeButton); header.append(heading, actions);
    const content = document.createElement("div"); content.setAttribute("role", "status"); content.textContent = "Loading persisted context…";
    popup.append(header, content); backButton.focus(); position(popup, capture.view);
    try {
      const data = await options.context(capture.token, snapshot.mention.entityId);
      if (picker !== pending) return;
      if (!valid(capture)) { close(); return; }
      if (data.entity.id !== snapshot.mention.entityId) throw new Error("Identity mismatch");

      content.replaceChildren(); content.removeAttribute("role");
      const repositionIfCurrent = () => { if (picker === pending && valid(capture)) position(popup, capture.view); };
      const { entityDisplayName } = renderEntityContextSections(content, data, snapshot.index, repositionIfCurrent);
      heading.textContent = `${entityDisplayName}${data.typeLabel ? " · " + data.typeLabel : ""} · ${snapshot.savedLink ? "Saved link" : "Unsaved link"}`;
    } catch (error) {
      if (picker !== pending) return;
      if (!valid(capture)) { close(); return; }
      const kind = typeof error === "object" && error !== null && "kind" in error ? (error as {kind?: unknown}).kind : undefined;
      content.textContent = kind === "context_too_large"
        ? "Context too large. This entity exceeds the bounded context read limit; use compact facts instead. Your draft is preserved."
        : "Context unavailable. The entity may be unavailable or the database could not be read. Your draft is preserved.";
    }
    if (picker === pending) position(popup, capture.view);
  }
  async function open(view: EditorView): Promise<void> {
    if (!options.allowed()) { options.message("Linking is unavailable while this document is read-only or its save is unresolved."); return; }
    const s = options.session()!;
    const capture: Capture = {view, doc: view.state.doc, selection: view.state.selection, mentions: view.state.field(mentionField), sessionId: s.sessionId, pageId: s.pageId, token: options.token(), scroll: [options.host, view.scrollDOM].map(element => ({element, top: element.scrollTop, left: element.scrollLeft}))};
    const range = view.state.selection.main;
    const context = selectionContext(view);
    if (context.kind === "refused") { options.message(context.reason); return; }
    const mention = context.kind === "inspect" ? context.mention : undefined;
    close(); picker = {...capture, kind: "loading"}; const pending = picker;
    popup.replaceChildren(); popup.textContent = "Loading entities…"; popup.hidden = false; action.hidden = true; position(popup, view);
    let index: EntitySummary[];
    try { index = await options.entities(capture.token); }
    catch { if (picker === pending) { close(true); options.message("Entities could not be read. Your draft is preserved."); } return; }
    if (picker !== pending) return;
    if (!valid(capture)) { close(); return; }
    const entities = prepareEntityIndex(index);
    popup.replaceChildren();
    const cancel = document.createElement("button"); cancel.type = "button"; cancel.textContent = "Cancel · Esc"; cancel.onclick = () => close(true);
    if (mention) {
      popup.setAttribute("aria-label", "Inspect link");
      picker = {...capture, kind: "inspect", mention};
      const label = document.createElement("p"), entity = entities.find(e => e.id === mention.entityId);
      label.textContent = (entity ? entity.caption : "Unavailable entity") + (s.baseMentions.some(m => sameMentions([m], [mention])) ? " · Saved link" : " · Unsaved link");
      const unlink = document.createElement("button"); unlink.type = "button"; unlink.textContent = "Unlink";
      unlink.onclick = () => { if (picker.kind !== "inspect" || !valid(capture)) { close(); return; } close(); view.dispatch(unlinkSpec(mention.id)); view.focus(); };
      const facts = document.createElement("button"); facts.type = "button"; facts.textContent = "Show facts";
      facts.disabled = !entity || !options.reveal;
      facts.onclick = () => void showFacts(capture, mention, s.baseMentions.some(m => sameMentions([m], [mention])), index, label.textContent ?? "");
      popup.append(label, facts, unlink, cancel); facts.disabled ? unlink.focus() : facts.focus();
    } else {
      popup.setAttribute("aria-label", "Link entity");
      const search = document.createElement("input"), results = document.createElement("div");
      search.setAttribute("aria-label", "Search existing entities"); search.setAttribute("role", "combobox"); search.setAttribute("aria-expanded", "true"); search.setAttribute("aria-controls", "entity-results"); search.setAttribute("aria-autocomplete", "list");
      results.id = "entity-results"; results.setAttribute("role", "listbox");
      search.value = capture.doc.sliceString(range.from, range.to);
      picker = {...capture, kind: "search", entities, activeId: null};
      const filtered = () => { const query = search.value.toLocaleLowerCase(); return entities.filter(e => e.searchText.includes(query)); };
      function render(): void {
        results.replaceChildren(); const state = picker; if (state.kind !== "search") return;
        search.removeAttribute("aria-activedescendant");
        const matches = filtered();
        for (const e of matches) {
          const row = document.createElement("button"); row.type = "button"; row.id = `entity-${e.id}`; row.dataset.entityId = e.id;
          row.setAttribute("role", "option"); row.setAttribute("aria-selected", String(state.activeId === e.id));
          row.textContent = e.label;
          if (e.suffix) { const id = document.createElement("span"); id.className = "entity-id"; id.textContent = e.suffix; row.append(id); }
          if (state.activeId === e.id) search.setAttribute("aria-activedescendant", row.id);
          row.onclick = () => confirm(capture, e.id); results.append(row);
        }
        if (!matches.length) results.textContent = "No matching entities.";
      }
      search.oninput = () => { if (picker.kind === "search") picker.activeId = null; render(); position(popup, view); };
      search.onkeydown = event => {
        if (picker.kind !== "search") return;
        const matches = filtered();
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault(); const index = matches.findIndex(e => e.id === (picker.kind === "search" ? picker.activeId : null));
          const next = index < 0 ? (event.key === "ArrowDown" ? 0 : matches.length - 1) : (index + (event.key === "ArrowDown" ? 1 : -1) + matches.length) % matches.length;
          picker.activeId = matches[next]?.id ?? null; render(); results.querySelector('[aria-selected="true"]')?.scrollIntoView({block: "nearest"});
        } else if (event.key === "Enter") { event.preventDefault(); if (picker.activeId) confirm(capture, picker.activeId); }
      };
      popup.append(search, results, cancel); render(); search.focus(); search.select();
    }
    position(popup, view);
  }
  popup.onkeydown = event => { if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); close(true); } };
  action.onmousedown = event => event.preventDefault();
  action.onclick = () => { const view = options.view(); if (view) void open(view); };
  const escape = (event: KeyboardEvent) => { if (event.key === "Escape" && picker.kind !== "closed") { event.preventDefault(); event.stopImmediatePropagation(); close(true); } };
  document.addEventListener("keydown", escape, true);
  const outside = (event: PointerEvent) => { if (!popup.contains(event.target as Node) && event.target !== action) close(); };
  const reposition = (event?: Event) => {
    if (event && event.type === "scroll" && event.target instanceof Node && popup.contains(event.target)) return;
    const view = options.view();
    if (view) position(picker.kind === "closed" ? action : popup, view);
  };
  document.addEventListener("pointerdown", outside); window.addEventListener("resize", reposition); document.addEventListener("scroll", reposition, true);
  function refresh(): void {
    const s = options.session(), view = options.view();
    if (picker.kind !== "closed" && !valid(picker)) close();
    const context = view ? selectionContext(view) : null;
    const label = context?.kind === "inspect" ? "Inspect link" : "Link entity";
    action.textContent = label; action.title = `${label} (Ctrl+K)`;
    action.hidden = picker.kind !== "closed" || !view || !options.allowed() || context?.kind === "refused";
    if (!action.hidden && view) position(action, view);
    const drops = s ? pendingDrops(s) : [];
    notice.hidden = drops.length === 0; notice.replaceChildren();
    if (drops.length) {
      notice.append(`${drops.length} ${drops.length === 1 ? "link removed" : "links removed"} by editing. Undo recent edits to restore links. `);
      const button = document.createElement("button"); button.type = "button"; button.textContent = "Undo last change"; button.onclick = () => { const v = options.view(); if (v) { undo(v); v.focus(); } }; notice.append(button);
    }
  }
  const extensions: Extension = [keymap.of([{key: "Mod-k", run(view) { void open(view); return true; }}]), EditorView.decorations.of(view => {
    const saved = options.session()?.baseMentions ?? [];
    return Decoration.set(view.state.field(mentionField).map(m => Decoration.mark({class: "entity-mention " + (saved.some(b => sameMentions([b], [m])) ? "saved" : "draft") + (options.session()?.detail?.includes(m.id) ? " mismatch" : ""), attributes: {"data-mention-id": m.id}}).range(m.startUtf16, m.endUtf16)), true);
  })];
  return {extensions, refresh, destroy() { close(); popup.remove(); action.remove(); notice.remove(); document.removeEventListener("keydown", escape, true); document.removeEventListener("pointerdown", outside); window.removeEventListener("resize", reposition); document.removeEventListener("scroll", reposition, true); }};
}
