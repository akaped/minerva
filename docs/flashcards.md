# Flashcards

Open **Flashcards** in the sidebar. Make a deck for a subject, book, course, or project. Add a clear question on the front and a concise answer on the back. Cards are plain text, with a few additions: `[label](https://…)` becomes a link, `![](https://…)` a picture, and pictures and sounds added to the card show with it. Other HTML and Markdown are shown literally.

## Make your own cards

Choose **Create a deck**, give it a name, then **Add card**. You can edit both sides, add source links, pause a card, or delete it. **Add picture or sound**, under each side, puts a picture (PNG, JPEG, GIF or WebP, up to 10 MB) or a sound (MP3, M4A, Ogg, WAV, WebM or FLAC, up to 20 MB) where the cursor is, and **Record sound** records one from your microphone; a sound plays with a player, for pronunciation or music. A deck holds up to 200 MB of them, private to you like the deck. Decks and progress are private to your account on this Minerva instance. They are not shared with a workspace or synchronized through workspace replication. Personal desktop decks are stored locally; manual study works offline there.

**Export JSON** downloads the deck, its cards, sources, and scheduling state. JSON exports cannot be imported back yet. Back up the Minerva database to preserve all decks.

## Import from Anki

On the decks page, choose **Import from Anki** and pick a file:

- **An Anki package (`.apkg`)**: in Anki, choose File → Export → _Anki Deck Package_. Packages from every current Anki version work, including the compressed format of Anki 2.1.50 and later. A collection backup (`.colpkg`) works too.
- **A text file**: Anki's _Notes in Plain Text_ export, or any tab-, comma-, or semicolon-separated file with the question in the first column and the answer in the second. Anki's `#separator`, `#html`, and `#deck column` headers are understood.

Each Anki deck becomes a deck here; subdecks keep their path, such as _Spanish › Verbs_. Decks with more than 2,000 cards are split into parts. Every card is built from its note type's templates, so reversed cards, optional fields, and cloze deletions come out as in Anki: on a cloze question, the hidden part shows as `[...]` or its hint. Parts of a template that Anki hides with styles or scripts, such as a hidden copy of the cloze text or a "Show all" button, are left out.

Pictures, sounds and links come along: export **with media** to keep them (pictures up to 10 MB each, sounds up to 20 MB, 100 MB per import). Each deck stores its own copy, visible only to you, and deleting the deck deletes them. Other formatting becomes plain text; a picture or sound missing from the package shows as `[image]` or `[audio]`.

With **Keep my review progress** (the default), cards keep their intervals, due dates, review and lapse counts, and paused state, and their **review history** comes along: each card's memory is rebuilt from it, it counts in your statistics, and it is enough to fit the scheduler to you right away. Otherwise every card starts as new. Tags and Anki's own scheduling settings are not imported.

Files can be up to 100 MB and 20,000 cards.

## Create with AI

Inside a deck, choose **Create with AI**, pick a provider and a source:

- **A note:** questions based on a note you can read.
- **A folder:** questions based on readable notes in that folder and its readable subfolders.
- **Research a topic on the web:** questions based on web research. This requires an OpenAI or Anthropic provider profile with web search enabled by your administrator.

Request 1–20 cards. Inspect the source links, edit the drafts, deselect unwanted cards, then choose **Save selected cards**. Nothing is saved until you do. Source links identify the material used; they do not guarantee that an AI answer is correct. Saved cards are independent copies, so changes or removal of a source note do not update them automatically.

Generation sends selected material to the configured provider and may incur charges. Note/folder generation does not use web search. Topic generation rejects results without actual provider search evidence or with unsupported source links. Folder inputs are limited to 40 notes / 60 KB; choose a smaller source if needed. There is one active generation per user, two per server process, and a limit of 20 attempts per user per hour (reset when the server restarts).

The implementation uses the provider's documented [OpenAI web search sources](https://developers.openai.com/api/docs/guides/tools-web-search) or [Anthropic search results and citations](https://platform.claude.com/docs/en/agents-and-tools/tool-use/web-search-tool). Compatible providers can generate cards from notes and folders, but cannot use this research mode.

