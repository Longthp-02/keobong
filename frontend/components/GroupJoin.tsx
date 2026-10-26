"use client";

import { useState } from "react";
import { type GroupMembership, type JoinGroupResult, browserJoinGroup, signInUrl } from "../lib/api";
import messages from "../messages/vi.json";

const t = messages.groups.join;

type Props = {
  groupId: string;
  /** `null` when signed out. */
  initial: GroupMembership | null;
  join?: (groupId: string) => Promise<JoinGroupResult>;
};

/** "DD/MM" in Ho Chi Minh City time. */
function localDate(iso: string): string {
  const local = new Date(new Date(iso).getTime() + 7 * 60 * 60 * 1000);
  const day = String(local.getUTCDate()).padStart(2, "0");
  const month = String(local.getUTCMonth() + 1).padStart(2, "0");
  return `${day}/${month}`;
}

/** Where the viewer stands with a group: sign in, ask to join, wait, or open it. */
export function GroupJoin({ groupId, initial, join = browserJoinGroup }: Props) {
  const [membership, setMembership] = useState(initial);
  const [state, setState] = useState<"idle" | "asking" | "sent" | Exclude<JoinGroupResult, { ok: true }>["error"]>(
    "idle",
  );

  async function ask() {
    setState("asking");
    const result = await join(groupId);
    if (result.ok) {
      setMembership(result.membership);
      // Announced in the live region, since the button that had focus is gone.
      setState("sent");
    } else {
      setState(result.error);
    }
  }

  const status = state === "idle" || state === "asking" ? "" : state === "sent" ? t.sent : t.errors[state];

  return (
    <section className="group-join" aria-labelledby="group-join-title">
      <h2 id="group-join-title" className="poll__question">
        {t.title}
      </h2>
      {membership === null ? (
        <div className="sign-in">
          <p className="muted">{t.signInFirst}</p>
          {/* A plain link: sign-in is a full-page redirect, not a client navigation. */}
          <a href={signInUrl(`/g/${groupId}`)} className="button-primary">
            {messages.auth.signInWithGoogle}
          </a>
        </div>
      ) : membership.status === "none" ||
        (membership.status === "declined" && Date.now() >= Date.parse(membership.askAgainAt)) ? (
        <button type="button" className="button-primary" onClick={ask} disabled={state === "asking"}>
          {state === "asking" ? t.asking : t.ask}
        </button>
      ) : membership.status === "pending" ? (
        <p>{t.pending}</p>
      ) : membership.status === "declined" ? (
        <p>{t.declined.replace("{date}", localDate(membership.askAgainAt))}</p>
      ) : (
        <div className="group-join__member">
          <p>{t.member}</p>
          {membership.pollPath ? (
            <a href={membership.pollPath} className="button-primary">
              {t.openPoll}
            </a>
          ) : null}
          {membership.groupLink ? (
            <a href={membership.groupLink} className="button-secondary" rel="noopener noreferrer" target="_blank">
              {t.openZalo}
            </a>
          ) : (
            <p className="muted">{t.noZaloLink}</p>
          )}
        </div>
      )}
      <p role="status" aria-live="polite">
        {status}
      </p>
      {state === "signed_out" ? (
        <a href={signInUrl(`/g/${groupId}`)} className="button-secondary">
          {messages.auth.signInWithGoogle}
        </a>
      ) : null}
    </section>
  );
}
