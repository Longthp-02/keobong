"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import {
  type OpenMatch,
  type OpenMatchPage,
  type OpenMatchQuery,
  type MatchView,
  browserOpenMatches,
} from "../lib/api";
import { formatClock, formatLevelRange } from "../lib/format";
import { formatDistance, formatShortVnd, listDays } from "../lib/matchList";
import { fill } from "../lib/text";
import messages from "../messages/vi.json";

const t = messages.list;
const TYPES: MatchView["matchType"][] = ["casual", "competitive", "beginner_friendly"];
/** At or below this, the places-left label turns urgent (as in the design). */
const FEW_PLACES = 2;

type Position = { lat: number; lng: number };

type Props = {
  load?: (query: OpenMatchQuery) => Promise<OpenMatchPage>;
  now?: () => Date;
  /** Asks the browser where the visitor is. The position is only sent with list requests. */
  locate?: () => Promise<Position>;
};

function browserLocate(): Promise<Position> {
  return new Promise((resolve, reject) => {
    if (!("geolocation" in navigator)) {
      reject(new Error("geolocation unavailable"));
      return;
    }
    navigator.geolocation.getCurrentPosition(
      (position) => resolve({ lat: position.coords.latitude, lng: position.coords.longitude }),
      reject,
      { maximumAge: 5 * 60 * 1000, timeout: 10_000 },
    );
  });
}

function PinIcon() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M12 21s-7-6.2-7-11.5A7 7 0 0 1 19 9.5C19 14.8 12 21 12 21z" />
      <circle cx="12" cy="9.5" r="2.5" />
    </svg>
  );
}

function MatchItem({ match }: { match: OpenMatch }) {
  const joined = Math.max(0, match.slotCount - match.placesLeft);
  const urgent = match.placesLeft <= FEW_PLACES;
  return (
    <li>
      <a href={`/m/${encodeURIComponent(match.shareId)}`} className="list-card">
        {/* A drawn pitch instead of a photo; the tint follows the match type. */}
        <div className={`list-card__pitch list-card__pitch--${match.matchType}`} aria-hidden="true">
          <span className="list-card__pitch-lines" />
        </div>
        <span className="list-card__type tag tag--dark">{messages.match.matchType[match.matchType]}</span>
        <span className="list-card__price">{formatShortVnd(match.pricePerPlayerVnd)}</span>
        <div className="list-card__body">
          <div className="list-card__headline">
            <span className="list-card__time">{formatClock(match.startsAt)}</span>
            <span className="list-card__venue">{match.venueName}</span>
            {match.distanceM !== null ? (
              <span className="list-card__distance">{formatDistance(match.distanceM)}</span>
            ) : null}
          </div>
          <div className="list-card__chips">
            <span className="chip-static">{messages.match.format[match.format]}</span>
            <span className="chip-static">
              {messages.match.level} {formatLevelRange(match.levelMin, match.levelMax)}
            </span>
          </div>
          <div className="list-card__places">
            <span className="list-card__dots" aria-hidden="true">
              {Array.from({ length: match.slotCount }, (_, index) => (
                <span key={index} className={index < joined ? "dot dot--filled" : "dot"} />
              ))}
            </span>
            <span className={urgent ? "list-card__left list-card__left--urgent" : "list-card__left"}>
              {fill(messages.match.placesLeft, { count: match.placesLeft })}
            </span>
          </div>
        </div>
      </a>
    </li>
  );
}

