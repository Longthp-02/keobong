/** The host's payout account: where players transfer the match fee. */

export type Bank = { bin: string; name: string };

export type PayoutAccount = {
  bankBin: string;
  accountNumber: string;
  accountName: string;
};

export type PayoutError = "bankBin" | "accountNumber" | "accountName" | "unauthenticated" | "unexpected";

/**
 * Converts a typed name to the form banks show: uppercase Latin letters
 * without diacritics. While typing, a trailing space is kept so words can be
 * separated.
 */
export function toAccountName(input: string, options: { typing?: boolean } = {}): string {
  const letters = input
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/[đĐ]/g, "D")
    .toUpperCase()
    .replace(/[^A-Z ]/g, " ")
    .replace(/ +/g, " ");
  return options.typing ? letters.replace(/^ /, "") : letters.trim();
}

const FIELDS: PayoutError[] = ["bankBin", "accountNumber", "accountName"];

/** Saves the signed-in user's payout account through the site's /api proxy. */
export async function savePayout(
  account: PayoutAccount,
  fetchImpl: typeof fetch = (...args) => fetch(...args),
): Promise<{ ok: true } | { ok: false; error: PayoutError }> {
  const response = await fetchImpl("/api/me/payout", {
    method: "PUT",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(account),
  });
  if (response.ok) {
    return { ok: true };
  }
  if (response.status === 401) {
    return { ok: false, error: "unauthenticated" };
  }
  if (response.status === 422) {
    const body = (await response.json().catch(() => ({}))) as { field?: string };
    if (FIELDS.includes(body.field as PayoutError)) {
      return { ok: false, error: body.field as PayoutError };
    }
  }
  return { ok: false, error: "unexpected" };
}
