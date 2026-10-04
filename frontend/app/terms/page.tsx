import type { Metadata } from "next";
import { LegalPage } from "../../components/LegalPage";
import messages from "../../messages/vi.json";

export const metadata: Metadata = { title: messages.legal.terms.title };

export default function TermsPage() {
  return <LegalPage document={messages.legal.terms} />;
}
