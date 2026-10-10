"use client";

import { useState } from "react";
import messages from "../messages/vi.json";

const t = messages.slots;

type Props = { shareId: string; title: string };

/** Shares the match link with the system share sheet (Zalo, Messenger…), or copies it. */
export function ShareButton({ shareId, title }: Props) {
  const [note, setNote] = useState<string | null>(null);

  async function share() {
    const url = `${window.location.origin}/m/${encodeURIComponent(shareId)}`;
    setNote(null);
    if (typeof navigator.share === "function") {
      try {
        await navigator.share({ title, text: t.shareText, url });
        return;
      } catch (error) {
        // The visitor closed the share sheet: nothing to do.
        if (error instanceof DOMException && error.name === "AbortError") {
          return;
        }
        // Otherwise fall back to copying the link.
      }
    }
    try {
      await navigator.clipboard.writeText(url);
      setNote(t.linkCopied);
    } catch {
      setNote(t.shareFailed);
    }
  }

  return (
    <>
      <button type="button" className="button-share" onClick={share}>
        <svg
          width="20"
          height="20"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <circle cx="18" cy="5" r="2.5" />
          <circle cx="6" cy="12" r="2.5" />
          <circle cx="18" cy="19" r="2.5" />
          <path d="M8.2 10.8l7.6-4.4M8.2 13.2l7.6 4.4" />
        </svg>
        {t.share}
      </button>
      {/* Always present, so screen readers announce the text when it appears. */}
      <span className="action-bar__note" role="status">
        {note ?? ""}
      </span>
    </>
  );
}
