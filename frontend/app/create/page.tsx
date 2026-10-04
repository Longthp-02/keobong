import type { Metadata } from "next";
import messages from "../../messages/vi.json";
import { CreateMatchClient } from "./CreateMatchClient";

export const metadata: Metadata = { title: messages.create.title };

export default function CreateMatchPage() {
  return <CreateMatchClient />;
}
