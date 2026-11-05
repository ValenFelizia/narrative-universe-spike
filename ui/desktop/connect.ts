import { EditorSelection, type Text, type Extension } from "@codemirror/state";
import { Decoration, EditorView, keymap } from "@codemirror/view";
import { undo } from "@codemirror/commands";
import { mentionField, linkSpec, unlinkSpec } from "../src/mention-history.ts";
import { MentionError, sameMentions, validateMentions, type Mention } from "../src/mentions.ts";
import { pendingDrops, type SaveState } from "../src/save-session.ts";
export type EntitySummary = { id: string; label: string; typeLabel: string | null; primaryPageTitle: string | null };
type Capture = { view: EditorView; doc: Text; selection: EditorSelection; mentions: readonly Mention[]; sessionId: string; pageId: string; token: string };
type Picker = { kind: "closed" } | (Capture & ({ kind: "search"; entities: EntitySummary[]; activeId: string | null } | { kind: "inspect"; mention: Mention } | { kind: "loading" }));
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
export function entityCaption(entity: EntitySummary, all: readonly EntitySummary[]): string {
  const caption = (e: EntitySummary) => `${e.label} · ${e.typeLabel ?? "Untyped"} · ${e.primaryPageTitle ?? "No primary page"}`;
  const tied = all.filter(e => caption(e) === caption(entity));
  let length = 4;
  while (length < entity.id.length && tied.some(e => e.id !== entity.id && e.id.slice(-length) === entity.id.slice(-length))) length++;
  return caption(entity) + (tied.length > 1 ? ` · …${entity.id.slice(-length)}` : "");
}
export function connectEditor(options: {
  session: () => SaveState | null; view: () => EditorView | null; token: () => string;
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
    if (focus && held.kind !== "closed" && valid(held)) held.view.focus();
  }
  function position(element: HTMLElement, view: EditorView): void {
    let rect;
    try { rect = view.coordsAtPos(view.state.selection.main.to); } catch { /* Fall back to the viewport corner if coordinates are unavailable. */ }
    const left = rect?.left ?? 0, top = rect?.bottom ?? 0;
    element.style.left = `${Math.max(8, Math.min(left, window.innerWidth - element.offsetWidth - 12))}px`;
    element.style.top = `${Math.max(8, Math.min(top + 6, window.innerHeight - element.offsetHeight - 12))}px`;
  }
  function confirm(c: Capture, entityId: string): void {
    if (picker.kind !== "search" || !picker.entities.some(e => e.id === entityId) || !valid(c) || selectionRefusal(c.view)) { close(); options.message("Selection changed. Select the phrase again."); return; }
    const {from, to} = c.selection.main;
    close(); c.view.dispatch(linkSpec({id: crypto.randomUUID(), entityId, startUtf16: from, endUtf16: to})); c.view.focus();
  }
  async function open(view: EditorView): Promise<void> {
    if (!options.allowed()) { options.message("Linking is unavailable while this document is read-only or its save is unresolved."); return; }
    const s = options.session()!;
    const capture: Capture = {view, doc: view.state.doc, selection: view.state.selection, mentions: view.state.field(mentionField), sessionId: s.sessionId, pageId: s.pageId, token: options.token()};
    const range = view.state.selection.main;
    const mention = range.empty && view.state.selection.ranges.length === 1 ? s.draftMentions.find(m => range.head >= m.startUtf16 && range.head < m.endUtf16) : undefined;
    if (!mention) { const refusal = selectionRefusal(view); if (refusal) { options.message(refusal); return; } }
    close(); picker = {...capture, kind: "loading"}; const pending = picker;
    popup.replaceChildren(); popup.textContent = "Loading entities…"; popup.hidden = false; action.hidden = true; position(popup, view);
    let entities: EntitySummary[];
    try { entities = await options.entities(capture.token); }
    catch { if (picker === pending) { close(true); options.message("Entities could not be read. Your draft is preserved."); } return; }
    if (picker !== pending) return;
    if (!valid(capture)) { close(); return; }
    popup.replaceChildren();
    const cancel = document.createElement("button"); cancel.type = "button"; cancel.textContent = "Cancel · Esc"; cancel.onclick = () => close(true);
    if (mention) {
      picker = {...capture, kind: "inspect", mention};
      const label = document.createElement("p"), entity = entities.find(e => e.id === mention.entityId);
      label.textContent = (entity ? entityCaption(entity, entities) : `Unavailable entity · ${mention.entityId}`) + (s.baseMentions.some(m => sameMentions([m], [mention])) ? " · Saved link" : " · Unsaved link");
      const unlink = document.createElement("button"); unlink.type = "button"; unlink.textContent = "Unlink";
      unlink.onclick = () => { if (picker.kind !== "inspect" || !valid(capture)) { close(); return; } close(); view.dispatch(unlinkSpec(mention.id)); view.focus(); };
      popup.append(label, unlink, cancel); unlink.focus();
    } else {
      const search = document.createElement("input"), results = document.createElement("div");
      search.setAttribute("aria-label", "Search existing entities"); search.setAttribute("role", "combobox"); search.setAttribute("aria-expanded", "true"); search.setAttribute("aria-controls", "entity-results"); search.setAttribute("aria-autocomplete", "list");
      results.id = "entity-results"; results.setAttribute("role", "listbox");
      search.value = capture.doc.sliceString(range.from, range.to);
      picker = {...capture, kind: "search", entities, activeId: null};
      const filtered = () => entities.filter(e => entityCaption(e, entities).toLocaleLowerCase().includes(search.value.toLocaleLowerCase()));
      function render(): void {
        results.replaceChildren(); const state = picker; if (state.kind !== "search") return;
        search.removeAttribute("aria-activedescendant");
        const matches = filtered();
        for (const e of matches) {
          const row = document.createElement("button"); row.type = "button"; row.id = `entity-${e.id}`; row.dataset.entityId = e.id;
          row.setAttribute("role", "option"); row.setAttribute("aria-selected", String(state.activeId === e.id)); const caption = entityCaption(e, entities), suffix = caption.lastIndexOf(" \u00b7 \u2026");
          row.textContent = suffix < 0 ? caption : caption.slice(0, suffix);
          if (suffix >= 0) { const id = document.createElement("span"); id.className = "entity-id"; id.textContent = caption.slice(suffix); row.append(id); }
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
    action.hidden = picker.kind !== "closed" || !view || !options.allowed() || selectionRefusal(view) !== null;
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
