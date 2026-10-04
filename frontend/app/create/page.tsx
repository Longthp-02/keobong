import type { Metadata } from "next";
import { cookies } from "next/headers";
import { notFound } from "next/navigation";
import { AccountBar } from "../../components/AccountBar";
import { SignInPrompt } from "../../components/SignInPrompt";
import { SESSION_COOKIE, getMe, isApiConfigured, sessionCookieHeader } from "../../lib/api";
import messages from "../../messages/vi.json";
import { CreateMatchClient } from "./CreateMatchClient";

export const metadata: Metadata = { title: messages.create.title };

export default async function CreateMatchPage() {
  // Until the API is deployed, creating a match cannot work, so the page does not exist.
  if (!isApiConfigured()) {
    notFound();
  }
  const session = (await cookies()).get(SESSION_COOKIE)?.value;
  const me = await getMe(sessionCookieHeader(session));
  if (!me) {
    return <SignInPrompt next="/create" />;
  }
  return (
    <>
      <AccountBar me={me} />
      <CreateMatchClient />
    </>
  );
}
