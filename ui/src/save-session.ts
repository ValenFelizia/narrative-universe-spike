import { ChangeSet, Text } from "@codemirror/state";

import {
  AdapterError,
  assertEditableText,
  composeTransactions,
  toSequentialEdits,
} from "./adapter.ts";
import {
  applyMentionOp,
  applyTransaction,
  canonicalMentions,
  linkOp,
  MentionError,
  replaceOp,
  sameMentions,
  validateMentions,
  type AnnotatedBody,
  type Mention,
  type MentionEffect,
  type MentionOp,
} from "./mentions.ts";

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
  ops: MentionOp[];
  mentions: Mention[];
};

export type SaveReply =
  | {
      status: "ok";
      requestId: number;
      sessionId: string;
      pageId: string;
      doc: string;
      title?: string;
      mentions: readonly Mention[];
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
  sentMentions: readonly Mention[];
  ledgerCount: number;
};

/// One ordered unit of unsaved work. A text entry keeps the ids the mirrored
/// reducer predicts it removed, so a later notice can name them.
export type LedgerEntry =
  | { kind: "text"; changes: ChangeSet; removed: readonly string[] }
  | { kind: "link"; mention: Mention }
  | { kind: "unlink"; mentionId: string };

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
  baseMentions: readonly Mention[];
  draftMentions: readonly Mention[];
  /// Every id this session has seen, with its entity. An id is never
  /// repurposed for another entity while the document is open.
  bound: ReadonlyMap<string, string>;
  ledger: readonly LedgerEntry[];
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
  mentions: readonly Mention[];
}): SaveState {
  if (!isSupportedRevision(options.revision)) {
    throw new UnsupportedRevisionError(options.revision);
  }
  // A stored link this client cannot validate fails closed here, before any
  // edit can be predicted from it.
  validateMentions({ body: options.doc, mentions: options.mentions });
  const mentions = canonicalMentions(options.mentions);
  return {
    sessionId: options.sessionId,
    pageId: options.pageId,
    mode: mentions.length > 0 ? "ledger" : "net",
    baseRevision: options.revision,
    baseTitle: options.title ?? "",
    draftTitle: options.title ?? "",
    titleAware: options.title !== undefined,
    baseDoc: options.doc,
    draftDoc: options.doc,
    baseMentions: mentions,
    draftMentions: mentions,
    bound: new Map(mentions.map((held) => [held.id, held.entityId])),
    ledger: [],
    inFlight: null,
    lastAttempt: null,
    nextRequestId: 1,
    status: "clean",
    detail: null,
  };
}

/// Records one editor transaction: its text changes first, then its link
/// effects, in order. The mention set is predicted with the mirrored reducer
/// over the same sequential edits the bridge will receive.
export function recordEdit(
  state: SaveState,
  changes: ChangeSet,
  effects: readonly MentionEffect[] = [],
): SaveState {
  if (changes.empty && effects.length === 0) {
    return state;
  }
  // An id stays bound to its first entity for the whole open document,
  // across saves, unlinks, and history. The reducer alone cannot see this
  // once the id has been dropped.
  let bound = state.bound;
  for (const effect of effects) {
    if (effect.kind !== "link") continue;
    const entity = bound.get(effect.mention.id);
    if (entity !== undefined && entity !== effect.mention.entityId) {
      throw new MentionError("mention_id_conflict", effect.mention.id);
    }
    if (entity === undefined) bound = new Map(bound).set(effect.mention.id, effect.mention.entityId);
  }
  // Net mode composes before it translates, so a transaction that would be
  // rejected on its own could otherwise be cancelled by a later one and
  // never seen by the adapter. The prediction translates the transaction
  // against the live draft now, which also rejects split scalar boundaries
  // and invalid inserted text, before any draft is allocated.
  const result = applyTransaction(state.draftDoc, state.draftMentions, changes, effects);
  const entries: LedgerEntry[] = [];
  if (!changes.empty) entries.push({ kind: "text", changes, removed: result.removed });
  for (const effect of result.applied) {
    entries.push(effect.kind === "link"
      ? { kind: "link", mention: { ...effect.mention } }
      : { kind: "unlink", mentionId: effect.mentionId });
  }
  if (entries.length === 0) {
    return state;
  }
  const status: SessionStatus = state.inFlight
    ? "saving"
    : state.status === "busy" ||
        state.status === "stale" ||
        state.status === "error"
      ? state.status
      : "dirty";
  return {
    ...state,
    mode: result.applied.length > 0 ? "ledger" : state.mode,
    draftDoc: result.doc,
    draftMentions: result.mentions,
    bound,
    ledger: [...state.ledger, ...entries],
    status,
    detail: state.inFlight ? null : state.detail,
  };
}

