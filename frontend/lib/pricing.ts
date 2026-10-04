import type { MatchView } from "./api";

/** Mirrors the backend rules (backend/src/matches/domain.rs); the API stays authoritative. */
const DEFAULT_SLOTS: Record<MatchView["format"], number> = {
  five_a_side: 14,
  seven_a_side: 18,
  eleven_a_side: 28,
};

export function defaultSlotCount(format: MatchView["format"]): number {
  return DEFAULT_SLOTS[format];
}

/** Total fee split across all slots, rounded up to the next 1,000 VND. */
export function pricePerPlayerVnd(totalFeeVnd: number, slotCount: number): number | null {
  if (!Number.isInteger(totalFeeVnd) || totalFeeVnd < 0 || !Number.isInteger(slotCount) || slotCount < 1) {
    return null;
  }
  return Math.ceil(totalFeeVnd / slotCount / 1000) * 1000;
}
