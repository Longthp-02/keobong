import type { Metadata } from "next";
import { signInUrl } from "../../lib/api";
import messages from "../../messages/vi.json";

const t = messages.auth;

export const metadata: Metadata = { title: t.failedTitle, robots: { index: false } };

/** Where the API sends the browser when a Google sign-in cannot be completed. */
export default function LoginFailedPage() {
  return (
    <section className="intro">
      <h1 className="intro__title">{t.failedTitle}</h1>
      <p className="muted">{t.failedBody}</p>
      <a href={signInUrl("/create")} className="button-primary">
        {t.tryAgain}
      </a>
      <p>
        <a href="/">{t.backHome}</a>
      </p>
    </section>
  );
}
