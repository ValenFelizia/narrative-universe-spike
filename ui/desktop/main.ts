import type { invoke } from "@tauri-apps/api/core";
import { Compartment, EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { decideEditing } from "../src/adapter.ts";
import { applyReply, beginSave, isDirty, openSession, reconcileBusy, recordEdit, recordTitleEdit, MAX_SUPPORTED_REVISION, type SaveState } from "../src/save-session.ts";

type PageIndexEntry = { id: string; title: string };
type OpenedUniverse = { token: string; pages: PageIndexEntry[] };
type PageContent = { id: string; title: string; body: string; revision: number | null; mentionCount: number };
type SaveResponse = { pageId: string; sessionId: string; requestId: number; body: string; title: string; revision: number };

export function initDesktop(invokeCommand: typeof invoke): { requestClose: (token: string) => void } {
  const form = document.querySelector<HTMLFormElement>("#open-form")!;
  const input = document.querySelector<HTMLInputElement>("#database-path")!;
  const name = document.querySelector<HTMLElement>("#universe-name")!;
  const count = document.querySelector<HTMLElement>("#page-count")!;
  const list = document.querySelector<HTMLUListElement>("#page-list")!;
  const status = document.querySelector<HTMLElement>("#status")!;
  const title = document.querySelector<HTMLElement>("#page-title")!;
  const titleInput = document.querySelector<HTMLInputElement>("#title-input")!;
  const newButton = document.querySelector<HTMLButtonElement>("#new-page")!;
  const verifyCreate = document.querySelector<HTMLButtonElement>("#verify-create")!;
  const pane = document.querySelector<HTMLElement>(".reading-area")!;
  const body = document.querySelector<HTMLElement>("#page-body")!;
  const saveButton = document.querySelector<HTMLButtonElement>("#save")!;
  const verifyButton = document.querySelector<HTMLButtonElement>("#verify")!;
  const saveStatus = document.querySelector<HTMLElement>("#save-status")!;
  const dialog = document.querySelector<HTMLDialogElement>("#guard")!;
  const keep = document.querySelector<HTMLButtonElement>("#continue")!;
  const saveProceed = document.querySelector<HTMLButtonElement>("#save-proceed")!;
  const discard = document.querySelector<HTMLButtonElement>("#discard")!;
  let universeToken = "";
  let session: SaveState | null = null;
  let selectedPageId: string | null = null;
  let editor: EditorView | null = null;
  const editability = new Compartment();
  let version = 0;
  let loading = false;
  let deciding = false;
  let transitioning = false;
  let saving: Promise<boolean> | null = null;
  let uncertain = false;
  let verifying = false;
  let pendingCreate: {token: string; id: string} | null = null;
  let pages: PageIndexEntry[] = [];

  function paint(): void {
    newButton.disabled = !universeToken || !!pendingCreate || transitioning || loading || deciding || verifying || !!saving;
    verifyCreate.hidden = !pendingCreate;
    titleInput.readOnly = !session || loading || deciding || transitioning || session.baseRevision >= MAX_SUPPORTED_REVISION;
    if (!session) { saveButton.disabled = true; verifyButton.hidden = true; return; }
    const labels = {clean: "Saved", dirty: "Unsaved", saving: "Saving…", stale: "Changed elsewhere. Copy your draft before leaving.", busy: "Database busy. Your draft is preserved.", error: "Save could not be confirmed. Your draft is preserved."};
    saveStatus.textContent = session.baseRevision >= MAX_SUPPORTED_REVISION ? "Revision limit reached. Copy any unsaved draft; this page is read-only." : uncertain ? "Save could not be confirmed. Continue editing to select and copy your draft." : labels[session.status];
    saveButton.disabled = transitioning || deciding || loading || verifying || session.baseRevision >= MAX_SUPPORTED_REVISION || !isDirty(session) || session.inFlight !== null || ["stale", "error", "busy"].includes(session.status);
    verifyButton.hidden = session.status !== "busy" || !uncertain;
    verifyButton.disabled = transitioning || deciding || loading || verifying;
  }

  async function verifyBusy(): Promise<void> {
    if (!session || session.status !== "busy" || !uncertain || verifying || loading || deciding || transitioning) return;
    const identity = session.sessionId, pageId = session.pageId, token = universeToken;
    verifying = true; paint();
    try {
      const page = await invokeCommand<PageContent>("read_page", {pageId, universeToken: token});
      if (session && session.sessionId === identity && page.revision !== null && page.mentionCount === 0) {
        session = reconcileBusy(session, {sessionId: identity, pageId: page.id, doc: page.body, title: page.title, revision: page.revision});
        uncertain = session.status !== "dirty";
      }
    } catch { /* Preserve the draft and allow another verification. */ }
    finally { verifying = false; paint(); }
  }

  async function save(fromGuard = false): Promise<boolean> {
    if (verifying || (!fromGuard && (deciding || transitioning || loading))) return false;
    if (saving) return saving;
    if (!session || ["stale", "error", "busy"].includes(session.status)) return false;
    if (!isDirty(session)) return true;
    let started;
    try { started = beginSave(session); } catch { saveStatus.textContent = "This edit cannot be saved. Your draft is preserved."; return false; }
    if (!started.ok) return false;
    const request = started.request;
    session = started.state;
    paint();
    saving = (async () => {
      try {
        const response = await invokeCommand<SaveResponse>("save_page", { request: { universeToken, pageId: request.pageId, sessionId: request.sessionId, requestId: request.requestId, expectedRevision: request.expectedRevision, title: request.titleChanged ? request.title : null, edits: request.edits } });
        if (!session || session.sessionId !== request.sessionId) return false;
        const outcome = applyReply(session, {status: "ok", requestId: response.requestId, sessionId: response.sessionId, pageId: response.pageId, doc: response.body, title: response.title, revision: response.revision});
        session = outcome.ignored ? {...outcome.state, status: "error", detail: "Save acknowledgement identity did not match."} : outcome.state;
        uncertain = !!outcome.ignored || !outcome.applied;
        if (outcome.applied && request.titleChanged) { pages = pages.map(p => p.id === request.pageId ? {...p, title: request.title} : p); renderPages(); }
      } catch (error) {
        if (!session || session.sessionId !== request.sessionId) return false;
        const failure = typeof error === "object" && error !== null ? error as Partial<SaveResponse> & {kind?: unknown} : {};
        if (failure.pageId !== request.pageId || failure.sessionId !== request.sessionId || failure.requestId !== request.requestId) {
          session = {...session, status: "error", detail: "Save outcome could not be identified."};
          uncertain = true;
          paint();
          return false;
        }
        const kind = String(failure.kind);
        const replyStatus = kind === "stale" ? "stale" : kind === "busy" ? "busy" : "error";
        uncertain = replyStatus !== "stale";
        session = applyReply(session, {status: replyStatus, requestId: request.requestId, sessionId: request.sessionId, pageId: request.pageId, detail: kind}).state;
        if (replyStatus === "busy") {
          try {
            const page = await invokeCommand<PageContent>("read_page", {pageId: request.pageId, universeToken});
            if (session && session.sessionId === request.sessionId && page.revision !== null && page.mentionCount === 0) {
              session = reconcileBusy(session, {sessionId: request.sessionId, pageId: page.id, doc: page.body, title: page.title, revision: page.revision});
              uncertain = session.status !== "dirty";
            }
          } catch { /* Keep the busy state and draft. */ }
        }
      }
      if (session && session.baseRevision >= MAX_SUPPORTED_REVISION && editor) editor.dispatch({effects: editability.reconfigure([EditorState.readOnly.of(true), EditorView.editable.of(false)])});
      paint();
      return !!session && session.status === "clean" && !isDirty(session);
    })();
    try { return await saving; } finally { saving = null; paint(); }
  }

  async function mayLeave(): Promise<boolean> {
    if (deciding || loading || saving || verifying) return false;
    if (!session || (!isDirty(session) && !session.inFlight && session.status === "clean")) return true;
    deciding = true;
    saveProceed.disabled = !!session.inFlight || ["stale", "busy", "error"].includes(session.status);
    discard.disabled = !!session.inFlight || uncertain;
    const choice = await new Promise<"keep" | "save" | "discard">(resolve => {
      const done = (value: "keep" | "save" | "discard") => { dialog.close(); resolve(value); };
      keep.onclick = () => done("keep"); saveProceed.onclick = () => done("save"); discard.onclick = () => done("discard");
      dialog.oncancel = event => { event.preventDefault(); done("keep"); };
      dialog.showModal(); keep.focus();
    });
    let result = choice === "discard";
    if (choice === "save") result = await save(true);
    deciding = false;
    return result;
  }

  function clearPage(): void { editor?.destroy(); editor = null; session = null; uncertain = false; body.replaceChildren(); saveStatus.textContent = ""; paint(); }

  async function selectPage(id: string, guarded = true): Promise<void> {
    if (guarded) {
      if (transitioning) return;
      transitioning = true;
      paint();
      if (!(await mayLeave())) { transitioning = false; paint(); return; }
    }
    const requestVersion = ++version;
    loading = true;
    try {
      const page = await invokeCommand<PageContent>("read_page", {pageId: id, universeToken});
      if (requestVersion !== version) return;
      clearPage();
      selectedPageId = page.id;
      title.textContent = page.title; titleInput.value = page.title; titleInput.hidden = false; title.hidden = true;
      pane.scrollTop = 0;
      for (const button of list.querySelectorAll<HTMLButtonElement>("button")) button.setAttribute("aria-current", String(button.dataset.pageId === selectedPageId));
      const decision = decideEditing(page.body, page.mentionCount);
      const titleDecision = decideEditing(page.title, 0);
      if (!decision.editable || !titleDecision.editable || page.title.includes("\n") || page.revision === null || page.revision >= MAX_SUPPORTED_REVISION) {
        title.hidden = false; titleInput.hidden = true;
        body.textContent = page.body;
        saveStatus.textContent = page.title.includes("\n") ? "This page title contains line breaks and is read-only." : !decision.editable ? ({linked:"This page contains links and is read-only.", carriage_return:"This page contains carriage returns and is read-only.", nul:"This page contains NUL and is read-only.", unpaired_surrogate:"This page contains unsupported text and is read-only."})[decision.reason] : !titleDecision.editable ? "This page title contains unsupported text and is read-only." : "This page revision is not editable.";
      } else {
        session = openSession({sessionId: crypto.randomUUID(), pageId: page.id, doc: page.body, title: page.title, revision: page.revision, hasMentions: false});
        editor = new EditorView({parent: body, state: EditorState.create({doc: page.body, extensions: [history(), keymap.of([...defaultKeymap, ...historyKeymap]), EditorView.lineWrapping, editability.of([])]}), dispatchTransactions(transactions, view) {
          if (!session || (transactions.some(transaction => transaction.docChanged) && (loading || deciding || session.baseRevision >= MAX_SUPPORTED_REVISION))) return;
          let next = session;
          try { for (const transaction of transactions) next = recordEdit(next, transaction.changes); }
          catch { saveStatus.textContent = "This edit is not supported; the draft was not changed."; return; }
          view.update(transactions); session = next; paint();
        }});
        paint();
      }
      status.textContent = "";
    } catch { if (requestVersion === version) status.textContent = "The page could not be read. Your current draft is preserved."; }
    finally { if (requestVersion === version) loading = false; if (guarded) transitioning = false; paint(); }
  }


  function renderPages(): void {
    // SQLite BINARY compares UTF-8 bytes; do not use locale-dependent sorting.
    const bytes = new TextEncoder();
    const compare = (a: string, b: string) => { const x = bytes.encode(a), y = bytes.encode(b); for (let i = 0; i < Math.min(x.length, y.length); i++) if (x[i] !== y[i]) return x[i]! - y[i]!; return x.length - y.length; };
    pages.sort((a,b) => compare(a.title,b.title) || compare(a.id,b.id));
    count.textContent = String(pages.length); list.replaceChildren();
    for (const page of pages) {
      const li = document.createElement("li"), button = document.createElement("button");
      button.type = "button"; button.className = "page-link"; button.textContent = page.title; button.dataset.pageId = page.id;
      button.setAttribute("aria-current", String(selectedPageId === page.id));
      button.addEventListener("click", () => void selectPage(page.id)); li.append(button); list.append(li);
    }
  }
  titleInput.addEventListener("input", () => {
    if (!session || titleInput.readOnly) return;
    try { session = recordTitleEdit(session, titleInput.value); paint(); }
    catch { titleInput.value = session.draftTitle; saveStatus.textContent = "This title is not supported."; }
  });
  newButton.addEventListener("click", () => void (async () => {
    if (!universeToken || pendingCreate || transitioning) return;
    transitioning = true; paint();
    if (!(await mayLeave())) { transitioning = false; paint(); return; }
    const token = universeToken, id = crypto.randomUUID();
    loading = true; paint();
    try {
      const page = await invokeCommand<PageContent>("create_page", {universeToken: token, pageId: id});
      if (universeToken !== token || page.id !== id) throw new Error("Creation identity mismatch");
      pages.push({id: page.id, title: page.title}); renderPages();
      loading = false; await selectPage(id, false);
    } catch {
      pendingCreate = {token, id};
      status.textContent = "Creation could not be confirmed. Verify the page list before creating another page.";
    } finally { loading = false; transitioning = false; paint(); }
  })());
  verifyCreate.addEventListener("click", () => void (async () => {
    if (!pendingCreate || transitioning) return;
    const pending = pendingCreate; transitioning = true; paint();
    try {
      const index = await invokeCommand<PageIndexEntry[]>("page_index", {universeToken: pending.token});
      if (universeToken !== pending.token || pendingCreate !== pending) return;
      pages = index; renderPages(); pendingCreate = null;
      status.textContent = index.some(p => p.id === pending.id) ? "The new page exists. Select it when ready." : "The new page was not found in the refreshed list.";
    } catch { status.textContent = "Verification failed. Your draft is preserved; try verification again."; }
    finally { transitioning = false; paint(); }
  })());

  form.addEventListener("submit", async event => {
    event.preventDefault();
    if (transitioning) return;
    transitioning = true;
    paint();
    if (!(await mayLeave())) { transitioning = false; paint(); return; }
    const path = input.value;
    loading = true; ++version;
    try {
      const opened = await invokeCommand<OpenedUniverse>("open_universe", {databasePath: path});
      universeToken = opened.token; pendingCreate = null; pages = opened.pages;
      selectedPageId = null;
      clearPage(); renderPages();
      name.textContent = path.split(/[\\/]/).at(-1) || path;
      if (pages.length) { loading = false; await selectPage(pages[0]!.id, false); }
      else { title.hidden = false; titleInput.hidden = true; title.textContent = "No page selected"; body.textContent = "This universe has no pages."; }
    } catch { status.textContent = "The universe could not be opened."; }
    finally { loading = false; transitioning = false; paint(); }
  });
  saveButton.addEventListener("click", () => void save());
  verifyButton.addEventListener("click", () => void verifyBusy());
  document.addEventListener("keydown", event => { if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") { event.preventDefault(); void save(); } });
  return {requestClose: (token: string) => { void (async () => {
    if (transitioning) { await invokeCommand("cancel_close", {token}); return; }
    transitioning = true;
    paint();
    try {
      if (await mayLeave()) await invokeCommand("close_window", {token});
      else await invokeCommand("cancel_close", {token});
    } finally { transitioning = false; paint(); }
  })(); }};
}
