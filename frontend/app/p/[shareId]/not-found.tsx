import messages from "../../../messages/vi.json";

export default function PollNotFound() {
  return (
    <section className="intro">
      <h1 className="intro__title">{messages.poll.pageTitle}</h1>
      <p className="muted">{messages.poll.notFound}</p>
    </section>
  );
}
