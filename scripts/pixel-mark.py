#!/usr/bin/env python3
"""Draws Omalogi's pixel mark and wordmark as an animated SVG, for the README.

  scripts/pixel-mark.py > docs/images/omalogi.svg

The mouse is the same 16 × 16 sprite as plugin/PixelMark.qml. Colours follow the
reader's light or dark theme, and the wheel and cable stay still for reduced motion.
"""

# k outline, s scroll wheel; the top cable pixel sways, so row 0 is drawn separately.
MOUSE = [
    "................",
    ".......k........",
    ".......k........",
    ".....kkkkk......",
    "....k..k..k.....",
    "...k...s...k....",
    "...k...s...k....",
    "...k...k...k....",
    "...kkkkkkkkk....",
    "...k.......k....",
    "...k.......k....",
    "...k.......k....",
    "...k.......k....",
    "....k.....k.....",
    ".....kkkkk......",
    "................",
]

# Lowercase on a 9-row grid: rows 0-1 ascender, 2-6 x-height, 7-8 descender.
GLYPHS = {
    "o": ["....", "....", ".##.", "#..#", "#..#", "#..#", ".##.", "....", "...."],
    "m": [".....", ".....", "##.#.", "#.#.#", "#.#.#", "#.#.#", "#.#.#", ".....", "....."],
    "a": ["....", "....", ".##.", "...#", ".###", "#..#", ".###", "....", "...."],
    "l": ["#.", "#.", "#.", "#.", "#.", "#.", ".#", "..", ".."],
    "g": ["....", "....", ".###", "#..#", "#..#", "#..#", ".###", "...#", ".##."],
    "i": ["#", ".", "#", "#", "#", "#", "#", ".", "."],
}
WORD = "omalogi"
SCALE = 8
WORD_X = 18
WORD_Y = 5


def rect(x, y, cls="ink"):
    return f'<rect class="{cls}" x="{x}" y="{y}" width="1" height="1"/>'


def main():
    cells = []
    for y, row in enumerate(MOUSE):
        for x, ch in enumerate(row):
            if ch == "k":
                cells.append(rect(x, y))
            elif ch == "s":
                cells.append(rect(x, y, f"wheel r{y % 2}"))
    cells.append(rect(8, 0, "ink cable"))

    x = WORD_X
    for letter in WORD:
        glyph = GLYPHS[letter]
        for y, row in enumerate(glyph):
            for dx, ch in enumerate(row):
                if ch == "#":
                    cells.append(rect(x + dx, WORD_Y + y))
        x += len(glyph[0]) + 1
    width = x - 1

    print(f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} 16" width="{width * SCALE}" height="{16 * SCALE}" shape-rendering="crispEdges" role="img" aria-label="Omalogi">
<style>
  .ink {{ fill: #171A21; }}
  .wheel {{ fill: #15917F; }}
  @media (prefers-color-scheme: dark) {{
    .ink {{ fill: #E4E7EC; }}
    .wheel {{ fill: #3CC7B5; }}
  }}
  .r0 {{ animation: roll 0.26s step-end infinite; }}
  .r1 {{ animation: roll 0.26s step-end infinite reverse; }}
  .cable {{ animation: sway 1.8s step-end infinite; }}
  @keyframes roll {{ 0% {{ opacity: 1; }} 50% {{ opacity: 0.35; }} }}
  @keyframes sway {{ 0% {{ transform: translateX(0); }} 50% {{ transform: translateX(-2px); }} }}
  @media (prefers-reduced-motion: reduce) {{
    .r0, .r1, .cable {{ animation: none; }}
    .r1 {{ opacity: 0.35; }}
  }}
</style>
{chr(10).join(cells)}
</svg>''')


if __name__ == "__main__":
    main()
