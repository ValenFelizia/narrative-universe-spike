import { ChangeSet, Text } from "@codemirror/state";

import {
  assertEditableText,
  composeTransactions,
  toSequentialEdits,
  toSequentialEditsFromTransactions,
  type PageEdit,
} from "./adapter.ts";

export type SessionMode = "net" | "ledger";

/// Revisions are JavaScript numbers on this side of the bridge. The open
/// revision may be `MAX_SAFE_INTEGER`; a request that would advance beyond it
/// is refused instead of committing a token the client cannot represent.
export const MAX_SUPPORTED_REVISION = Number.MAX_SAFE_INTEGER;

export class UnsupportedRevisionError extends RangeError {
  constructor(revision: number) {
    super(
      `revision ${revision} is outside the supported range 0..=${MAX_SUPPORTED_REVISION}`,
    );
    this.name = "UnsupportedRevisionError";
  }
}

function isSupportedRevision(revision: number): boolean {
  return (
    Number.isSafeInteger(revision) &&
    revision >= 0 &&
    revision <= MAX_SUPPORTED_REVISION
  );
}

export type SessionStatus =
  | "clean"
  | "dirty"
  | "saving"
  | "stale"
  | "busy"
  | "error";

export type SaveRequest = {
  requestId: number;
  sessionId: string;
  pageId: string;
  expectedRevision: number;
  doc: string;
  title: string;
  titleChanged: boolean;
  edits: PageEdit[];
};

export type SaveReply =
  | {
      status: "ok";
      requestId: number;
      sessionId: string;
      pageId: string;
      doc: string;
      title?: string;
      revision: number;
    }
  | {
      status: "stale" | "busy" | "error";
      requestId: number;
      sessionId: string;
      pageId: string;
      detail: string;
    };

export type InFlight = {
  requestId: number;
  expectedRevision: number;
  sentDoc: string;
  sentTitle: string;
  ledgerCount: number;
};

export type SaveState = {
  sessionId: string;
  pageId: string;
  mode: SessionMode;
  baseRevision: number;
  baseTitle: string;
  draftTitle: string;
  titleAware: boolean;
  baseDoc: string;
  draftDoc: string;
  ledger: readonly ChangeSet[];
  inFlight: InFlight | null;
  lastAttempt: InFlight | null;
  nextRequestId: number;
  status: SessionStatus;
  detail: string | null;
};

function docAfter(text: string, changes: ChangeSet): string {
  return changes.apply(Text.of(text.split("\n"))).toString();
}

export function openSession(options: {
  sessionId: string;
  pageId: string;
  doc: string;
  title?: string;
  revision: number;
  hasMentions: boolean;
}): SaveState {
  if (!isSupportedRevision(options.revision)) {
    throw new UnsupportedRevisionError(options.revision);
  }
  return {
    sessionId: options.sessionId,
    pageId: options.pageId,
    mode: options.hasMentions ? "ledger" : "net",
    baseRevision: options.revision,
    baseTitle: options.title ?? "",
    draftTitle: options.title ?? "",
    titleAware: options.title !== undefined,
    baseDoc: options.doc,
    draftDoc: options.doc,
    ledger: [],
    inFlight: null,
    lastAttempt: null,
    nextRequestId: 1,
    status: "clean",
    detail: null,
  };
}

export function recordEdit(
  state: SaveState,
  changes: ChangeSet,
): SaveState {
  if (changes.empty) {
    return state;
  }
  // Net mode composes before it translates, so a transaction that would be
  // rejected on its own could otherwise be cancelled by a later one and never
  // seen by the adapter. Translate the transaction against the live draft now,
  // which also rejects split scalar boundaries and invalid inserted text. Do
  // this before applying the changes so a rejected transaction never allocates
  // a draft.
  toSequentialEdits(changes, state.draftDoc);
  const draftDoc = docAfter(state.draftDoc, changes);
  const status: SessionStatus = state.inFlight
    ? "saving"
    : state.status === "busy" ||
        state.status === "stale" ||
        state.status === "error"
      ? state.status
      : "dirty";
  return {
    ...state,
    draftDoc,
    ledger: [...state.ledger, changes],
    status,
    detail: state.inFlight ? null : state.detail,
  };
}

export function recordTitleEdit(state: SaveState, title: string): SaveState {
  assertEditableText(title);
  if (title === state.draftTitle) return state;
  const status: SessionStatus = state.inFlight ? "saving"
    : state.status === "busy" || state.status === "stale" || state.status === "error"
      ? state.status : (title !== state.baseTitle || state.ledger.length > 0 ? "dirty" : "clean");
  return { ...state, titleAware: true, draftTitle: title, status, detail: state.inFlight ? null : state.detail };
}

export type BeginSave =
  | { ok: true; state: SaveState; request: SaveRequest }
  | {
      ok: false;
      reason: "in_flight" | "busy_pending" | "revision_exhausted" | "no_changes";
    };

