import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { isApiConfigured } from "../../lib/api";
import messages from "../../messages/vi.json";
import { CreateMatchClient } from "./CreateMatchClient";

export const metadata: Metadata = { title: messages.create.title };

export default function CreateMatchPage() {
  // Until the API is deployed, creating a match cannot work, so the page does not exist.
  if (!isApiConfigured()) {
    notFound();
  }
  return <CreateMatchClient />;
}
