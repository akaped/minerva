import { msg } from "./i18n.js";
// A line is a block, except fenced code, display math and contiguous
// tables/lists/quotes: their Markdown meaning depends on keeping the lines
// together (a callout is a quote whose first line is `> [!NOTE]`).
export function splitMarkdown(markdown) {
  const lines = markdown.replace(/\r\n?/g, "\n").split("\n");
  const blocks = [];
  let fence = null;
  let group = [];
  // What the open group holds: "list" (tables and lists) or "quote".
  let kind = null;
  const flush = () => {
    if (group.length) blocks.push(group.join("\n"));
    group = [];
    kind = null;
  };
  for (const line of lines) {
    if (fence) {
      group.push(line);
      const end =
        fence === "$$"
          ? /^\s*\$\$\s*$/.test(line)
          : (() => {
              const m = line.match(/^ {0,3}(`{3,}|~{3,})\s*$/);
              return m && m[1][0] === fence[0] && m[1].length >= fence.length;
            })();
      if (end) {
        flush();
        fence = null;
      }
      continue;
    }
    const start = line.match(/^ {0,3}(`{3,}|~{3,})/);
    if (start || /^\s*\$\$\s*$/.test(line)) {
      flush();
      fence = start ? start[1] : "$$";
      group.push(line);
      continue;
    }
    const quote = /^\s{0,3}>/.test(line);
    const list = /^\s*\|/.test(line) || /^\s*(?:[-+*]|\d+[.)])\s/.test(line);
    if (quote || list) {
      const next = quote ? "quote" : "list";
      if (kind && kind !== next) flush();
      kind = next;
      group.push(line);
      continue;
    }
    flush();
    blocks.push(line);
  }
  flush();
  return blocks;
}

export const blockCommands = [
  [msg("Text"), "", "paragraph"],
  [msg("Heading 1"), "# ", "title"],
  [msg("Heading 2"), "## ", "section"],
  [msg("Heading 3"), "### ", "subsection"],
  [msg("Task"), "- [ ] ", "checkbox todo"],
  [msg("Bullet list"), "- ", "unordered"],
  [msg("Numbered list"), "1. ", "ordered"],
  [msg("Quote"), "> ", "callout"],
  [msg("Code"), "```rust\n// Your code here\n```", "fenced programming"],
  [msg("Divider"), "---", "horizontal rule"],
  [
    msg("Table"),
    "| Column | Column |\n| --- | --- |\n| Value | Value |",
    "grid",
  ],
  [msg("Link"), "[Link text](https://example.com)", "url"],
  [msg("Image or file"), null, "attachment upload photo"],
  [msg("Record audio"), "@record", "/record voice microphone memo sound"],
  [msg("Database (live)"), "@database", "table board tasks records todo embed"],
  [msg("Callout · Note"), "> [!NOTE]\n> ", "callout info alert admonition"],
  [msg("Callout · Tip"), "> [!TIP]\n> ", "callout hint idea admonition"],
  [
    msg("Callout · Warning"),
    "> [!WARNING]\n> ",
    "callout caution alert admonition",
  ],
  [
    msg("Math (block)"),
    "$$\n\\int_0^1 x^2\\,dx\n$$",
    "equation formula latex tex katex",
  ],
  [
    msg("Diagram (Mermaid)"),
    "```mermaid\ngraph TD\n  A[Idea] --> B[Plan]\n  B --> C[Done]\n```",
    "flowchart chart graph sequence gantt",
  ],
  [msg("Highlight"), "==highlighted text==", "marker mark color"],
  [msg("Emoji"), "@emoji", "/emoji smiley icon face symbol"],
  [msg("Toggle"), "@toggle", "collapse fold expand details accordion nested"],
  [msg("Table of contents"), "[[toc]]", "toc outline headings contents index"],
  [msg("Link to note"), "[[", "wiki backlink mention reference recall"],
  [
    msg("Embedded note"),
    "![[",
    "embed transclude include show note page block synced",
  ],
  [
    msg("Assign to someone"),
    "@assign",
    "/assigntouser assign user person mention email notify",
  ],
  [
    msg("Remind me"),
    "@remind",
    "/remindme reminder date time alarm email notify",
  ],
  [
    msg("Unfinished journal tasks"),
    "[[uncompleted_journal_tasks]]",
    "todo carry over open tasks journal daily",
  ],
];

/**
 * Search snippets mark matches with U+0001…U+0002 around plain text: escape the
 * text with `escape`, then turn the markers into highlights.
 */
export function highlightSnippet(snippet, escape) {
  return escape(snippet || "")
    .replace(/\u0001/g, "<mark>")
    .replace(/\u0002/g, "</mark>");
}
