import { EditorSelection, type Text, type Extension } from "@codemirror/state";
import { Decoration, EditorView, keymap } from "@codemirror/view";
import { undo } from "@codemirror/commands";
import { mentionField, linkSpec, unlinkSpec } from "../src/mention-history.ts";
import { MentionError, sameMentions, validateMentions, type Mention } from "../src/mentions.ts";
import { pendingDrops, type SaveState } from "../src/save-session.ts";
export type EntitySummary = { id: string; label: string; typeLabel: string | null; primaryPageTitle: string | null };
export type RevealIdentity = {id: string; label: string};
export type RevealClaim = {
  id: string; definitionId: string; propertyLabel: string | null; valueClass: string; cardinality: string;
  subject: RevealIdentity; value: {kind: "text"; text: string} | {kind: "boolean"; value: boolean} | {kind: "unknown"} | {kind: "reference"; target: RevealIdentity};
  canon: "ACORDADO" | "PROVISIONAL" | "ABIERTO" | "MISTERIO_INTENCIONAL";
  sourceStatusRaw: string | null; scopeNote: string | null;
  provenance: {externalRef: string | null; locator: string | null; fragment: string | null; sourceRevision: string | null};
};
export type EntityReveal = {entity: RevealIdentity; typeLabel: string | null; outgoing: RevealClaim[]; incoming: RevealClaim[]};
type EntityOption = { id: string; caption: string; searchText: string; label: string; suffix: string };
type Capture = { view: EditorView; doc: Text; selection: EditorSelection; mentions: readonly Mention[]; sessionId: string; pageId: string; token: string; scroll: {element: HTMLElement; top: number; left: number}[] };
type Picker = { kind: "closed" } | (Capture & ({ kind: "search"; entities: EntityOption[]; activeId: string | null } | { kind: "inspect"; mention: Mention } | { kind: "facts" } | { kind: "loading" }));
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
export function connectEditor(options: {
  session: () => SaveState | null; view: () => EditorView | null; token: () => string;
  reveal?: (token: string, entityId: string) => Promise<EntityReveal>;
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
  function close(focus = false): void {
    const held = picker; picker = {kind: "closed"}; popup.hidden = true; popup.replaceChildren();
    if (focus && held.kind !== "closed" && valid(held)) { held.view.contentDOM.focus({preventScroll: true}); for (const s of held.scroll) { s.element.scrollTop = s.top; s.element.scrollLeft = s.left; } }
  }
  function position(element: HTMLElement, view: EditorView): void {
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
  async function showFacts(capture: Capture, mention: Mention, savedLink: boolean, index: readonly EntitySummary[], caption: string): Promise<void> {
    if (picker.kind !== "inspect" || !valid(capture) || !options.reveal) return;
    picker = {...capture, kind: "facts"}; const pending = picker;
    popup.setAttribute("aria-label", "Entity facts"); popup.replaceChildren();
    const closeButton = document.createElement("button"); closeButton.type = "button"; closeButton.textContent = "Close facts · Esc"; closeButton.onclick = () => close(true);
    const heading = document.createElement("h2"); heading.textContent = caption;
    const content = document.createElement("div"); content.setAttribute("role", "status"); content.textContent = "Loading persisted facts…";
    popup.append(heading, closeButton, content); closeButton.focus(); position(popup, capture.view);
    try {
      const data = await options.reveal(capture.token, mention.entityId);
      if (picker !== pending) return;
      if (!valid(capture)) { close(); return; }
      if (data.entity.id !== mention.entityId) throw new Error("Identity mismatch");

      content.replaceChildren(); content.removeAttribute("role");
      const note = document.createElement("p"); note.textContent = "Persisted knowledge · point-in-time read"; content.append(note);
      const labels = new Map<string, Set<string>>();
      for (const identity of [...index, data.entity, ...[...data.outgoing, ...data.incoming].flatMap(c => [c.subject, ...(c.value.kind === "reference" ? [c.value.target] : [])])]) {
        const ids = labels.get(identity.label) ?? new Set<string>(); ids.add(identity.id); labels.set(identity.label, ids);
      }
      const name = (identity: RevealIdentity) => {
        const ids = [...(labels.get(identity.label) ?? [])];
        if (ids.length < 2) return identity.label;
        let length = 4;
        while (length < identity.id.length && ids.some(id => id !== identity.id && id.slice(-length) === identity.id.slice(-length))) length++;
        return `${identity.label} · …${identity.id.slice(-length)}`;
      };
      heading.textContent = `${name(data.entity)}${data.typeLabel ? " · " + data.typeLabel : ""} · ${savedLink ? "Saved link" : "Unsaved link"}`;
      const gloss = {ACORDADO: "Agreed", PROVISIONAL: "Provisional", ABIERTO: "Open", MISTERIO_INTENCIONAL: "Intentional mystery"};
      function section(title: string, claims: RevealClaim[]): void {
        const region = document.createElement("section"), heading = document.createElement("h3"), rows = document.createElement("div");
        heading.textContent = `${title} · ${claims.length} recorded claims`; region.append(heading, rows); content.append(region);
        let offset = 0;
        function render(): void {
          rows.replaceChildren();
          if (!claims.length) { rows.textContent = title === "Outgoing facts" ? "No recorded facts" : "No incoming references"; return; }
          for (const claim of claims.slice(offset, offset + 4)) {
            const row = document.createElement("article"); row.className = "fact-row"; row.dataset.assertionId = claim.id; row.dataset.definitionId = claim.definitionId;
            const value = claim.value.kind === "reference" ? name(claim.value.target) : claim.value.kind === "text" ? claim.value.text : claim.value.kind === "boolean" ? (claim.value.value ? "Yes" : "No") : "Unknown (recorded)";
            const text = document.createElement("p"); text.textContent = `${name(claim.subject)} · ${claim.propertyLabel ?? "Unnamed property"} → ${value}`;
            const state = document.createElement("p"); state.textContent = `${claim.canon} · ${gloss[claim.canon]}`;
            const source = document.createElement("details"), summary = document.createElement("summary"), metadata = document.createElement("p");
            const entries = Object.entries(claim.provenance).filter(([,value]) => value !== null);
            summary.textContent = entries.length ? (claim.provenance.locator ?? entries[0]![1]!) : "No source recorded";
            metadata.textContent = [...entries.map(([key,value]) => `${({externalRef: "Source", locator: "Locator", fragment: "Fragment", sourceRevision: "Source revision"} as Record<string, string>)[key]}: ${value}`), ...(claim.sourceStatusRaw !== null ? [`Source status: ${claim.sourceStatusRaw}`] : []), ...(claim.scopeNote !== null ? [`Scope: ${claim.scopeNote}`] : [])].join(" · ");
            source.addEventListener("toggle", () => { if (picker === pending && valid(capture)) position(popup, capture.view); });
            source.append(summary, metadata); row.append(text, state, source); rows.append(row);
          }
          if (claims.length > 4) {
            const count = document.createElement("p"); count.textContent = `Showing ${Math.min(4, claims.length - offset)} of ${claims.length} recorded claims · ${offset + 1}–${Math.min(offset + 4, claims.length)}`;
            const next = document.createElement("button"); next.type = "button"; next.textContent = "Next claims";
            next.onclick = () => { if (picker !== pending || !valid(capture)) { close(); return; } offset = offset + 4 < claims.length ? offset + 4 : 0; render(); rows.querySelector("button")?.focus(); position(popup, capture.view); };
            rows.append(count, next);
          }
        }
        render();
      }
      section("Outgoing facts", data.outgoing); section("Incoming references", data.incoming);
    } catch {
      if (picker !== pending) return;
      if (!valid(capture)) { close(); return; }
      content.textContent = "Facts unavailable. The entity may be unavailable or the database could not be read. Your draft is preserved.";
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
  const reposition = () => { const view = options.view(); if (view) position(picker.kind === "closed" ? action : popup, view); };
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
