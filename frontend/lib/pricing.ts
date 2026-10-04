import type { MatchView } from "./api";

/**
 * Mirrors the backend rules in backend/src/matches/domain.rs (MAX_TOTAL_FEE_VND,
 * SLOT_COUNT_RANGE, Format::default_slot_count). The API stays authoritative;
 * change both sides together.
 */
export const MAX_TOTAL_FEE_VND = 100_000_000;
export const MIN_SLOT_COUNT = 2;
export const MAX_SLOT_COUNT = 30;

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
