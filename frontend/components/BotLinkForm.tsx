"use client";

import { useState } from "react";
import { type LinkZaloResult, browserLinkZalo } from "../lib/api";
import messages from "../messages/vi.json";

const t = messages.botLink;

type Props = {
  /** One-time code from Bot DaGhep's private message. */
  code: string;
  accountName: string | null;
  link?: (code: string) => Promise<LinkZaloResult>;
};

/** Confirms which Đá Ghép account the Zalo account is linked to. Linking needs a click, never a page load. */
export function BotLinkForm({ code, accountName, link = (value) => browserLinkZalo(value) }: Props) {
  const [state, setState] = useState<"idle" | "linking" | LinkZaloResult>("idle");

  async function submit() {
    setState("linking");
    setState(await link(code));
  }

  const status =
    state === "linked"
      ? t.done
      : state === "invalid"
        ? t.invalid
        : state === "failed"
          ? t.failed
          : state === "signed_out"
            ? messages.create.errors.unauthenticated
            : "";

  return (
    <section className="bot-link">
      <h1 className="create-form__title">{t.title}</h1>
      <p className="muted">{t.intro}</p>
      <p>
        {t.account} <strong>{accountName ?? messages.auth.anonymousName}</strong>
      </p>
      {state === "linked" || state === "invalid" ? null : (
        <button type="button" className="button-primary" onClick={submit} disabled={state === "linking"}>
          {state === "linking" ? t.linking : t.submit}
        </button>
      )}
      <p role="status" aria-live="polite">
        {status}
      </p>
    </section>
  );
}
