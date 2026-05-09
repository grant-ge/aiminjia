"""Check coverage between bid requirement points and outline chapters.

Inputs (passed via stdin as a JSON object with two arrays):
    {
      "requirements": [
        {"id": "R1", "text": "公司须具备 ISO27001 认证"},
        ...
      ],
      "outline": [
        {"level": 1, "title": "公司资质"},
        {"level": 2, "title": "1.1 ISO27001"},
        ...
      ]
    }

Output (stdout JSON):
    {
      "covered": [{"id": "R1", "chapter": "1.1 ISO27001"}, ...],
      "uncovered": [{"id": "R5", "text": "..."}],
      "coverage_rate": 0.83
    }

Coverage heuristic: requirement R is covered if its text shares at least one
token with some chapter title (Chinese characters split per char; English
split on whitespace and lowercased). This is a permissive hint for the LLM
to decide which requirements need explicit chapters — not a strict gate.
"""

import json
import re
import sys


def tokenize(text: str) -> set[str]:
    text = text.lower()
    chinese_chars = set(re.findall(r"[一-鿿]", text))
    english_words = set(re.findall(r"[a-z0-9]+", text))
    return chinese_chars | english_words


def main() -> None:
    raw = sys.stdin.read()
    data = json.loads(raw)
    requirements = data.get("requirements", [])
    outline = data.get("outline", [])

    covered = []
    uncovered = []
    for req in requirements:
        req_tokens = tokenize(req["text"])
        match = None
        for chap in outline:
            chap_tokens = tokenize(chap["title"])
            if len(req_tokens & chap_tokens) >= 1:
                match = chap["title"]
                break
        if match:
            covered.append({"id": req["id"], "chapter": match})
        else:
            uncovered.append(req)

    total = max(len(requirements), 1)
    rate = round(len(covered) / total, 2)
    print(json.dumps(
        {"covered": covered, "uncovered": uncovered, "coverage_rate": rate},
        ensure_ascii=False,
    ))


if __name__ == "__main__":
    main()
