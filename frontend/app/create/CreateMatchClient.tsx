"use client";

import { useRouter } from "next/navigation";
import { CreateMatchForm } from "../../components/CreateMatchForm";
import type { CreateMatchInput } from "../../lib/api";
import { createMatchAction } from "./actions";

export function CreateMatchClient() {
  const router = useRouter();

  async function submit(input: CreateMatchInput) {
    const result = await createMatchAction(input);
    if (!result.ok) {
      return { field: result.field };
    }
    router.push(`/m/${encodeURIComponent(result.match.shareId)}`);
    return undefined;
  }

  return <CreateMatchForm onSubmit={submit} />;
}
