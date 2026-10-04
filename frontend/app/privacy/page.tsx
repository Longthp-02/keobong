import type { Metadata } from "next";
import { LegalPage } from "../../components/LegalPage";
import messages from "../../messages/vi.json";

export const metadata: Metadata = { title: messages.legal.privacy.title };

export default function PrivacyPage() {
  return <LegalPage document={messages.legal.privacy} />;
}