/// Links one occurrence as its own ledger entry.
export function recordLink(state: SaveState, mention: Mention): SaveState {
  return recordEdit(state, ChangeSet.empty(state.draftDoc.length), [{ kind: "link", mention }]);
}

/// Unlinks one occurrence as its own ledger entry. Unlinking an id the
/// draft does not hold records nothing.
export function recordUnlink(state: SaveState, mentionId: string): SaveState {
  return recordEdit(state, ChangeSet.empty(state.draftDoc.length), [{ kind: "unlink", mentionId }]);
}

/// Ids that unsaved text edits removed and that the draft no longer holds.
/// Undo or a deliberate relink clears an id; a confirmed save clears every
/// id it carried.
export function pendingDrops(state: SaveState): string[] {
  const live = new Set(state.draftMentions.map((held) => held.id));
  const dropped = new Set<string>();
  for (const entry of state.ledger) {
    if (entry.kind === "text") {
      for (const id of entry.removed) if (!live.has(id)) dropped.add(id);
    }
  }
  return [...dropped].sort();
}

/// A mention-free session sends one composed diff. Mode turns to ledger on
/// the first link or unlink, so every entry here is text.
function netOps(state: SaveState): MentionOp[] {
  const changes = state.ledger.map((entry) => {
    if (entry.kind !== "text") {
      throw new AdapterError("inconsistent_changeset", "a net session holds a link entry");
    }
    return entry.changes;
  });
  return toSequentialEdits(composeTransactions(changes, state.baseDoc.length), state.baseDoc).map(replaceOp);
}

function ledgerOps(baseDoc: string, ledger: readonly LedgerEntry[]): MentionOp[] {
  const ops: MentionOp[] = [];
  let text = baseDoc;
  for (const entry of ledger) {
    if (entry.kind === "text") {
      ops.push(...toSequentialEdits(entry.changes, text).map(replaceOp));
      text = docAfter(text, entry.changes);
    } else if (entry.kind === "link") {
      ops.push(linkOp(entry.mention));
    } else {
      ops.push({ kind: "unlink", mentionId: entry.mentionId });
    }
  }
  return ops;
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
  const ops = state.mode === "net" ? netOps(state) : ledgerOps(state.baseDoc, state.ledger);
  // Replay the exact request through the mirror before it leaves. A session
  // whose own prediction disagrees with its draft never sends.
  let replay: AnnotatedBody = { body: state.baseDoc, mentions: state.baseMentions };
  for (const op of ops) replay = applyMentionOp(replay, op);
  if (replay.body !== state.draftDoc || !sameMentions(replay.mentions, state.draftMentions)) {
    throw new AdapterError("inconsistent_changeset", "the save request does not reproduce the draft");
  }
  const request: SaveRequest = {
    requestId: state.nextRequestId,
    sessionId: state.sessionId,
    pageId: state.pageId,
    expectedRevision: state.baseRevision,
    doc: state.draftDoc,
    title: state.draftTitle,
    titleChanged: state.draftTitle !== state.baseTitle,
    ops,
    mentions: canonicalMentions(state.draftMentions),
  };
  const inFlight: InFlight = {
    requestId: request.requestId,
    expectedRevision: state.baseRevision,
    sentDoc: state.draftDoc,
    sentTitle: state.draftTitle,
    sentMentions: canonicalMentions(state.draftMentions),
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
      (state.titleAware ? reply.title === attempt.sentTitle : (reply.title ?? "") === attempt.sentTitle) &&
      Array.isArray(reply.mentions) &&
      sameMentions(reply.mentions, attempt.sentMentions);
    // A changed aggregate always advances the revision by exactly one. An
    // unchanged revision is only trustworthy when the whole sent aggregate,
    // mentions included, equals the base: then the store provably wrote
    // nothing. A body that returns to the base can still have lost a link
    // (R9), which the mention comparison catches.
    const expectedNext = attempt.expectedRevision + 1;
    const acceptedRevision =
      reply.revision === expectedNext ||
      (reply.revision === attempt.expectedRevision &&
        attempt.sentDoc === state.baseDoc &&
        attempt.sentTitle === state.baseTitle &&
        sameMentions(attempt.sentMentions, state.baseMentions));
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
        baseMentions: attempt.sentMentions,
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
/// prove that only when the page still holds the base body, title, and
/// mention set at the revision this session started from. It never proves that the request committed:
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
    mentions: readonly Mention[];
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
    (state.titleAware ? stored.title === state.baseTitle : (stored.title ?? "") === state.baseTitle) &&
    Array.isArray(stored.mentions) &&
    sameMentions(stored.mentions, state.baseMentions);
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
