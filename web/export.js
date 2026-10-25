// Prepares an exported note for printing: renders diagrams, waits for images and
// fonts, then tells the app it is ready (or prints when opened on its own).
(async () => {
  const codes = [...document.querySelectorAll("pre > code.language-mermaid")];
  if (codes.length) {
    try {
      await new Promise((resolve, reject) => {
        const script = document.createElement("script");
        script.src = "/mermaid.js";
        script.onload = resolve;
        script.onerror = reject;
        document.head.append(script);
      });
      const mermaid = window.minervaMermaid;
      mermaid.initialize({
        startOnLoad: false,
        securityLevel: "strict",
        layout: "dagre",
        theme: "default",
      });
      for (const code of codes) {
        const id = "diagram-" + Math.random().toString(36).slice(2);
        try {
          const { svg } = await mermaid.render(id, code.textContent);
          const figure = document.createElement("div");
          figure.className = "mermaid-diagram";
          figure.innerHTML = svg;
          code.parentElement.replaceWith(figure);
        } catch {
          // Keep the diagram source; remove Mermaid's error graphic.
          document.getElementById("d" + id)?.remove();
          document.getElementById(id)?.remove();
        }
      }
    } catch {
      // Diagrams unavailable; the source remains readable.
    }
  }
  await Promise.all(
    [...document.images].map((img) =>
      img.complete
        ? null
        : new Promise((resolve) => {
            img.onload = img.onerror = resolve;
          }),
    ),
  );
  await document.fonts?.ready;
  if (window.parent !== window)
    window.parent.postMessage(
      { type: "minerva-export-ready" },
      location.origin,
    );
  else window.print();
})();
