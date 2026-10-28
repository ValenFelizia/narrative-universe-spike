import type { invoke } from "@tauri-apps/api/core";
import { Compartment, EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { decideEditing } from "../src/adapter.ts";
import { applyReply, beginSave, isDirty, openSession, reconcileBusy, recordEdit, MAX_SUPPORTED_REVISION, type SaveState } from "../src/save-session.ts";

type PageIndexEntry = { id: string; title: string };
type OpenedUniverse = { token: string; pages: PageIndexEntry[] };
type PageContent = { id: string; title: string; body: string; revision: number | null; mentionCount: number };
type SaveResponse = { pageId: string; sessionId: string; requestId: number; body: string; revision: number };

export function initDesktop(invokeCommand: typeof invoke): { requestClose: (token: string) => void } {
  const form = document.querySelector<HTMLFormElement>("#open-form")!;
  const input = document.querySelector<HTMLInputElement>("#database-path")!;
  const name = document.querySelector<HTMLElement>("#universe-name")!;
  const count = document.querySelector<HTMLElement>("#page-count")!;
  const list = document.querySelector<HTMLUListElement>("#page-list")!;
  const status = document.querySelector<HTMLElement>("#status")!;
  const title = document.querySelector<HTMLElement>("#page-title")!;
  const pane = document.querySelector<HTMLElement>(".reading-area")!;
  const body = document.querySelector<HTMLElement>("#page-body")!;
  const saveButton = document.querySelector<HTMLButtonElement>("#save")!;
  const saveStatus = document.querySelector<HTMLElement>("#save-status")!;
  const dialog = document.querySelector<HTMLDialogElement>("#guard")!;
  const keep = document.querySelector<HTMLButtonElement>("#continue")!;
  const saveProceed = document.querySelector<HTMLButtonElement>("#save-proceed")!;
  const discard = document.querySelector<HTMLButtonElement>("#discard")!;
  let universeToken = "";
  let session: SaveState | null = null;
  let editor: EditorView | null = null;
  const editability = new Compartment();
  let version = 0;
  let loading = false;
  let deciding = false;
  let transitioning = false;
  let saving: Promise<boolean> | null = null;
  let uncertain = false;

  function paint(): void {
    if (!session) { saveButton.disabled = true; return; }
    const labels = {clean: "Saved", dirty: "Unsaved", saving: "Saving…", stale: "Changed elsewhere. Copy your draft before leaving.", busy: "Database busy. Your draft is preserved.", error: "Save could not be confirmed. Your draft is preserved."};
    saveStatus.textContent = session.baseRevision >= MAX_SUPPORTED_REVISION ? "Revision limit reached. Copy any unsaved draft; this page is read-only." : labels[session.status];
    saveButton.disabled = session.baseRevision >= MAX_SUPPORTED_REVISION || !isDirty(session) || session.inFlight !== null || ["stale", "error", "busy"].includes(session.status);
  }

  async function save(): Promise<boolean> {
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
        const response = await invokeCommand<SaveResponse>("save_page", { request: { universeToken, pageId: request.pageId, sessionId: request.sessionId, requestId: request.requestId, expectedRevision: request.expectedRevision, edits: request.edits } });
        if (!session || session.sessionId !== request.sessionId) return false;
        const outcome = applyReply(session, {status: "ok", requestId: response.requestId, sessionId: response.sessionId, pageId: response.pageId, doc: response.body, revision: response.revision});
        session = outcome.ignored ? {...outcome.state, status: "error", detail: "Save acknowledgement identity did not match."} : outcome.state;
        uncertain = !!outcome.ignored || !outcome.applied;
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
              session = reconcileBusy(session, {sessionId: request.sessionId, pageId: page.id, doc: page.body, revision: page.revision});
              uncertain = session.status !== "dirty";
            }
          } catch { /* Keep the busy state and draft. */ }
        }
      }
      if (session && session.baseRevision >= MAX_SUPPORTED_REVISION && editor) editor.dispatch({effects: editability.reconfigure([EditorState.readOnly.of(true), EditorView.editable.of(false)])});
      paint();
      return !!session && session.status === "clean" && !isDirty(session);
    })();
    try { return await saving; } finally { saving = null; }
  }

  async function mayLeave(): Promise<boolean> {
    if (deciding || loading || saving) return false;
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
    if (choice === "save") result = await save();
    deciding = false;
    return result;
  }

  function clearPage(): void { editor?.destroy(); editor = null; session = null; uncertain = false; body.replaceChildren(); saveStatus.textContent = ""; paint(); }

  async function selectPage(id: string, guarded = true): Promise<void> {
    if (guarded) {
      if (transitioning) return;
      transitioning = true;
      if (!(await mayLeave())) { transitioning = false; return; }
    }
    const requestVersion = ++version;
    loading = true;
    try {
      const page = await invokeCommand<PageContent>("read_page", {pageId: id, universeToken});
      if (requestVersion !== version) return;
      clearPage();
      title.textContent = page.title;
      pane.scrollTop = 0;
      for (const button of list.querySelectorAll<HTMLButtonElement>("button")) button.setAttribute("aria-current", String(button.dataset.pageId === id));
      const decision = decideEditing(page.body, page.mentionCount);
      if (!decision.editable || page.revision === null || page.revision >= MAX_SUPPORTED_REVISION) {
        body.textContent = page.body;
        saveStatus.textContent = !decision.editable ? ({linked:"This page contains links and is read-only.", carriage_return:"This page contains carriage returns and is read-only.", nul:"This page contains NUL and is read-only.", unpaired_surrogate:"This page contains unsupported text and is read-only."})[decision.reason] : "This page revision is not editable.";
      } else {
        session = openSession({sessionId: crypto.randomUUID(), pageId: page.id, doc: page.body, revision: page.revision, hasMentions: false});
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
    finally { if (requestVersion === version) loading = false; if (guarded) transitioning = false; }
  }

  form.addEventListener("submit", async event => {
    event.preventDefault();
    if (transitioning) return;
    transitioning = true;
    if (!(await mayLeave())) { transitioning = false; return; }
    const path = input.value;
    loading = true; ++version;
    try {
      const {pages, token} = await invokeCommand<OpenedUniverse>("open_universe", {databasePath: path});
      universeToken = token;
      clearPage(); list.replaceChildren();
      name.textContent = path.split(/[\\/]/).at(-1) || path;
      count.textContent = String(pages.length);
      for (const page of pages) {
        const li = document.createElement("li"), button = document.createElement("button");
        button.type = "button"; button.className = "page-link"; button.textContent = page.title; button.dataset.pageId = page.id;
        button.addEventListener("click", () => void selectPage(page.id)); li.append(button); list.append(li);
      }
      if (pages.length) { loading = false; await selectPage(pages[0]!.id, false); }
      else { title.textContent = "No page selected"; body.textContent = "This universe has no pages."; }
    } catch { status.textContent = "The universe could not be opened."; }
    finally { loading = false; transitioning = false; }
  });
  saveButton.addEventListener("click", () => void save());
  document.addEventListener("keydown", event => { if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") { event.preventDefault(); void save(); } });
  return {requestClose: (token: string) => { void (async () => {
    if (transitioning) { await invokeCommand("cancel_close", {token}); return; }
    transitioning = true;
    try {
      if (await mayLeave()) await invokeCommand("close_window", {token});
      else await invokeCommand("cancel_close", {token});
    } finally { transitioning = false; }
  })(); }};
}
