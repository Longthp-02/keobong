"use server";

import { type CreateMatchInput, type CreateMatchResult, createMatch } from "../../lib/api";

/** Server-side call so the API base URL stays private to the server. */
export async function createMatchAction(input: CreateMatchInput): Promise<CreateMatchResult> {
  return createMatch(input);
}
