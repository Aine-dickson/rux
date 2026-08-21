"""Validate site-absolute internal links and their anchors.

Zola does not do this. It validates `@/page.md#anchor` links, and treats an
absolute `/reference/css/#honored-css` as an ordinary URL it has no opinion
about. Every generated page uses the absolute form, because `sync-docs.sh`
rewrites `./05-as-built.md` into a URL rather than into a Zola link.

The gap is not theoretical. Eleven dead anchors into `/why/` shipped and stayed
live: the docs linked to headings in `01-rationale.md`, the rewriter pointed
them at `why.md`, and `why.md` has none of those headings. `zola build`
reported success the whole time.

    python site/check-links.py

Exit 0 when every link resolves, 1 otherwise, listing each break.
"""

import os
import re
import sys
import glob

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "content")


def slugify(heading: str) -> str:
    """Zola's heading anchors, which are not GitHub's.

    Zola replaces every run of non-alphanumerics with a single hyphen, so
    `don't` becomes `don-t`. GitHub drops the punctuation instead, giving
    `dont`. Getting this wrong makes the checker agree with the docs and
    disagree with the built site, which is worse than not checking: it was a
    hand-rolled GitHub-style slug here that let the Law 4 link pass while Zola
    rendered a different id.
    """
    # Inline markdown is not part of the rendered text of a heading.
    text = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", heading)
    text = text.replace("`", "").replace("*", "").replace("_", "")
    slug = re.sub(r"[^a-z0-9]+", "-", text.strip().lower())
    return slug.strip("-")


def url_for(path: str) -> str:
    rel = os.path.relpath(path, ROOT).replace(os.sep, "/")
    if rel == "_index.md":
        return "/"
    if rel.endswith("_index.md"):
        return "/" + rel[: -len("_index.md")]
    return "/" + rel[:-3] + "/"


def main() -> int:
    pages = {}
    for path in glob.glob(os.path.join(ROOT, "**", "*.md"), recursive=True):
        body = open(path, encoding="utf-8").read()
        # Front matter is TOML between +++ fences, and contains no headings.
        body = body.split("+++", 2)[-1]
        # A `#` inside a fenced code block is not a heading.
        body = re.sub(r"^```.*?^```", "", body, flags=re.S | re.M)
        pages[url_for(path)] = {slugify(h) for h in re.findall(r"^#{1,6}\s+(.+)$", body, re.M)}

    broken = []
    for path in sorted(glob.glob(os.path.join(ROOT, "**", "*.md"), recursive=True)):
        body = open(path, encoding="utf-8").read()
        rel = os.path.relpath(path, ROOT).replace(os.sep, "/")
        for link in re.findall(r"\]\((/[^)\s]*)\)", body):
            target, _, anchor = link.partition("#")
            if not target.endswith("/"):
                target += "/"
            if target not in pages:
                broken.append((rel, link, "no such page"))
            elif anchor and anchor not in pages[target]:
                broken.append((rel, link, "no such anchor"))
        # `@/` links are Zola's own and it does validate them, but only for the
        # file; a typo'd anchor still passes there, so both are checked here.
        for link in re.findall(r"\]\(@/([^)\s]+)\)", body):
            rel_target, _, anchor = link.partition("#")
            if not os.path.exists(os.path.join(ROOT, rel_target)):
                broken.append((rel, "@/" + link, "no such file"))
            elif anchor:
                url = url_for(os.path.join(ROOT, rel_target))
                if anchor not in pages.get(url, set()):
                    broken.append((rel, "@/" + link, "no such anchor"))

    # A link written as `https://ruxlang.dev/learn/` leaves the site it is on.
    # On a local `zola serve` it jumps to production, so a page cannot be
    # reviewed before it ships, and on a preview build it hides the change being
    # previewed. Absolute-to-self links are never what was meant.
    site_root = os.path.dirname(os.path.abspath(ROOT))
    for path in sorted(
        glob.glob(os.path.join(ROOT, "**", "*.md"), recursive=True)
        + glob.glob(os.path.join(site_root, "templates", "**", "*.html"), recursive=True)
    ):
        rel = os.path.relpath(path, site_root).replace(os.sep, "/")
        for m in re.finditer(r"https://ruxlang\.dev(/[^\s\"')]*)?", open(path, encoding="utf-8").read()):
            # config.toml's base_url is the one place it belongs, and that file
            # is not scanned here.
            broken.append((rel, m.group(0), "links to production; use a root-relative path"))

    for source, link, why in broken:
        print(f"{source}: {link} ({why})", file=sys.stderr)

    if broken:
        print(
            f"\n{len(broken)} broken internal link(s). "
            "Zola does not catch these; see the note at the top of this file.",
            file=sys.stderr,
        )
        return 1
    print(f"{len(pages)} pages, every internal link resolves.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
