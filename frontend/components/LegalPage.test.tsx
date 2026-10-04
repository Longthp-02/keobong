import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import messages from "../messages/vi.json";
import { LegalPage } from "./LegalPage";

describe("LegalPage", () => {
  it("renders the privacy policy title, every section heading and the contact email", () => {
    render(<LegalPage document={messages.legal.privacy} />);

    expect(screen.getByRole("heading", { level: 1, name: messages.legal.privacy.title })).toBeTruthy();
    for (const section of messages.legal.privacy.sections) {
      expect(screen.getByRole("heading", { level: 2, name: section.heading })).toBeTruthy();
    }
    const contact = screen.getAllByRole("link", { name: messages.legal.contactEmail });
    expect(contact[0].getAttribute("href")).toBe(`mailto:${messages.legal.contactEmail}`);
  });

  it("renders the terms of use with its last-updated date", () => {
    render(<LegalPage document={messages.legal.terms} />);

    expect(screen.getByRole("heading", { level: 1, name: messages.legal.terms.title })).toBeTruthy();
    expect(screen.getByText(new RegExp(messages.legal.terms.updated))).toBeTruthy();
  });
});