## Cards in your notes

A deck can follow a workspace, or a folder with its subfolders: choose **Follow the cards written in my notes** when you create it. Its cards are the ones written in the notes you can open there:

- **A question tagged `#card`**, answered by the blocks indented under it:

  ```
  - What is the capital of Italy? #card
    - Rome
  ```

- **`Question :: Answer #card`** in one block.
- **Cloze deletions**, as in Anki: `{{c1::Rome}} is the capital of {{c2::Italy::country}}`. Each number becomes a card that hides that part (`[...]`, or the hint after the second `::`). In the note, the hidden parts show highlighted.

The deck keeps up with your notes each time you open it: new cards appear, changed ones are updated and keep their review history, and cards whose block was deleted, or whose note you can no longer open, go away. Change such a card in its note; in the deck you can only pause it. You can still add cards of your own to the deck.

## Study and spaced repetition

Choose **Study due cards**. Try to answer before choosing **Reveal answer**, then rate your recall: **Again**, **Hard**, **Good** or **Easy**. Each button shows when the card would come back.

Minerva schedules reviews with [FSRS](https://github.com/open-spaced-repetition/fsrs4anki/wiki/The-Algorithm) (version 4.5), the scheduler Anki uses. Each card has a memory: its _stability_ (how many days until you would recall it with 90% probability) and its _difficulty_. Every rating updates both, taking into account how long it has been since the last review, and the card comes back when your recall is expected to drop to your **desired retention** (90% unless you choose otherwise). With the default weights, a new card answered Good returns after 4 days, Easy after 14; Again brings it back in 10 minutes. Intervals are capped at 10 years.

**Scheduling**, on the decks page, sets the desired retention between 70% and 97%: higher means more reviews and less forgetting, lower the reverse. It applies to reviews from then on. **Optimize for my memory** fits FSRS's 17 weights to your own review history, as Anki's optimizer does: it needs at least 400 reviews of cards studied more than once, on different days, and takes a few seconds. The fitted weights are kept only if they predict your recall better than the defaults; the page shows the prediction error before and after. **Back to the defaults** undoes it.

**Statistics**, on the decks page or in a deck, shows reviews per day over 30 days, your true retention (how often you recalled cards studied on earlier days, against your target), your streak of study days, the recall expected for your cards now, how many cards are new, learning (intervals under 21 days), mature or paused, and how many come due in each of the next 30 days. Every review is logged for this, and deleting a card or deck deletes its history.

Cards reviewed before FSRS keep their schedule: on their next review, their last interval stands for their stability. Cards imported from Anki with their progress do the same.

Each session reviews the cards due when it starts, once each; return later for cards scheduled again. Cards keep review and lapse counts, and pausing a card excludes it from sessions. The statistics also count the time you spend on each card, from the question to your rating.

## Shared decks

When you create or edit a deck, **Shared with** can give it to a workspace where you can edit (not My Workspace). Everyone who can open the workspace finds it among their decks and studies it **with their own progress**: their reviews, due dates, statistics and paused cards are theirs alone. People who can edit in the workspace add, change and delete its cards and pictures; the others only study, and can pause a card for themselves. A shared deck belongs to the workspace: it stays when the person who made it leaves, does not count towards anyone's 100 decks, and is deleted with the workspace. Moving a personal deck into a workspace keeps your progress; moving it back keeps only yours. A shared deck cannot follow notes, since each person may see different ones.

**Desired retention for this deck**, in the deck's settings, overrides each person's own retention for that deck: useful for an exam deck to aim higher, or a large vocabulary deck to aim lower.

## Limits and privacy

An account can have 100 decks, each with up to 2,000 cards. Questions allow 4,000 UTF-8 bytes and answers 8,000. Deck/card changes carry revisions to prevent simultaneous tabs from silently overwriting edits or counting the same review twice. Review due times use the server clock.

Source permissions are checked before generation and again before returning drafts. Private decks are only available to their owner through the API, including when another account is a server administrator. Administrators with database access can still read stored data, as with other self-hosted content.
