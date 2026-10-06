import type { Metadata } from "next";
import { cookies } from "next/headers";
import { notFound } from "next/navigation";
import { AccountBar } from "../../../components/AccountBar";
import { PayoutForm } from "../../../components/PayoutForm";
import { SignInPrompt } from "../../../components/SignInPrompt";
import { SESSION_COOKIE, getBanks, getMe, getPayout, isApiConfigured, sessionCookieHeader } from "../../../lib/api";
import messages from "../../../messages/vi.json";

const t = messages.payout;

export const metadata: Metadata = { title: t.title, robots: { index: false } };

type Props = { searchParams: Promise<{ next?: string }> };

/** Where the host's players transfer money. Private to the signed-in user. */
export default async function PayoutPage({ searchParams }: Props) {
  if (!isApiConfigured()) {
    notFound();
  }
  // Only the create page is offered as a way back, so the link cannot point elsewhere.
  const next = (await searchParams).next === "/create" ? "/create" : null;
  const cookie = sessionCookieHeader((await cookies()).get(SESSION_COOKIE)?.value);
  const me = await getMe(cookie);
  if (!me) {
    const here = next ? `/account/payout?next=${encodeURIComponent(next)}` : "/account/payout";
    return <SignInPrompt next={here} title={t.title} />;
  }
  const [banks, payout] = await Promise.all([getBanks(), getPayout(cookie)]);
  return (
    <>
      <AccountBar me={me} />
      <h1 className="create-form__title">{t.title}</h1>
      <p className="muted">{t.intro}</p>
      <PayoutForm banks={banks} initial={payout} next={next} />
    </>
  );
}
