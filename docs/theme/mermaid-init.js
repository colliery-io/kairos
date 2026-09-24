// Render ```mermaid blocks (KAIROS-T-0185).
//
// Client-side rather than through an mdbook-mermaid preprocessor, following
// colliery-io/brokkr: the book then builds with nothing but mdBook, which is
// what .github/workflows/docs.yml installs.
//
// mdBook emits a fenced block as <pre><code class="language-mermaid">, which
// mermaid does not recognise, so rewrite those into <pre class="mermaid">
// before initialising.
(() => {
  const blocks = document.querySelectorAll("pre > code.language-mermaid");
  for (const code of blocks) {
    const pre = code.parentElement;
    const host = document.createElement("pre");
    host.className = "mermaid";
    host.textContent = code.textContent;
    pre.replaceWith(host);
  }
  if (!blocks.length) return;

  // Follow the book's light/dark choice rather than picking one: mdBook puts
  // its theme on <html>, and "ayu"/"coal"/"navy" are the dark ones.
  const cls = document.documentElement.className || "";
  const dark = /\b(ayu|coal|navy)\b/.test(cls);
  mermaid.initialize({
    startOnLoad: true,
    theme: dark ? "dark" : "default",
    securityLevel: "strict",
  });
})();
