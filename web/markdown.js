// A line is a block, except fenced code and contiguous tables/lists: their
// Markdown meaning depends on keeping the lines together.
export function splitMarkdown(markdown) {
  const lines = markdown.replace(/\r\n?/g, "\n").split("\n");
  const blocks = [];
  let fence = null;
  let group = [];
  const flush = () => {
    if (group.length) blocks.push(group.join("\n"));
    group = [];
  };
  for (const line of lines) {
    if (fence) {
      group.push(line);
      const end = line.match(/^ {0,3}(`{3,}|~{3,})\s*$/);
      if (end && end[1][0] === fence[0] && end[1].length >= fence.length) {
        flush();
        fence = null;
      }
      continue;
    }
    const start = line.match(/^ {0,3}(`{3,}|~{3,})/);
    if (start) {
      flush();
      fence = start[1];
      group.push(line);
      continue;
    }
    if (/^\s*\|/.test(line) || /^\s*(?:[-+*]|\d+[.)])\s/.test(line)) {
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
