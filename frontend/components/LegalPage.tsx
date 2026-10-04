import messages from "../messages/vi.json";

export type LegalDocument = {
  title: string;
  updated: string;
  intro: string;
  sections: { heading: string; paragraphs: string[] }[];
};

const t = messages.legal;

export function LegalPage({ document }: { document: LegalDocument }) {
  return (
    <article className="legal">
      <h1 className="legal__title">{document.title}</h1>
      <p className="muted">{`${t.updatedLabel} ${document.updated}`}</p>
      <p>{document.intro}</p>
      {document.sections.map((section) => (
        <section key={section.heading}>
          <h2 className="legal__heading">{section.heading}</h2>
          {section.paragraphs.map((paragraph) => (
            <p key={paragraph}>{paragraph}</p>
          ))}
        </section>
      ))}
      <p className="legal__contact">
        {t.contactLabel} <a href={`mailto:${t.contactEmail}`}>{t.contactEmail}</a>
      </p>
    </article>
  );
}
