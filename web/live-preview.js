/**
 * Live preview while a block is edited: a layer under its field shows the same
 * Markdown with the markers dimmed and the text they mark styled. The field's
 * own text is transparent above it, so the cursor, selection, typing and undo
 * stay the field's. Styles only change colour, background and lines, never the
 * width of letters, so the layer and the field stay aligned.
 */

const INLINE = [
  // [pattern, class of the marked text]; markers are group 1 and 3.
  [/^(`)([^`\n]+)(`)/, "lp-code"],
  [/^(\*\*|__)(?=\S)(.+?)(?<=\S)(\1)/, "lp-bold"],
  [/^(~~)(?=\S)(.+?)(?<=\S)(~~)/, "lp-strike"],
  [/^(==)(?=\S)(.+?)(?<=\S)(==)/, "lp-highlight"],
  [/^(\{\{c\d{1,3}::)(.+?)(\}\})/, "lp-highlight"],
  [/^(\*|_)(?=\S)([^*_\n]+?)(?<=\S)(\1)(?![*_])/, "lp-italic"],
];
const LINK = /^\[([^\]\n]+)\]\(([^)\s]+)\)/;
const mark = (s, esc) => `<span class="lp-mark">${esc(s)}</span>`;

/** One line of inline Markdown as preview HTML. */
function inline(text, esc) {
  let out = "";
  let plain = "";
  const flush = () => {
    out += esc(plain);
    plain = "";
  };
  for (let i = 0; i < text.length;) {
    const rest = text.slice(i);
    const link = rest[0] === "[" && rest.match(LINK);
    if (link) {
      flush();
      out += `${mark("[", esc)}<span class="lp-link">${esc(link[1])}</span>${mark(`](${link[2]})`, esc)}`;
      i += link[0].length;
      continue;
    }
    const found = INLINE.map(([re, cls]) => [rest.match(re), cls]).find(
      ([m]) => m,
    );
    // Markers only open after a space or punctuation, as in Markdown.
    const opens = i === 0 || /[\s([{>"'-]/.test(text[i - 1]);
    if (found && (opens || found[1] === "lp-code")) {
      const [m, cls] = found;
      flush();
      out += `${mark(m[1], esc)}<span class="${cls}">${esc(m[2])}</span>${mark(m[3], esc)}`;
      i += m[0].length;
      continue;
    }
    plain += text[i];
    i++;
  }
  flush();
  return out;
}

/** The preview of a block's Markdown, line by line. */
export function previewHTML(markdown, esc) {
  let fence = false;
  const lines = markdown.split("\n").map((line) => {
    if (/^\s*(```|~~~)/.test(line)) {
      fence = !fence;
      return mark(line, esc);
    }
    if (fence) return `<span class="lp-code">${esc(line)}</span>`;
    const lead = line.match(
      /^(\s*)(#{1,6} |> ?|- \[[ xX]\] |[-*+] |\d+[.)] )?/,
    );
    const [all, space, marker = ""] = lead;
    // A callout's kind, in its colour as in its rendered title.
    const kind =
      /^>/.test(marker) && line.slice(all.length).match(/^\[![a-z]+\]/i);
    const body = kind
      ? `<span class="lp-callout-kind">${esc(kind[0])}</span>${inline(line.slice(all.length + kind[0].length), esc)}`
      : inline(line.slice(all.length), esc);
    return (
      esc(space) +
      (marker ? mark(marker, esc) : "") +
      (/^#/.test(marker) ? `<span class="lp-heading">${body}</span>` : body)
    );
  });
  // A trailing newline still takes a line, as in the field.
  return lines.join("\n") + "​";
}

const CALLOUTS = ["note", "tip", "important", "warning", "caution"];
/**
 * The frame a block is edited in, as it is rendered: `quote` when every line is
 * quoted, a callout's kind (`note`, `tip`…) when it starts with `> [!KIND]`,
 * else `""`.
 */
export function frameOf(markdown) {
  const lines = markdown.split("\n").filter((l) => l.trim());
  if (!lines.length || !lines.every((l) => /^\s*>/.test(l))) return "";
  const kind = lines[0].match(/^\s*>\s*\[!(\w+)\]/)?.[1]?.toLowerCase();
  return kind && CALLOUTS.includes(kind) ? kind : "quote";
}

/** The heading level (1–3) of a block whose first line is a heading, else 0. */
export function headingLevel(markdown) {
  const m = markdown.match(/^\s*(#{1,3}) /);
  return m ? m[1].length : 0;
}

/**
 * Adds the preview under the field `ta`; it follows the field's text until the
 * field leaves the page.
 */
export function attachPreview(ta, esc) {
  const layer = document.createElement("div");
  layer.className = "live-preview";
  layer.setAttribute("aria-hidden", "true");
  const wrap = document.createElement("div");
  wrap.className = "live-preview-wrap";
  ta.replaceWith(wrap);
  wrap.append(layer, ta);
  ta.classList.add("live-preview-field");
  // The field's text is transparent, and so would be a `currentColor` cursor:
  // the cursor takes the block's own text colour.
  ta.style.caretColor = getComputedStyle(wrap).color;
  let shown = null;
  let frame = "";
  const body = wrap.closest(".block-body");
  // The text's size or width changed: the field fits it again.
  const refit = () => {
    ta.style.height = "auto";
    ta.style.height = ta.scrollHeight + "px";
  };
  const draw = () => {
    if (!ta.isConnected) {
      body?.classList.remove("lp-h1", "lp-h2", "lp-h3");
      return;
    }
    if (ta.value !== shown) {
      shown = ta.value;
      layer.innerHTML = previewHTML(shown, esc);
      const level = headingLevel(shown);
      const current =
        [1, 2, 3].find((n) => body?.classList.contains(`lp-h${n}`)) || 0;
      if (body && current !== level) {
        for (const n of [1, 2, 3])
          body.classList.toggle(`lp-h${n}`, n === level);
        refit();
      }
      // Quotes and callouts are edited inside their frame (style.css).
      const now = frameOf(shown);
      if (now !== frame) {
        frame = now;
        wrap.className = `live-preview-wrap${
          frame === "quote"
            ? " lp-quote"
            : frame
              ? ` lp-callout markdown-alert-${frame}`
              : ""
        }`;
        refit();
      }
    }
    requestAnimationFrame(draw);
  };
  draw();
}
