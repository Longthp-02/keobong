import type { MeView } from "../lib/api";
import messages from "../messages/vi.json";

const t = messages.auth;

/** Shows who is signed in, with a sign-out button (a POST, so links cannot sign people out). */
export function AccountBar({ me }: { me: MeView }) {
  return (
    <div className="account-bar">
      <span className="muted">{t.signedInAs}</span> <strong>{me.displayName ?? t.anonymousName}</strong>
      <a href="/account/payout" className="account-bar__link">
        {t.payoutLink}
      </a>
      <form method="post" action="/api/auth/logout">
        <button type="submit" className="link-button">
          {t.signOut}
        </button>
      </form>
    </div>
  );
}
