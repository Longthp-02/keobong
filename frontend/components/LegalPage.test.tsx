import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import messages from "../messages/vi.json";
import { type LegalDocument, LegalPage } from "./LegalPage";

const documents: [string, LegalDocument][] = [
  ["privacy policy", messages.legal.privacy],
  ["terms of use", messages.legal.terms],
];

describe.each(documents)("LegalPage (%s)", (_name, document) => {
  it("renders the title, the last-updated date and every section heading", () => {
    render(<LegalPage document={document} />);

    screen.getByRole("heading", { level: 1, name: document.title });
    screen.getByText(new RegExp(document.updated));
    const headings = screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent);
    expect(headings).toEqual(document.sections.map((s) => s.heading));
  });

  it("links to the contact email so users can exercise their rights", () => {
    render(<LegalPage document={document} />);

    const contact = screen.getByRole("link", { name: messages.legal.contactEmail });
    expect(contact.getAttribute("href")).toBe(`mailto:${messages.legal.contactEmail}`);
  });
});

describe("legal content", () => {
  it("names the operator and states the confirmed minimum age in both documents", () => {
    for (const [, document] of documents) {
      const text = JSON.stringify(document);
      expect(text).toContain(messages.legal.operatorName);
      expect(text).toContain(messages.legal.minimumAgePhrase);
    }
  });
});
