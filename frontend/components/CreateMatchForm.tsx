"use client";

import { type FormEvent, useState } from "react";
import type { CreateMatchInput, MatchView } from "../lib/api";
import { formatVnd } from "../lib/format";
import { defaultSlotCount, pricePerPlayerVnd } from "../lib/pricing";
import messages from "../messages/vi.json";

const t = messages.create;
const FORMATS: MatchView["format"][] = ["five_a_side", "seven_a_side", "eleven_a_side"];
const MATCH_TYPES: MatchView["matchType"][] = ["casual", "competitive", "beginner_friendly"];
const LEVELS = Array.from({ length: 9 }, (_, i) => 1 + i * 0.5);

type Props = {
  /** Resolves with the rejected field, or undefined when the match was created (the caller navigates). */
  onSubmit: (input: CreateMatchInput) => Promise<{ field: string } | undefined>;
};

/** Converts a date and time picked in Ho Chi Minh City (UTC+7, no DST) to a UTC ISO string. */
function toUtcIso(date: string, time: string): string {
  return new Date(`${date}T${time}:00+07:00`).toISOString();
}

function errorMessage(field: string): string {
  const errors: Record<string, string> = t.errors;
  return errors[field] ?? t.errors.unknown;
}

export function CreateMatchForm({ onSubmit }: Props) {
  const [venueName, setVenueName] = useState("");
  const [date, setDate] = useState("");
  const [startTime, setStartTime] = useState("");
  const [endTime, setEndTime] = useState("");
  const [format, setFormat] = useState<MatchView["format"]>("seven_a_side");
  const [matchType, setMatchType] = useState<MatchView["matchType"]>("casual");
  const [levelMin, setLevelMin] = useState(2.5);
  const [levelMax, setLevelMax] = useState(3.5);
  const [totalFee, setTotalFee] = useState("");
  const [slotCount, setSlotCount] = useState(String(defaultSlotCount("seven_a_side")));
  const [slotsTouched, setSlotsTouched] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const price = pricePerPlayerVnd(Number(totalFee), Number(slotCount));

  function chooseFormat(next: MatchView["format"]) {
    setFormat(next);
    if (!slotsTouched) {
      setSlotCount(String(defaultSlotCount(next)));
    }
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError(null);
    setSubmitting(true);
    try {
      const rejected = await onSubmit({
        venueName,
        startsAt: toUtcIso(date, startTime),
        endsAt: toUtcIso(date, endTime),
        format,
        matchType,
        levelMin,
        levelMax,
        totalFeeVnd: Number(totalFee),
        slotCount: Number(slotCount),
      });
      if (rejected) {
        setError(errorMessage(rejected.field));
      }
    } catch {
      setError(t.unexpectedError);
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <form className="create-form" onSubmit={handleSubmit} noValidate>
      <h1 className="create-form__title">{t.title}</h1>

      <label className="field">
        <span>{t.venue}</span>
        <input
          type="text"
          value={venueName}
          maxLength={120}
          placeholder={t.venuePlaceholder}
          onChange={(e) => setVenueName(e.target.value)}
          required
        />
      </label>

      <label className="field">
        <span>{t.date}</span>
        <input type="date" value={date} onChange={(e) => setDate(e.target.value)} required />
      </label>

      <div className="field-row">
        <label className="field">
          <span>{t.startTime}</span>
          <input type="time" value={startTime} onChange={(e) => setStartTime(e.target.value)} required />
        </label>
        <label className="field">
          <span>{t.endTime}</span>
          <input type="time" value={endTime} onChange={(e) => setEndTime(e.target.value)} required />
        </label>
      </div>

      <fieldset className="field">
        <legend>{t.format}</legend>
        <div className="segmented">
          {FORMATS.map((option) => (
            <label key={option} className="segmented__option">
              <input
                type="radio"
                name="format"
                value={option}
                checked={format === option}
                onChange={() => chooseFormat(option)}
              />
              <span>{messages.match.format[option]}</span>
            </label>
          ))}
        </div>
      </fieldset>

      <fieldset className="field">
        <legend>{t.matchType}</legend>
        <div className="chips">
          {MATCH_TYPES.map((option) => (
            <label key={option} className="chip">
              <input
                type="radio"
                name="matchType"
                value={option}
                checked={matchType === option}
                onChange={() => setMatchType(option)}
              />
              <span>{messages.match.matchType[option]}</span>
            </label>
          ))}
        </div>
      </fieldset>

      <div className="field-row">
        <label className="field">
          <span>{t.levelMin}</span>
          <select value={levelMin} onChange={(e) => setLevelMin(Number(e.target.value))}>
            {LEVELS.map((level) => (
              <option key={level} value={level}>
                {level.toFixed(1)}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>{t.levelMax}</span>
          <select value={levelMax} onChange={(e) => setLevelMax(Number(e.target.value))}>
            {LEVELS.map((level) => (
              <option key={level} value={level}>
                {level.toFixed(1)}
              </option>
            ))}
          </select>
        </label>
      </div>

      <div className="field-row">
        <label className="field">
          <span>{t.totalFee}</span>
          <input
            type="number"
            inputMode="numeric"
            min={0}
            step={1000}
            value={totalFee}
            onChange={(e) => setTotalFee(e.target.value)}
            required
          />
        </label>
        <label className="field">
          <span>{t.slotCount}</span>
          <input
            type="number"
            inputMode="numeric"
            min={2}
            max={30}
            value={slotCount}
            onChange={(e) => {
              setSlotsTouched(true);
              setSlotCount(e.target.value);
            }}
            required
          />
        </label>
      </div>
      <p className="muted field-hint">{t.slotHint}</p>

      <p className="price-preview">
        <span>{t.pricePerPlayer}</span>
        <strong data-testid="price-per-player">{price === null ? "—" : formatVnd(price)}</strong>
      </p>

      {error ? (
        <p className="form-error" role="alert">
          {error}
        </p>
      ) : null}

      <button type="submit" className="button-primary" disabled={submitting}>
        {submitting ? t.submitting : t.submit}
      </button>
    </form>
  );
}
