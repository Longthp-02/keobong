import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { MatchCard } from "../../../components/MatchCard";
import { TeamSlots } from "../../../components/TeamSlots";
import { getMatch, getRoster } from "../../../lib/api";
import { emptyRoster } from "../../../lib/slots";
import { formatMatchDate, formatTimeRange } from "../../../lib/format";
import messages from "../../../messages/vi.json";

type Props = { params: Promise<{ shareId: string }> };

// Incremental static regeneration: each match page is rendered on first request,
// then served from the CDN cache and refreshed at most every 30 seconds.
export const revalidate = 30;

export async function generateStaticParams() {
  return [];
}

// Link previews (Zalo, Messenger) read these tags. They must never include payment details.
export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { shareId } = await params;
  const match = await getMatch(shareId);
  if (!match) {
    return { title: messages.match.notFoundTitle };
  }
  const title = `${match.venueName} · ${formatTimeRange(match.startsAt, match.endsAt)}`;
  const description = `${formatMatchDate(match.startsAt)} · ${messages.match.format[match.format]} · ${messages.match.matchType[match.matchType]}`;
  return { title, description, openGraph: { title, description, siteName: messages.app.name } };
}

export default async function MatchPage({ params }: Props) {
  const { shareId } = await params;
  const [match, roster] = await Promise.all([
    getMatch(shareId),
    // The share link must still work if only the roster is unavailable; the
    // client refreshes it after loading.
    getRoster(shareId).catch((error: unknown) => {
      console.error("roster unavailable", error);
      return null;
    }),
  ]);
  if (!match) {
    notFound();
  }
  return (
    <>
      <MatchCard match={match} />
      <TeamSlots shareId={match.shareId} startsAt={match.startsAt} initialRoster={roster ?? emptyRoster(match.slotCount)} />
    </>
  );
}
