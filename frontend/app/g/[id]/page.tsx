import type { Metadata } from "next";
import { cookies } from "next/headers";
import Link from "next/link";
import { notFound } from "next/navigation";
import { cache } from "react";
import { BrowserAccountBar } from "../../../components/BrowserAccountBar";
import { GroupJoin } from "../../../components/GroupJoin";
import { GroupDetails } from "../../../components/GroupList";
import { SESSION_COOKIE, getGroup, isApiConfigured, sessionCookieHeader } from "../../../lib/api";
import messages from "../../../messages/vi.json";

type Props = { params: Promise<{ id: string }> };

/** One API call per request for both the metadata and the page. */
const loadGroup = cache(async (id: string) => {
  const cookie = sessionCookieHeader((await cookies()).get(SESSION_COOKIE)?.value);
  return getGroup(id, { cookie });
});

// The page shows the viewer's own standing with the group, so it is rendered per request.
export const dynamic = "force-dynamic";

// Link previews read these tags; they show only the group's title.
export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { id } = await params;
  const group = isApiConfigured()
    ? await loadGroup(id).catch((error: unknown) => {
        console.error("group unavailable for metadata", error);
        return null;
      })
    : null;
  const title = group ? group.title : messages.groups.pageTitle;
  return { title, openGraph: { title, siteName: messages.app.name } };
}

/** A public weekly group: what it plays, and how to join it. */
export default async function GroupPage({ params }: Props) {
  if (!isApiConfigured()) {
    notFound();
  }
  const { id } = await params;
  const group = await loadGroup(id);
  if (!group) {
    notFound();
  }
  return (
    <>
      <BrowserAccountBar />
      <section className="group-page">
        <Link href="/" className="muted">
          {messages.groups.back}
        </Link>
        <div className="group-card">
          <GroupDetails group={group} />
        </div>
        <GroupJoin groupId={group.id} initial={group.membership} />
      </section>
    </>
  );
}
