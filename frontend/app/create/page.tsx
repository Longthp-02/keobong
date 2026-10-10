import type { Metadata } from "next";
import { cookies } from "next/headers";
import { notFound } from "next/navigation";
import { AccountBar } from "../../components/AccountBar";
import { SignInPrompt } from "../../components/SignInPrompt";
import {
  SESSION_COOKIE,
  getBanks,
  getMe,
  getPayout,
  getVenues,
  isApiConfigured,
  sessionCookieHeader,
} from "../../lib/api";
import { canHostPaidMatches } from "../../lib/payout";
import messages from "../../messages/vi.json";
import { CreateMatchClient } from "./CreateMatchClient";

export const metadata: Metadata = { title: messages.create.title };

export default async function CreateMatchPage() {
  // Until the API is deployed, creating a match cannot work, so the page does not exist.
  if (!isApiConfigured()) {
    notFound();
  }
  const cookie = sessionCookieHeader((await cookies()).get(SESSION_COOKIE)?.value);
  const me = await getMe(cookie);
  if (!me) {
    return <SignInPrompt next="/create" />;
  }
  const [payout, banks, venues] = await Promise.all([
    getPayout(cookie),
    getBanks(),
    // The form explains an empty list, which beats an error page (e.g. during a deploy).
    getVenues().catch((error: unknown) => {
      console.error("venues unavailable", error);
      return [];
    }),
  ]);
  const hasPayout = canHostPaidMatches(payout, banks);
  return (
    <>
      <AccountBar me={me} />
      <CreateMatchClient hasPayout={hasPayout} venues={venues} />
    </>
  );
}
