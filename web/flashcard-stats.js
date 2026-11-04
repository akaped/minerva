/**
 * Study statistics for the Flashcards page: headline numbers as tiles, and
 * reviews per day and the coming due cards as single-series bar charts, each
 * with a table for screen readers (`GET /api/v1/flashcards/stats`).
 */
import { $t, $tn } from "./i18n.js";

const percent = (x) => `${Math.round(x * 100)}%`;

/** A short date for a chart: "3 Nov". */
function shortDay(iso) {
  const [y, m, d] = iso.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
  });
}

/**
 * Bars for one series: each bar shows its value on hover or focus; the
 * tallest sets the scale. `labels[i]` names bar `i`.
 */
export function barChart(title, values, labels, esc) {
  const top = Math.max(0, ...values);
  const max = Math.max(1, top);
  const bars = values
    .map((v, i) => {
      const tip = `${labels[i]}: ${v}`;
      return `<span class="fc-bar" tabindex="0" role="img" aria-label="${esc(tip)}" data-tip="${esc(tip)}"><span style="height:${v ? Math.max(4, (v / max) * 100) : 0}%"></span></span>`;
    })
    .join("");
  const table = `<table class="sr-only"><caption>${esc(title)}</caption><tbody>${values.map((v, i) => `<tr><th scope="row">${esc(labels[i])}</th><td>${v}</td></tr>`).join("")}</tbody></table>`;
  return `<figure class="fc-chart"><figcaption><strong>${esc(title)}</strong><span>${esc($t("most in a day: {count}", { count: top }))}</span></figcaption><div class="fc-bars">${bars}</div><div class="fc-axis"><span>${esc(labels[0])}</span><span>${esc(labels.at(-1))}</span></div>${table}</figure>`;
}

function tile(label, value, note = "") {
  return `<div class="fc-tile"><span>${label}</span><strong>${value}</strong>${note ? `<small>${note}</small>` : ""}</div>`;
}

/** Seconds as "45 seconds", "12 minutes" or "3.5 hours". */
export function duration(seconds) {
  if (seconds < 90)
    return $tn(Math.round(seconds), "{count} second", "{count} seconds");
  const minutes = Math.round(seconds / 60);
  if (minutes < 90) return $tn(minutes, "{count} minute", "{count} minutes");
  const hours = Math.round((seconds / 3600) * 10) / 10;
  return $tn(hours, "{count} hour", "{count} hours");
}

/** The statistics page's content. */
export function statsHTML(s, esc) {
  const reviewDays = s.days.map((d) => d.again + d.hard + d.good + d.easy);
  const reviewLabels = s.days.map((d) => shortDay(d.day));
  const ahead = s.forecast.map((_, i) =>
    i === 0
      ? $t("Later today")
      : i === 1
        ? $t("Tomorrow")
        : $t("In {count} days", { count: i }),
  );
  const retention =
    s.retention == null
      ? tile(
          $t("True retention"),
          "—",
          esc($t("Shown after reviews of cards studied on earlier days.")),
        )
      : tile(
          $t("True retention"),
          percent(s.retention),
          esc(
            $tn(
              s.retention_reviews,
              "{count} review in 30 days · aiming for {target}",
              "{count} reviews in 30 days · aiming for {target}",
              { target: percent(s.desired_retention) },
            ),
          ),
        );
  const c = s.cards;
  return `<div class="fc-tiles">${tile($t("Reviews in 30 days"), s.reviews_30_days, esc($tn(s.reviews, "{count} review in all", "{count} reviews in all")))}${retention}${tile(
    $t("Streak"),
    $tn(s.streak, "{count} day", "{count} days"),
    esc(s.studied_today ? $t("Studied today") : $t("Not studied yet today")),
  )}${tile(
    $t("Expected recall now"),
    s.expected_recall == null ? "—" : percent(s.expected_recall),
    esc($t("Across the cards you have studied")),
  )}${tile(
    $t("Time studied in 30 days"),
    duration(s.seconds_30_days || 0),
    s.seconds_per_review == null
      ? ""
      : esc(
          $t("{time} per card on average", {
            time: duration(s.seconds_per_review),
          }),
        ),
  )}</div><div class="fc-tiles fc-states">${tile($t("Due now"), c.due)}${tile($t("New"), c.new)}${tile($t("Learning"), c.learning, esc($t("Intervals under 21 days")))}${tile($t("Mature"), c.mature, esc($t("Intervals of 21 days or more")))}${tile($t("Paused"), c.paused)}</div>${barChart($t("Reviews per day"), reviewDays, reviewLabels, esc)}${barChart($t("Cards coming due"), s.forecast, ahead, esc)}`;
}