export function beginSave(state: SaveState): BeginSave {
  if (state.inFlight) {
    return { ok: false, reason: "in_flight" };
  }
  // A busy save may or may not have committed. Sending another one first
  // would either replay an applied ledger or race the stale base revision.
  if (state.status === "busy" && state.lastAttempt !== null) {
    return { ok: false, reason: "busy_pending" };
  }
  if (state.ledger.length === 0 && state.draftTitle === state.baseTitle) {
    return { ok: false, reason: "no_changes" };
  }
  // A changed save must be able to name its new revision. Refuse before
  // dispatch rather than commit a token the client cannot represent.
  if (state.baseRevision === MAX_SUPPORTED_REVISION) {
    return { ok: false, reason: "revision_exhausted" };
  }
  const edits =
    state.mode === "net"
      ? toSequentialEdits(
          composeTransactions(state.ledger, state.baseDoc.length),
          state.baseDoc,
        )
      : toSequentialEditsFromTransactions(state.baseDoc, state.ledger);
  const request: SaveRequest = {
    requestId: state.nextRequestId,
    sessionId: state.sessionId,
    pageId: state.pageId,
    expectedRevision: state.baseRevision,
    doc: state.draftDoc,
    title: state.draftTitle,
    titleChanged: state.draftTitle !== state.baseTitle,
    edits,
  };
  const inFlight: InFlight = {
    requestId: request.requestId,
    expectedRevision: state.baseRevision,
    sentDoc: state.draftDoc,
    sentTitle: state.draftTitle,
    ledgerCount: state.ledger.length,
  };
  return {
    ok: true,
    state: {
      ...state,
      inFlight,
      lastAttempt: inFlight,
      nextRequestId: state.nextRequestId + 1,
      status: "saving",
      detail: null,
    },
    request,
  };
}

export type ReplyOutcome = {
  state: SaveState;
  applied: boolean;
  ignored: "no_in_flight" | "other_request" | "other_session" | null;
};

/// Accepts a reply only when it belongs to the active document and to the
/// request that is actually in flight. The live draft is irrelevant to
/// acceptance: a valid acknowledgement still advances the base revision
/// while typing continues.
export function applyReply(state: SaveState, reply: SaveReply): ReplyOutcome {
  if (!state.inFlight) {
    return { state, applied: false, ignored: "no_in_flight" };
  }
  if (reply.requestId !== state.inFlight.requestId) {
    return { state, applied: false, ignored: "other_request" };
  }
  if (
    reply.sessionId !== state.sessionId ||
    reply.pageId !== state.pageId
  ) {
    return { state, applied: false, ignored: "other_session" };
  }

  if (reply.status === "ok") {
    const attempt = state.inFlight;
    const sameDocument = reply.doc === attempt.sentDoc &&
      (state.titleAware ? reply.title === attempt.sentTitle : (reply.title ?? "") === attempt.sentTitle);
    // A changed aggregate always advances the revision by exactly one. An
    // unchanged revision is only trustworthy for a mention-free net request
    // whose text did not change: then a no-op is provably aggregate-neutral.
    // Ledger-mode no-ops can change mentions while keeping the body, so they
    // are not accepted from an unchanged revision.
    const expectedNext = attempt.expectedRevision + 1;
    const acceptedRevision =
      reply.revision === expectedNext ||
      (state.mode === "net" &&
        reply.revision === attempt.expectedRevision &&
        attempt.sentDoc === state.baseDoc &&
        attempt.sentTitle === state.baseTitle);
    if (!sameDocument || !isSupportedRevision(reply.revision) || !acceptedRevision) {
      return {
        state: {
          ...state,
          inFlight: null,
          status: "error",
          detail: "acknowledgement does not match the saved snapshot",
        },
        applied: false,
        ignored: null,
      };
    }
    const ledger = state.ledger.slice(attempt.ledgerCount);
    return {
      state: {
        ...state,
        baseDoc: attempt.sentDoc,
        baseTitle: attempt.sentTitle,
        baseRevision: reply.revision,
        ledger,
        inFlight: null,
        lastAttempt: null,
        status: ledger.length > 0 || state.draftTitle !== attempt.sentTitle ? "dirty" : "clean",
        detail: null,
      },
      applied: true,
      ignored: null,
    };
  }

  // A stale or busy reply never rebases and never reloads. The draft, base,
  // and ledger stay exactly as they were.
  return {
    state: {
      ...state,
      inFlight: null,
      status: reply.status,
      detail: reply.detail,
    },
    applied: false,
    ignored: null,
  };
}

/// A Busy reply means the write transaction did not commit. A reread can
/// prove that only when the page still holds the base body at the revision
/// this session started from. It never proves that the request committed:
/// another writer's commit can advance the revision while leaving the body
/// identical, which would silently discard an R9 mention removal. So a busy
/// save either becomes retryable with its whole ledger intact, or stays an
/// error; it is never reported clean.
export function reconcileBusy(
  state: SaveState,
  stored: {
    sessionId: string;
    pageId: string;
    doc: string;
    title?: string;
    revision: number;
  },
): SaveState {
  const attempt = state.lastAttempt;
  if (state.status !== "busy" || attempt === null) {
    return state;
  }
  if (
    stored.sessionId !== state.sessionId ||
    stored.pageId !== state.pageId
  ) {
    return state;
  }
  const unchanged =
    stored.revision === attempt.expectedRevision &&
    stored.doc === state.baseDoc &&
    (state.titleAware ? stored.title === state.baseTitle : (stored.title ?? "") === state.baseTitle);
  if (!unchanged) {
    return {
      ...state,
      lastAttempt: null,
      status: "error",
      detail: "busy outcome could not be reconciled",
    };
  }
  return {
    ...state,
    lastAttempt: null,
    status: "dirty",
    detail: null,
  };
}

/// True while the editor holds work the store has not accepted.
export function isDirty(state: SaveState): boolean {
  return state.draftTitle !== state.baseTitle || state.draftDoc !== state.baseDoc || state.ledger.length > 0;
}
