"use server";

import { cookies } from "next/headers";
import {
  type CreateMatchInput,
  type CreateMatchResult,
  SESSION_COOKIE,
  createMatch,
  sessionCookieHeader,
} from "../../lib/api";

/**
 * Server-side call so the API base URL stays private to the server. The
 * session cookie is forwarded so the API can identify the host.
 */
export async function createMatchAction(input: CreateMatchInput): Promise<CreateMatchResult> {
  const session = (await cookies()).get(SESSION_COOKIE)?.value;
  return createMatch(input, { cookie: sessionCookieHeader(session) });
}
