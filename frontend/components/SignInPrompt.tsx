import { signInUrl } from "../lib/api";
import messages from "../messages/vi.json";

const t = messages.auth;

/** Asks a signed-out visitor to sign in before an action that needs an account. */
export function SignInPrompt({ next, title = t.signInTitle }: { next: string; title?: string }) {
  return (
    <section className="sign-in">
      <h1 className="create-form__title">{title}</h1>
      <p className="muted">{t.signInBody}</p>
      {/* A plain link: sign-in is a full-page redirect to Google, not a client navigation. */}
      <a href={signInUrl(next)} className="button-primary">
        {t.signInWithGoogle}
      </a>
    </section>
  );
}
