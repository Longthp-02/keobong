import messages from "../../../messages/vi.json";

export default function MatchNotFound() {
  return (
    <section className="intro">
      <h1 className="intro__title">{messages.match.notFoundTitle}</h1>
      <p className="muted">{messages.match.notFoundBody}</p>
    </section>
  );
}