/** Home page: upcoming matches that still need players, by day and type. */
export function MatchList({ load = browserOpenMatches, now = () => new Date(), locate = browserLocate }: Props) {
  // Computed once per visit so the day picker does not change under the visitor's finger.
  const [days] = useState(() => listDays(now()));
  const [date, setDate] = useState(days[0].date);
  const [type, setType] = useState<MatchView["matchType"] | null>(null);
  const [near, setNear] = useState<Position | null>(null);
  const [matches, setMatches] = useState<OpenMatch[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [status, setStatus] = useState<"loading" | "ready" | "failed">("loading");
  const [loadingMore, setLoadingMore] = useState(false);
  const [locating, setLocating] = useState(false);
  const [locationFailed, setLocationFailed] = useState(false);
  // Answers for filters the visitor has already left are dropped.
  const generation = useRef(0);

  const query = useCallback(
    (extra: Partial<OpenMatchQuery> = {}): OpenMatchQuery => ({
      date,
      ...(type ? { type } : {}),
      ...(near ? { near } : {}),
      ...extra,
    }),
    [date, type, near],
  );

  const reload = useCallback(() => {
    const current = ++generation.current;
    setStatus("loading");
    load(query()).then(
      (page) => {
        if (current === generation.current) {
          setMatches(page.matches);
          setCursor(page.nextCursor);
          setStatus("ready");
        }
      },
      (error: unknown) => {
        if (current === generation.current) {
          console.error("could not load matches", error);
          setStatus("failed");
        }
      },
    );
  }, [load, query]);

  useEffect(() => {
    reload();
  }, [reload]);

  async function loadMore() {
    if (!cursor) {
      return;
    }
    const current = generation.current;
    setLoadingMore(true);
    try {
      const page = await load(query({ cursor }));
      if (current === generation.current) {
        setMatches((shown) => [...shown, ...page.matches]);
        setCursor(page.nextCursor);
      }
    } catch (error) {
      console.error("could not load more matches", error);
      setStatus("failed");
    } finally {
      setLoadingMore(false);
    }
  }

  async function shareMyPosition() {
    setLocating(true);
    setLocationFailed(false);
    try {
      setNear(await locate());
    } catch {
      setLocationFailed(true);
    } finally {
      setLocating(false);
    }
  }

  return (
    <section className="match-list" aria-labelledby="match-list-summary">
      <div className="match-list__near">
        <button
          type="button"
          className={near ? "pill pill--active" : "pill"}
          onClick={shareMyPosition}
          disabled={locating}
          aria-pressed={near !== null}
        >
          <PinIcon />
          {locating ? t.locating : near ? t.nearMeActive : t.nearMe}
        </button>
      </div>
      {locationFailed ? <p className="muted match-list__note">{t.locationFailed}</p> : null}

      <div className="day-picker" role="group" aria-label={t.daysLabel}>
        {days.map((day) => (
          <button
            key={day.date}
            type="button"
            className={day.date === date ? "day day--active" : "day"}
            aria-pressed={day.date === date}
            onClick={() => setDate(day.date)}
          >
            <span className="day__label">{day.label}</span>
            <span className="day__sub">{day.sub}</span>
          </button>
        ))}
      </div>

      <div className="type-filter" role="group" aria-label={t.typesLabel}>
        <button
          type="button"
          className={type === null ? "pill pill--active" : "pill"}
          aria-pressed={type === null}
          onClick={() => setType(null)}
        >
          {t.allTypes}
        </button>
        {TYPES.map((option) => (
          <button
            key={option}
            type="button"
            className={type === option ? "pill pill--active" : "pill"}
            aria-pressed={type === option}
            onClick={() => setType(option)}
          >
            {messages.match.matchType[option]}
          </button>
        ))}
      </div>

      {status === "failed" ? (
        <div className="match-list__state">
          <p className="form-error" role="alert">
            {t.loadFailed}
          </p>
          <button type="button" className="button-secondary" onClick={reload}>
            {t.retry}
          </button>
        </div>
      ) : status === "loading" ? (
        <p className="muted match-list__state" id="match-list-summary">
          {t.loading}
        </p>
      ) : matches.length === 0 ? (
        <div className="match-list__state">
          <p className="muted" id="match-list-summary">
            {t.empty}
          </p>
          <a href="/create" className="button-primary">
            {messages.app.createLink}
          </a>
        </div>
      ) : (
        <>
          <p className="muted match-list__summary" id="match-list-summary">
            {fill(t.summary, { count: matches.length })}
          </p>
          <ul className="match-list__items">
            {matches.map((match) => (
              <MatchItem key={match.shareId} match={match} />
            ))}
          </ul>
          {cursor ? (
            <button type="button" className="button-secondary" onClick={loadMore} disabled={loadingMore}>
              {loadingMore ? t.loading : t.loadMore}
            </button>
          ) : null}
        </>
      )}

      {/* The empty state has its own button for this. */}
      {status === "ready" && matches.length === 0 ? null : (
        <a href="/create" className="fab">
          <svg
            width="20"
            height="20"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.4"
            strokeLinecap="round"
            aria-hidden="true"
          >
            <path d="M12 5v14M5 12h14" />
          </svg>
          {messages.app.createLink}
        </a>
      )}
    </section>
  );
}
