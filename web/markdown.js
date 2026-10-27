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
  ["Text", "", "paragraph"],
  ["Heading 1", "# ", "title"],
  ["Heading 2", "## ", "section"],
  ["Heading 3", "### ", "subsection"],
  ["Task", "- [ ] ", "checkbox todo"],
  ["Bullet list", "- ", "unordered"],
  ["Numbered list", "1. ", "ordered"],
  ["Quote", "> ", "callout"],
  ["Code", "```rust\n// Your code here\n```", "fenced programming"],
  ["Divider", "---", "horizontal rule"],
  ["Table", "| Column | Column |\n| --- | --- |\n| Value | Value |", "grid"],
  ["Link", "[Link text](https://example.com)", "url"],
  ["Image or file", null, "attachment upload photo"],
  ["Record audio", "@record", "/record voice microphone memo sound"],
  ["Database (live)", "@database", "table board tasks records todo embed"],
  ["Callout · Note", "> [!NOTE]\n> ", "callout info alert admonition"],
  ["Callout · Tip", "> [!TIP]\n> ", "callout hint idea admonition"],
  ["Callout · Warning", "> [!WARNING]\n> ", "callout caution alert admonition"],
  [
    "Math (block)",
    "$$\n\\int_0^1 x^2\\,dx\n$$",
    "equation formula latex tex katex",
  ],
  [
    "Diagram (Mermaid)",
    "```mermaid\ngraph TD\n  A[Idea] --> B[Plan]\n  B --> C[Done]\n```",
    "flowchart chart graph sequence gantt",
  ],
  ["Highlight", "==highlighted text==", "marker mark color"],
  ["Link to note", "[[", "wiki backlink mention reference recall"],
  [
    "Assign to someone",
    "@assign",
    "/assigntouser assign user person mention email notify",
  ],
  ["Remind me", "@remind", "/remindme reminder date time alarm email notify"],
  [
    "Unfinished journal tasks",
    "[[uncompleted_journal_tasks]]",
    "todo carry over open tasks journal daily",
  ],
];
