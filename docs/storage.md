# Storage: how it scales, and the way to relational tables and PostgreSQL

Minerva keeps a server's data in memory (`Data` in `lib.rs`), behind one lock, and saves it to SQLite: each entity in its own row (`storage.rs`), note text in an FTS5 index (`search.rs`), versions, comments, attachments and other large or per-person things in their own tables. Every request copies `Data`, works on the copy, and saves what changed in one transaction; a request that fails changes nothing.

This page records what that costs as a server grows, what was done about it, and what a move to relational queries or PostgreSQL would take.

## Measured

`crates/minerva-server/tests/scale.rs` fills a server with notes of a few hundred words and times typical requests (median of 20, in milliseconds, release build, a laptop):

```sh
SCALE=1000,3000,5000 cargo test --release -p minerva-server --test scale -- --ignored --nocapture
```

| Notes | Open a note | Edit a note | Search | Bootstrap (open the app) |
| ----: | ----------: | ----------: | -----: | -----------------------: |
| 1,000 |         2.5 |         4.7 |    9.8 |                       16 |
| 3,000 |        10.2 |        22.1 |   44.0 |                       71 |
| 5,000 |        11.6 |        25.6 |   51.1 |                       86 |

Where the time went, at 3,000 notes:

- **Copying `Data`: about 5 ms per request**, 4 of them for replication's field clocks (one map per entity, copied deeply).
- **Saving: 15–28 ms**, also for searches and rendering, which change nothing: working out what changed (2–4 ms), diffing the stored rows (2–3 ms), and the transaction.
- **Bootstrap**: building every note's summary for the sidebar, which grows with the notes.

## Done

- Field clocks and note documents are shared between copies and copied only where a request changes them (`FieldClocks`, and `collaboration` as an `Arc` of `Arc`s). A request that does not touch them copies nothing of them, and saving skips them when they are the very same map.
- Searching, rendering a block and reading embeds no longer go through saving. The editor renders a block at every change, so this matters most there.
- Working out which notes changed compares note by note in order first, and hashes only what moved.

After these, the same benchmark:

| Notes | Open a note | Edit a note | Search | Bootstrap |
| ----: | ----------: | ----------: | -----: | --------: |
| 1,000 |         1.5 |         3.9 |    9.5 |        22 |
| 3,000 |         3.2 |        10.7 |   21.8 |        62 |
| 5,000 |         5.8 |        19.0 |   34.2 |       108 |

Opening and editing notes take about half the time they did; bootstrap is unchanged, as expected (its cost is building every note's summary, step 2 below).

## Next, in order

1. **Share the notes list the same way** (0.3 ms at 3,000 notes; notes are already shared one by one). After that, a copy of `Data` costs about the same at any size, and what remains is scanning: saving still compares every note, and opening one looks through every note for its backlinks.
2. **Page the bootstrap.** Send the sidebar's notes for the open workspace, and the rest on demand, instead of every note on the server.
3. **Read through SQL.** The rows are already one per entity. With tables per kind (workspaces, folders, notes, blocks, records) and foreign keys, handlers can query what they need (one note and its access, a folder's notes, backlinks by an index of links) instead of scanning `Data`. Then `Data` holds only what must stay in memory (sessions, presence, caches), and requests stop growing with the server. This is the large step: about 15,000 lines of handlers read `Data` today, and they can move one kind at a time, starting with the most used (opening, saving and listing notes, backlinks).

## PostgreSQL

What it would bring:

- More than one app server behind a load balancer, with concurrent writers.
- Managed backups, replicas and point-in-time recovery, which hosting providers offer.
- Room for very large shared servers (hundreds of people).

What it would cost:

- A second storage backend to write and test. SQLite stays for the desktop app and small servers, so every query would exist twice, or go through an abstraction that fits both (sqlx, or SeaQuery).
- Today's design depends on one process holding `Data`. With several app servers, `Data` cannot be held in memory, so step 3 (reading through SQL) must be finished first anyway. Live signals, presence and jobs would also need a shared channel (PostgreSQL `LISTEN/NOTIFY` would do).
- Running one more service for every installation that chooses it.

**Recommendation.** Not yet. For a team on one server, SQLite in WAL mode with `synchronous=FULL` is fast and safe, and the measured costs come from copying and scanning `Data`, not from SQLite. Do steps 1 to 3 first. Once reads go through SQL, PostgreSQL becomes a matter of a second dialect for the same queries, worth adding when someone needs more than one app server.
