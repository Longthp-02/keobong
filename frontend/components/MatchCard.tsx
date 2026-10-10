import messages from "../messages/vi.json";
import type { MatchView } from "../lib/api";
import type { RosterView } from "../lib/slots";
import { formatLevelRange, formatMatchDate, formatTimeRange, formatVnd } from "../lib/format";
import { fill } from "../lib/text";

const t = messages.match;

type Props = {
  match: MatchView;
  /** The current roster, when known, to show how many have joined. */
  roster?: RosterView | null;
  /** After kickoff there is no point in advertising open places. */
  started?: boolean;
};

export function MatchCard({ match, roster, started = false }: Props) {
  const joined = roster ? roster.teams.reduce((sum, team) => sum + team.players.length, 0) : null;
  const left = joined === null ? null : Math.max(0, match.slotCount - joined);
  // The roster is fresher than the cached match, so either may report the cancel first.
  const cancelled = match.cancelledAt !== null || roster?.cancelled === true;
  return (
    <article className="match-card">
      <header className="match-card__hero">
        <div className="match-card__tags">
          <span className="tag tag--dark">{t.matchType[match.matchType]}</span>
          {cancelled ? <span className="tag tag--alert">{t.cancelled}</span> : null}
          {!cancelled && !started && left !== null ? (
            <span className="tag tag--light">{left > 0 ? fill(t.placesLeft, { count: left }) : t.matchFull}</span>
          ) : null}
        </div>
        <p className="match-card__date">
          {formatMatchDate(match.startsAt)} · <span>{t.format[match.format]}</span>
        </p>
        <p className="match-card__time">{formatTimeRange(match.startsAt, match.endsAt)}</p>
        <h1 className="match-card__venue">{match.venueName}</h1>
        <p className="match-card__fee">
          {t.totalFee} {formatVnd(match.totalFeeVnd)}
        </p>
        <dl className="match-card__stats">
          <div>
            <dt>{t.pricePerPlayer}</dt>
            <dd data-testid="match-price">{formatVnd(match.pricePerPlayerVnd)}</dd>
          </div>
          <div>
            <dt>{t.level}</dt>
            <dd>{formatLevelRange(match.levelMin, match.levelMax)}</dd>
          </div>
          {joined === null ? (
            <div>
              <dt>{t.slots}</dt>
              <dd>{`${match.slotCount} ${t.slotsUnit}`}</dd>
            </div>
          ) : (
            <div>
              <dt>{t.joinedLabel}</dt>
              <dd data-testid="match-joined">{`${joined}/${match.slotCount}`}</dd>
            </div>
          )}
        </dl>
      </header>
    </article>
  );
}
