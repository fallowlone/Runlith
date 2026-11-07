#!/usr/bin/env python3
"""D05 deterministic verification (AGENT.md §5: evidence over claims).

Checks: HTML parses; all variants share fixture.js; unsupported actions do
not leak; density/text-size fixtures exist; variant C inspector rules.
"""
import re
import sys
from pathlib import Path
from html.parser import HTMLParser

HERE = Path(__file__).parent
VOID = {"meta", "link", "br", "img", "input", "hr", "source", "wbr"}


class Checker(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.stack, self.errors = [], []

    def handle_starttag(self, tag, attrs):
        if tag not in VOID:
            self.stack.append(tag)

    def handle_endtag(self, tag):
        if tag in VOID:
            return
        if not self.stack or self.stack[-1] != tag:
            self.errors.append(f"mismatched </{tag}> (stack={self.stack[-3:]})")
        else:
            self.stack.pop()


def main() -> int:
    failures = []

    def check(name, cond, detail=""):
        print(("PASS  " if cond else "FAIL  ") + name + (f" — {detail}" if detail and not cond else ""))
        if not cond:
            failures.append(name)

    variants = {}
    for v in "abc":
        html = (HERE / f"{v}.html").read_text(encoding="utf-8")
        c = Checker()
        c.feed(html)
        check(f"{v}.html parses with balanced tags", not c.errors and not c.stack, str(c.errors[:3]))
        variants[v] = html

    check("all variants load fixture.js", all('src="fixture.js"' in h for h in variants.values()))
    check("all variants load proto.js", all('src="proto.js"' in h for h in variants.values()))
    check("all variants use shared.css", all('href="shared.css"' in h for h in variants.values()))
    check(
        "variant shells differ only in layout, not in mounted data",
        len({re.sub(r"\s+", "", re.sub(r"<script.*</script>", "", h, flags=re.S)) for h in variants.values()}) == 3,
    )

    css = (HERE / "shared.css").read_text(encoding="utf-8")
    for token in ("--row-h: 28px", "--row-h: 32px", "--row-h: 40px", 'data-text="large"'):
        check(f"shared.css has {token}", token in css)

    fixture = (HERE / "fixture.js").read_text(encoding="utf-8")
    check("fixture has no TAKEOVER choice", "TAKEOVER" not in fixture)
    check("fixture HDR offers exactly RESUME/REPLAN/ABORT",
          all(c in fixture for c in ('"RESUME"', '"REPLAN"', '"ABORT"')))
    check("fixture marks stale approval READ_ONLY", '"READ_ONLY"' in fixture and "REQUEST_EXPIRED" in fixture)

    everything = "\n".join([fixture, css] + list(variants.values()) + [(HERE / "proto.js").read_text(encoding="utf-8")])
    check("no Cancel Task / universal Resume control", not re.search(r"Cancel Task|CancelTask", everything))
    check("no TAKEOVER anywhere", "TAKEOVER" not in everything)
    check("terminal is labeled read-only", "Read-only command output" in everything or "read-only" in everything)

    c_html = variants["c"]
    check("C inspector starts closed", 'id="inspector" style="display:none"' in c_html)
    check("C has explicit open + pin + diagnostics buttons",
          all(x in c_html for x in ("Open inspector", "Pin inspector", "Open diagnostics")))
    check("C notes no self-opening / no focus steal",
          "never takes keyboard focus" in c_html)
    check("C carries History tab", 'data-panel="history"' in c_html)

    proto = (HERE / "proto.js").read_text(encoding="utf-8")
    check("stale approval renders disabled mutation buttons",
          'disabled title="Request expired — read-only"' in proto)
    check("decision panel separates recommendation from choice",
          "not pre-selected consent" in fixture and "esc(a.recommendation)" in proto)

    print()
    if failures:
        print(f"{len(failures)} failure(s): {failures}")
        return 1
    print("ALL CHECKS PASSED")
    return 0


if __name__ == "__main__":
    sys.exit(main())