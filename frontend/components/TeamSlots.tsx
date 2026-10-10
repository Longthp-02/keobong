"use client";

import { type FormEvent, type ReactNode, useCallback, useEffect, useState } from "react";
import { type MatchView, signInUrl } from "../lib/api";
import { formatTimeRange, formatVnd } from "../lib/format";
import {
  type MyPlace,
  type RosterView,
  type SlotError,
  type SlotsClient,
  type Team,
  type TeamView,
  emptyRoster,
  slotsClient,
} from "../lib/slots";
import { fill } from "../lib/text";
import { HostPayments } from "./HostPayments";
import { MatchCard } from "./MatchCard";
import { PaymentPanel } from "./PaymentPanel";
import { ShareButton } from "./ShareButton";
import messages from "../messages/vi.json";

const t = messages.slots;
const MAX_GUESTS = 2;
const JOIN_FORM = "join-form";
const defaultClient = slotsClient();

type Props = {
  match: MatchView;
  /** Server-rendered roster, or `null` if it could not be loaded; refreshed from the API after every change. */
  initialRoster: RosterView | null;
  client?: SlotsClient;
};

function openPlaces(team: TeamView): number {
  return Math.max(0, team.capacity - team.players.length);
}

function Avatar({ name, url }: { name: string; url: string | null }) {
  if (url) {
    // Google avatars refuse requests that carry a referrer from other sites.
    return <img className="slot__circle" src={url} alt="" width={40} height={40} referrerPolicy="no-referrer" />;
  }
  return (
    <span className="slot__circle slot__circle--initial" aria-hidden="true">
      {name.trim().charAt(0).toUpperCase()}
    </span>
  );
}

type Choice = {
  /** The visitor may pick a team now. */
  enabled: boolean;
  /** Nobody can take a place any more (cancelled or started), so open places look unavailable. */
  inactive: boolean;
  /** Places the party would take in this team (0 when another team is chosen). */
  chosenCount: number;
  fits: boolean;
  onChoose: () => void;
};

