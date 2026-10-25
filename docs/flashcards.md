# Flashcards

Open **Flashcards** in the sidebar. Make a deck for a subject, book, course, or project. Add a clear question on the front and a concise answer on the back. Cards use plain text, so pasted HTML and Markdown are shown literally.

## Make your own cards

Choose **Create a deck**, give it a name, then **Add card**. You can edit both sides, add source links, pause a card, or delete it. Decks and progress are private to your account on this Minerva instance. They are not shared with a workspace or synchronized through workspace replication. Personal desktop decks are stored locally; manual study works offline there.

**Export JSON** downloads the deck, its cards, sources, and scheduling state. This release does not import Anki packages or JSON exports. Back up the Minerva database to preserve all decks.

## Create with AI

Inside a deck, choose **Create with AI**, pick a provider and a source:

- **A note:** questions based on a note you can read.
- **A folder:** questions based on readable notes in that folder and its readable subfolders.
- **Research a topic on the web:** questions based on web research. This requires an OpenAI or Anthropic provider profile with web search enabled by your administrator.

Request 1–20 cards. Inspect the source links, edit the drafts, deselect unwanted cards, then choose **Save selected cards**. Nothing is saved until you do. Source links identify the material used; they do not guarantee that an AI answer is correct. Saved cards are independent copies, so changes or removal of a source note do not update them automatically.

Generation sends selected material to the configured provider and may incur charges. Note/folder generation does not use web search. Topic generation rejects results without actual provider search evidence or with unsupported source links. Folder inputs are limited to 40 notes / 60 KB; choose a smaller source if needed. There is one active generation per user, two per server process, and a limit of 20 attempts per user per hour (reset when the server restarts).

The implementation uses the provider's documented [OpenAI web search sources](https://developers.openai.com/api/docs/guides/tools-web-search) or [Anthropic search results and citations](https://platform.claude.com/docs/en/agents-and-tools/tool-use/web-search-tool). Compatible providers can generate cards from notes and folders, but cannot use this research mode.

## Study and spaced repetition

Choose **Study due cards**. Try to answer before choosing **Reveal answer**, then rate your recall:

| Rating | Next interval |
| --- | --- |
| Again | 10 minutes |
| Hard | 1 day initially; then the previous interval × 1.2 |
| Good | 1 day initially; then the previous interval × 2.5 |
| Easy | 4 days initially; then the previous interval × 3.5 |

Intervals are capped at 365 days. A card repeated after an Again rating starts again from the initial day intervals. Each session reviews the cards due when it starts, once each; return later for cards scheduled again. The UI shows the next interval before you rate. Cards retain review and lapse counts, and pausing a card excludes it from sessions.

This is Minerva's simple interval scheduler, not Anki's FSRS algorithm. This first version supports question/answer cards; cloze deletion, media cards, shared decks, Anki import, and detailed study analytics are not implemented.

## Limits and privacy

An account can have 100 decks, each with up to 2,000 cards. Questions allow 4,000 UTF-8 bytes and answers 8,000. Deck/card changes carry revisions to prevent simultaneous tabs from silently overwriting edits or counting the same review twice. Review due times use the server clock.

Source permissions are checked before generation and again before returning drafts. Private decks are only available to their owner through the API, including when another account is a server administrator. Administrators with database access can still read stored data, as with other self-hosted content.
