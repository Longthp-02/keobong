import { MatchList } from "../components/MatchList";
import { isApiConfigured } from "../lib/api";
import messages from "../messages/vi.json";

export default function HomePage() {
  // Without a backend (a preview with no API) there is nothing to list yet.
  if (!isApiConfigured()) {
    return (
      <section className="intro">
        <h1 className="intro__title">{messages.app.name}</h1>
        <p>{messages.app.tagline}</p>
        <p className="muted">{messages.app.comingSoon}</p>
      </section>
    );
  }
  return (
    <>
      <h1 className="visually-hidden">{messages.app.name}</h1>
      <p className="muted home__tagline">{messages.app.tagline}</p>
      <MatchList />
    </>
  );
}
