import messages from "../messages/vi.json";

/** Any unknown address. Missing matches have their own page in `m/[shareId]/not-found.tsx`. */
export default function NotFound() {
  return (
    <section className="intro">
      <h1 className="intro__title">{messages.app.notFoundTitle}</h1>
      <p className="muted">{messages.app.notFoundBody}</p>
      <p>
        <a href="/">{messages.app.backHome}</a>
      </p>
    </section>
  );
}
