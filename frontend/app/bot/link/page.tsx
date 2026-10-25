import type { Metadata } from "next";
import { cookies } from "next/headers";
import { notFound } from "next/navigation";
import { BotLinkForm } from "../../../components/BotLinkForm";
import { SignInPrompt } from "../../../components/SignInPrompt";
import { SESSION_COOKIE, getMe, isApiConfigured, sessionCookieHeader } from "../../../lib/api";
import messages from "../../../messages/vi.json";

const t = messages.botLink;

export const metadata: Metadata = { title: t.title, robots: { index: false } };

type Props = { searchParams: Promise<{ code?: string }> };

/** Codes from the bot are 32 random bytes in URL-safe base64 (43 characters). */
const CODE = /^[A-Za-z0-9_-]{43}$/;

/** Where Bot DaGhep's one-time link lands: sign in, then confirm linking Zalo to this account. */
export default async function BotLinkPage({ searchParams }: Props) {
  if (!isApiConfigured()) {
    notFound();
  }
  const code = (await searchParams).code ?? "";
  if (!CODE.test(code)) {
    return (
      <section className="bot-link">
        <h1 className="create-form__title">{t.title}</h1>
        <p role="status">{t.invalid}</p>
      </section>
    );
  }
  const me = await getMe(sessionCookieHeader((await cookies()).get(SESSION_COOKIE)?.value));
  if (!me) {
    return <SignInPrompt next={`/bot/link?code=${code}`} title={t.signInTitle} />;
  }
  return <BotLinkForm code={code} accountName={me.displayName} />;
}
