import type { Metadata } from "next";
import { cookies } from "next/headers";
import { notFound } from "next/navigation";
import { BrowserAccountBar } from "../../../components/BrowserAccountBar";
import { PollBoard } from "../../../components/PollBoard";
import { SESSION_COOKIE, getPoll, isApiConfigured, sessionCookieHeader } from "../../../lib/api";
import messages from "../../../messages/vi.json";

type Props = { params: Promise<{ shareId: string }> };

// The poll shows the viewer's own answers, so it is rendered per request.
export const dynamic = "force-dynamic";

// Link previews read these tags; they show only the poll's title.
export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { shareId } = await params;
  const poll = isApiConfigured() ? await getPoll(shareId).catch(() => null) : null;
  const title = poll ? poll.title : messages.poll.pageTitle;
  return { title, robots: { index: false }, openGraph: { title, siteName: messages.app.name } };
}

/** Where Bot DaGhep's attendance poll links land: the table, and yes/no for signed-in players. */
export default async function PollPage({ params }: Props) {
  if (!isApiConfigured()) {
    notFound();
  }
  const { shareId } = await params;
  const cookie = sessionCookieHeader((await cookies()).get(SESSION_COOKIE)?.value);
  const poll = await getPoll(shareId, { cookie });
  if (!poll) {
    notFound();
  }
  return (
    <>
      <BrowserAccountBar />
      <PollBoard shareId={shareId} initial={poll} />
    </>
  );
}