function TeamColumn({ team, choice }: { team: TeamView; choice: Choice }) {
  const label = t.team[team.team];
  const open = openPlaces(team);
  return (
    <section className="team" aria-label={label}>
      <header className="team__header">
        <h3>{label}</h3>
        <span className="muted">{open > 0 ? fill(t.openPlaces, { count: open }) : t.full}</span>
      </header>
      <ul className="slot-grid">
        {team.players.map((player, index) => {
          const name = player.name ?? messages.auth.anonymousName;
          const host = player.guestOf ?? messages.auth.anonymousName;
          return (
            <li
              key={`${name}-${index}`}
              className="slot"
              title={player.isGuest ? fill(t.guestOf, { name: host }) : name}
            >
              <Avatar name={name} url={player.avatarUrl} />
              <span className="slot__caption">{name}</span>
              {player.isGuest ? <small className="visually-hidden">{fill(t.guestOf, { name: host })}</small> : null}
            </li>
          );
        })}
        {Array.from({ length: open }, (_, index) => {
          const chosen = index < choice.chosenCount;
          const caption = chosen ? (index === 0 ? t.you : t.guestShort) : t.openShort;
          const icon = chosen ? <CheckIcon /> : <PlusIcon />;
          // Every open circle can be tapped, but only the first is announced and
          // focusable, so assistive technology hears one choice per team.
          const first = index === 0;
          return (
            <li
              key={`open-${index}`}
              className={chosen ? "slot slot--chosen" : `slot slot--open${choice.inactive ? " slot--inactive" : ""}`}
            >
              {choice.enabled ? (
                <button
                  type="button"
                  className="slot__circle slot__button"
                  aria-label={first ? fill(t.takeSlot, { team: label }) : undefined}
                  aria-pressed={first ? choice.chosenCount > 0 : undefined}
                  aria-hidden={first ? undefined : true}
                  tabIndex={first ? undefined : -1}
                  disabled={!choice.fits}
                  onClick={choice.onChoose}
                >
                  {icon}
                </button>
              ) : (
                <span className="slot__circle slot__button" aria-hidden="true">
                  {icon}
                </span>
              )}
              <span className="slot__caption">{caption}</span>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

function CheckIcon() {
  return (
    <svg
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M5 12.5l4.5 4.5L19 7.5" />
    </svg>
  );
}

function PlusIcon() {
  return (
    <svg
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.4"
      strokeLinecap="round"
      aria-hidden="true"
    >
      <path d="M12 5v14M5 12h14" />
    </svg>
  );
}

export function TeamSlots({ match, initialRoster, client = defaultClient }: Props) {
  const { shareId, startsAt } = match;
  const [loadedRoster, setRoster] = useState(initialRoster);
  // Until a roster arrives, show empty teams but no counts that could be wrong.
  const roster = loadedRoster ?? emptyRoster(match.slotCount, match.cancelledAt !== null);
  const [isHost, setIsHost] = useState(false);
  const [mine, setMine] = useState<MyPlace | null>(null);
  // Decided after hydration so server and client render the same markup.
  const [started, setStarted] = useState(false);
  const [team, setTeam] = useState<Team | null>(null);
  const [guests, setGuests] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);

  const refreshRoster = useCallback(async () => {
    try {
      setRoster(await client.roster(shareId));
    } catch {
      // Keep showing the last roster; the next action or page load retries.
    }
  }, [client, shareId]);

  const loadMine = useCallback(() => {
    setLoadFailed(false);
    client.mine(shareId).then(setMine, () => setLoadFailed(true));
  }, [client, shareId]);

  useEffect(() => {
    setStarted(Date.parse(startsAt) <= Date.now());
    loadMine();
    // The page itself is cached for up to 30 seconds; show who has joined since.
    void refreshRoster();
  }, [startsAt, loadMine, refreshRoster]);

  const partySize = 1 + guests.length;
  const fits = (candidate: TeamView) => openPlaces(candidate) >= partySize;
  const chosenView = roster.teams.find((candidate) => candidate.team === team && fits(candidate));
  const chosen = chosenView?.team ?? roster.teams.find(fits)?.team ?? null;

  const canChoose = !roster.cancelled && !started && !loadFailed && mine?.status === "out";

  function joinLabel(chosenTeam: Team): string {
    const label = fill(t.join, { team: t.team[chosenTeam] });
    // The host's own party is confirmed without a transfer.
    const amount = isHost ? 0 : match.pricePerPlayerVnd * partySize;
    return amount > 0 ? `${label} · ${formatVnd(amount)}` : label;
  }

  function choiceFor(candidate: TeamView) {
    return {
      enabled: canChoose && !busy,
      inactive: roster.cancelled || started,
      chosenCount: canChoose && chosen === candidate.team ? partySize : 0,
      fits: fits(candidate),
      onChoose: () => setTeam(candidate.team),
    };
  }

  function showError(code: SlotError) {
    setError(t.errors[code]);
    // Move the UI to the state the server reported.
    if (code === "unauthenticated") {
      setMine({ status: "signedOut" });
    }
    if (code === "match_started") {
      setStarted(true);
    }
    if (code === "match_cancelled") {
      void refreshRoster();
    }
  }

  async function join(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!chosen) {
      return;
    }
    const names = guests.map((name) => name.trim());
    if (names.some((name) => name.length === 0 || name.length > 40)) {
      showError("guests");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const result = await client.join(shareId, chosen, names);
      if (result.ok) {
        setMine(result.place);
        setGuests([]);
      } else {
        showError(result.error);
        if (result.error === "already_joined") {
          setMine(await client.mine(shareId));
        }
      }
      await refreshRoster();
    } catch {
      showError("unexpected");
    } finally {
      setBusy(false);
    }
  }

  const reloadAfterExpiry = useCallback(() => {
    loadMine();
    void refreshRoster();
  }, [loadMine, refreshRoster]);

  async function reportPayment() {
    setBusy(true);
    setError(null);
    try {
      const result = await client.reportPayment(shareId);
      if (result.ok) {
        setMine(result.place);
      } else {
        showError(result.error);
        if (result.error === "not_joined") {
          setMine({ status: "out" });
          await refreshRoster();
        }
      }
    } catch {
      showError("unexpected");
    } finally {
      setBusy(false);
    }
  }

  async function leave() {
    setBusy(true);
    setError(null);
    try {
      const result = await client.leave(shareId);
      if (result.ok) {
        setMine({ status: "out" });
      } else {
        showError(result.error);
      }
      await refreshRoster();
    } catch {
      showError("unexpected");
    } finally {
      setBusy(false);
    }
  }

  function actions() {
    if (roster.cancelled) {
      return (
        <div className="my-place">
          <p className="form-error">{t.cancelled}</p>
          {mine?.status === "in" && mine.amountVnd > 0 ? <p className="muted">{t.cancelledRefund}</p> : null}
        </div>
      );
    }
    if (started) {
      return <p className="muted">{t.started}</p>;
    }
    if (loadFailed || !mine || mine.status === "signedOut") {
      return null;
    }
    if (mine.status === "in") {
      return (
        <div className="my-place">
          <p>
            <strong>{fill(t.joined, { team: t.team[mine.team] })}</strong>
          </p>
          {mine.guests.length > 0 ? (
            <p className="muted">{fill(t.withGuests, { names: mine.guests.join(", ") })}</p>
          ) : null}
          <PaymentPanel place={mine} onReport={reportPayment} onExpired={reloadAfterExpiry} busy={busy} />
          <button type="button" className="button-secondary" onClick={leave} disabled={busy}>
            {busy ? t.leaving : t.leave}
          </button>
        </div>
      );
    }
    return (
      <form id={JOIN_FORM} className="join-form" onSubmit={join} noValidate>
        <fieldset className="field">
          <legend>{t.guestsLabel}</legend>
          {guests.map((name, index) => (
            <div key={index} className="guest-row">
              <input
                type="text"
                value={name}
                maxLength={40}
                placeholder={t.guestPlaceholder}
                aria-label={`${t.guestPlaceholder} ${index + 1}`}
                onChange={(e) => setGuests(guests.map((g, i) => (i === index ? e.target.value : g)))}
              />
              <button
                type="button"
                className="link-button"
                onClick={() => setGuests(guests.filter((_, i) => i !== index))}
              >
                {t.removeGuest}
              </button>
            </div>
          ))}
          {guests.length < MAX_GUESTS ? (
            <button type="button" className="link-button" onClick={() => setGuests([...guests, ""])}>
              {t.addGuest}
            </button>
          ) : null}
        </fieldset>
        {match.pricePerPlayerVnd > 0 && !isHost ? (
          <div className="pay-info">
            <strong>{t.payInfoTitle}</strong>
            <p className="muted">{t.payInfoBody}</p>
          </div>
        ) : null}
      </form>
    );
  }

  /** The main action in the bottom bar, next to sharing. */
  function primaryAction(): ReactNode {
    if (roster.cancelled || started) {
      return null;
    }
    if (loadFailed) {
      return (
        <button type="button" className="button-primary" onClick={loadMine}>
          {t.retry}
        </button>
      );
    }
    if (mine?.status === "signedOut") {
      return (
        <a href={signInUrl(`/m/${shareId}`)} className="button-primary">
          {t.signInToJoin}
        </a>
      );
    }
    if (mine?.status === "out") {
      return (
        <button type="submit" form={JOIN_FORM} className="button-primary" disabled={!chosen || busy}>
          {busy ? t.joining : chosen ? joinLabel(chosen) : t.full}
        </button>
      );
    }
    return null;
  }

  return (
    <>
      <MatchCard match={match} roster={loadedRoster} started={started} />
      <section className="team-slots" aria-labelledby="team-slots-title">
        <div className="roster-card">
          <header className="roster-card__header">
            <h2 id="team-slots-title" className="team-slots__title">
              {t.title}
            </h2>
            {canChoose ? <span className="muted">{t.tapHint}</span> : null}
          </header>
          {roster.teams.map((candidate) => (
            <TeamColumn key={candidate.team} team={candidate} choice={choiceFor(candidate)} />
          ))}
        </div>
        {error ? (
          <p className="form-error" role="alert">
            {error}
          </p>
        ) : null}
        {actions()}
        <HostPayments
          shareId={shareId}
          startsAt={startsAt}
          cancelled={roster.cancelled}
          client={client}
          onChange={refreshRoster}
          onHostKnown={setIsHost}
        />
        {/* Rendered once, so the share button keeps its state while the rest changes. */}
        <div className="action-bar">
          <ShareButton
            shareId={shareId}
            title={`${match.venueName} · ${formatTimeRange(match.startsAt, match.endsAt)}`}
          />
          {primaryAction()}
        </div>
      </section>
    </>
  );
}
