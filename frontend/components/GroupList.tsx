import type { PublicGroup } from "../lib/api";
import messages from "../messages/vi.json";

const t = messages.groups;

/** Monday first, as people say them: "T7, CN". */
function weekdaysText(weekdays: number[]): string {
  return [1, 2, 3, 4, 5, 6, 0]
    .filter((day) => weekdays.includes(day))
    .map((day) => messages.poll.weekdays[day])
    .join(", ");
}

/** How a member answers in the chat, from the group's first days: "T7 có, CN không". */
function answerExample(weekdays: number[]): string {
  return weekdaysText(weekdays)
    .split(", ")
    .slice(0, 2)
    .map((day, index) => `${day} ${index === 0 ? t.yes : t.no}`)
    .join(", ");
}

function headline(group: PublicGroup): string | null {
  const parts = [
    group.sport ? t.sports[group.sport] : null,
    group.format ? t.formats[group.format] : null,
    group.venue?.name ?? null,
  ].filter((part): part is string => part !== null);
  return parts.length > 0 ? parts.join(" · ") : null;
}

/** Weekly groups that chose to show on the home page. No member names. */
export function GroupList({ groups }: { groups: PublicGroup[] }) {
  if (groups.length === 0) {
    return null;
  }
  return (
    <section className="groups" aria-labelledby="groups-title">
      <h2 id="groups-title" className="groups__title">
        {t.title}
      </h2>
      <p className="muted groups__intro">{t.intro}</p>
      <ul className="groups__list">
        {groups.map((group, index) => {
          const top = headline(group);
          const when = [weekdaysText(group.weekdays), group.playTime].filter(Boolean).join(" · ");
          return (
            <li key={index} className="group-card">
              {top ? <p className="group-card__top">{top}</p> : null}
              <p className="group-card__title">{group.title}</p>
              {group.venue ? <p className="muted group-card__address">{group.venue.address}</p> : null}
              <p className="group-card__when">{when}</p>
              <p className="group-card__players">
                {group.week ? t.playersThisWeek.replace("{n}", String(group.week.players)) : t.noPollYet}
              </p>
              {group.weekdays.length > 0 ? (
                <p className="muted group-card__answer">
                  {t.howToAnswer.replace("{example}", answerExample(group.weekdays))}
                </p>
              ) : null}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
