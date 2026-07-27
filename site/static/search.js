/* Docs search.
 *
 * Zola builds `search_index.en.json` (elasticlunr_json format) and also drops an
 * `elasticlunr.min.js` next to it. We deliberately load neither library nor its
 * inverted index: we read the `documentStore` and score it ourselves. Over a
 * dozen documents a linear scan is instant, and skipping the 40kB library keeps
 * the page weight honest for a project whose pitch is "no browser stack".
 * Revisit if the docs grow past a few hundred pages.
 *
 * Progressive enhancement: the input is inert until the index loads, and if this
 * script never runs the page is still complete.
 */
(function () {
  "use strict";

  var input = document.getElementById("docs-search-input");
  var list = document.getElementById("docs-search-results");
  if (!input || !list) return;

  var script = document.currentScript || document.querySelector("script[data-search-index]");
  var indexUrl = script && script.getAttribute("data-search-index");
  if (!indexUrl) return;

  var docs = null;

  function load() {
    if (docs) return Promise.resolve(docs);
    return fetch(indexUrl)
      .then(function (r) { return r.json(); })
      .then(function (data) {
        var store = (data.documentStore && data.documentStore.docs) || {};
        docs = Object.keys(store).map(function (url) {
          var d = store[url];
          return {
            url: url,
            title: d.title || url,
            body: d.body || "",
            haystack: ((d.title || "") + " " + (d.body || "")).toLowerCase()
          };
        });
        return docs;
      })
      .catch(function () { docs = []; return docs; });
  }

  // Sections that are documentation rather than narrative. A search box in the
  // docs sidebar should answer "where is this documented", not "which release
  // post mentioned it". Without the boost, long blog posts outrank the
  // reference on its own terms (verified: "pseudo" and "grid" both did).
  var DOCS_SECTIONS = ["/reference/", "/roadmap/", "/contribute/"];

  function occurrences(haystack, term) {
    var n = 0, at = haystack.indexOf(term);
    while (at !== -1) { n++; at = haystack.indexOf(term, at + term.length); }
    return n;
  }

  function score(doc, terms) {
    var title = doc.title.toLowerCase();
    var total = 0;
    for (var i = 0; i < terms.length; i++) {
      var t = terms[i];
      var tf = occurrences(doc.haystack, t);
      if (tf === 0) return 0; // every term must appear
      // A title hit dominates; body hits count by frequency, log-damped so a
      // page saying "grid" 40 times doesn't bury one that explains it properly.
      total += (title.indexOf(t) !== -1 ? 12 : 0) + Math.log(1 + tf) * 3;
    }
    // Normalise by length, or the longest document wins nearly every query.
    var s = (total / Math.sqrt(Math.max(doc.haystack.length, 1))) * 100;
    for (var j = 0; j < DOCS_SECTIONS.length; j++) {
      if (doc.url.indexOf(DOCS_SECTIONS[j]) !== -1) { s *= 1.8; break; }
    }
    return s;
  }

  function snippet(doc, term) {
    var at = doc.body.toLowerCase().indexOf(term);
    if (at === -1) return doc.body.slice(0, 100) + "…";
    var start = Math.max(0, at - 40);
    return (start > 0 ? "…" : "") + doc.body.slice(start, at + 80).trim() + "…";
  }

  function render(results, terms) {
    list.textContent = "";
    if (!results.length) {
      list.hidden = true;
      return;
    }
    results.forEach(function (doc) {
      var li = document.createElement("li");
      var a = document.createElement("a");
      a.href = doc.url;

      var strong = document.createElement("strong");
      strong.textContent = doc.title;
      a.appendChild(strong);

      var span = document.createElement("span");
      span.textContent = snippet(doc, terms[0]);
      a.appendChild(span);

      li.appendChild(a);
      list.appendChild(li);
    });
    list.hidden = false;
  }

  function run() {
    var q = input.value.trim().toLowerCase();
    if (q.length < 2) {
      list.hidden = true;
      list.textContent = "";
      return;
    }
    var terms = q.split(/\s+/);
    load().then(function (all) {
      var hits = all
        .map(function (d) { return { doc: d, s: score(d, terms) }; })
        .filter(function (h) { return h.s > 0; })
        .sort(function (a, b) { return b.s - a.s; })
        .slice(0, 8)
        .map(function (h) { return h.doc; });
      render(hits, terms);
    });
  }

  input.addEventListener("input", run);
  input.addEventListener("focus", load);
  input.addEventListener("keydown", function (e) {
    if (e.key === "Escape") {
      input.value = "";
      list.hidden = true;
      list.textContent = "";
      input.blur();
    }
  });
  // A click anywhere else dismisses the results, but not a click *on* them,
  // that would cancel the navigation before it happened.
  document.addEventListener("click", function (e) {
    if (!list.contains(e.target) && e.target !== input) list.hidden = true;
  });
})();
