import { test } from "node:test";
import assert from "node:assert/strict";
import { barChart, statsHTML } from "../flashcard-stats.js";
import { rich } from "../flashcards.js";

const esc = (s) =>
  String(s).replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
const day = (i) => `2026-11-${String(i + 1).padStart(2, "0")}`;

test("bars scale to the busiest day and say their values", () => {
  const html = barChart("Reviews", [0, 5, 10], ["a", "b", "c"], esc);
  assert.match(html, /height:0%/);
  assert.match(html, /height:50%/);
  assert.match(html, /height:100%/);
  assert.match(html, /aria-label="c: 10"/);
  assert.match(
    html,
    /<th scope="row">b<\/th><td>5<\/td>/,
    "a table for screen readers",
  );
  assert.match(barChart("None", [0, 0], ["a", "b"], esc), /most in a day: 0/);
});

test("statistics show tiles and both charts", () => {
  const html = statsHTML(
    {
      days: Array.from({ length: 30 }, (_, i) => ({
        day: day(i),
        again: 1,
        hard: 0,
        good: i,
        easy: 0,
      })),
      reviews: 500,
      reviews_30_days: 465,
      retention: 0.87,
      retention_reviews: 300,
      desired_retention: 0.9,
      streak: 4,
      studied_today: true,
      cards: { new: 3, learning: 5, mature: 7, paused: 1, due: 2 },
      expected_recall: 0.91,
      forecast: Array.from({ length: 30 }, () => 2),
    },
    esc,
  );
  assert.match(html, /87%/);
  assert.match(html, /aiming for 90%/);
  assert.match(html, /4 days/);
  assert.match(html, /91%/);
  assert.equal((html.match(/class="fc-chart"/g) || []).length, 2);
  assert.match(
    statsHTML(
      {
        ...JSON.parse(
          JSON.stringify({
            days: [{ day: day(0), again: 0, hard: 0, good: 0, easy: 0 }],
            reviews: 0,
            reviews_30_days: 0,
            retention: null,
            retention_reviews: 0,
            desired_retention: 0.9,
            streak: 0,
            studied_today: false,
            cards: { new: 0, learning: 0, mature: 0, paused: 0, due: 0 },
            expected_recall: null,
            forecast: [0],
          }),
        ),
      },
      esc,
    ),
    /—/,
  );
});

test("cards play their sounds", () => {
  const id = "0b8f5f9e-0000-4000-8000-000000000000";
  assert.match(
    rich(`Listen ![audio](media:${id})`, esc),
    /<audio class="fc-sound" controls preload="none" src="\/api\/v1\/flashcards\/media\//,
  );
  assert.match(rich(`![](media:${id})`, esc), /<img class="fc-picture"/);
});
