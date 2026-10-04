"use server";

import { cookies } from "next/headers";
import { type CreateMatchInput, type CreateMatchResult, createMatch } from "../../lib/api";

/**
 * Server-side call so the API base URL stays private to the server. The
 * browser's cookies are forwarded so the API can identify the host.
 */
export async function createMatchAction(input: CreateMatchInput): Promise<CreateMatchResult> {
  return createMatch(input, { cookie: (await cookies()).toString() });
}
