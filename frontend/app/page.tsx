import { isApiConfigured } from "../lib/api";
import messages from "../messages/vi.json";

export default function HomePage() {
  return (
    <section className="intro">
      <h1 className="intro__title">{messages.app.name}</h1>
      <p>{messages.app.tagline}</p>
      <p className="muted">{messages.app.comingSoon}</p>
      {isApiConfigured() ? (
        <a href="/create" className="button-primary">
          {messages.app.createLink}
        </a>
      ) : null}
    </section>
  );
}
