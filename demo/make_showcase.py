"""Draws assets/showcase.svg for the README: the demo's real output without and with the crate.

  cd demo && python3 make_showcase.py      # needs cargo-llvm-cov

Runs the demo's test twice, plain (`cargo test`) and with the crate's numbers and coverage
(`RESIDUAL_RISK=1 cargo llvm-cov --no-report --features coverage -- --nocapture`), keeps the lines
that matter from each output as printed (for the plain run, the test's one line), and draws them side by side.
"""

import html
import os
import pathlib
import subprocess

HERE = pathlib.Path(__file__).resolve().parent
OUT = HERE.parent / "assets" / "showcase.svg"

PLAIN_CMD = ["cargo", "test"]
OURS_CMD = ["cargo", "llvm-cov", "--no-report", "--features", "coverage", "--", "--nocapture"]


def run(cmd, env=None):
    r = subprocess.run(cmd, cwd=HERE, capture_output=True, text=True,
                       env={**os.environ, **(env or {})}, check=True)
    return (r.stdout + r.stderr).expandtabs(4).splitlines()


def keep_plain(lines):
    """The one line a passing test gets."""
    return [next(l for l in lines if l.startswith("test tests::"))]


def keep_ours(lines):
    start = next(i for i, l in enumerate(lines) if l.startswith("proptest: "))
    end = next(i for i, l in enumerate(lines) if l.startswith("test tests::"))
    return lines[start:end + 1]


plain = keep_plain(run(PLAIN_CMD))
ours = keep_ours(run(OURS_CMD, {"RESIDUAL_RISK": "1"}))

# Colours: GitHub's dark terminal palette.
BG, BAR, FG, DIM, PROMPT = "#0d1117", "#161b22", "#e6edf3", "#8b949e", "#7ee787"
MARK = {"failure:": "#56d364", "below ": "#56d364", "coverage:": "#79c0ff", "new code:": "#e3b341"}
CW, LH, FS = 7.8, 19, 13  # character width, line height, font size (px)
PAD, GAP, TOP = 16, 24, 58


def colour(line):
    s = line.strip()
    for k, c in MARK.items():
        if s.startswith(k):
            return c
    return FG


def pane(x, title, cmd_lines, body, width, height):
    out = [f'<g transform="translate({x},0)">',
           f'<text x="0" y="22" class="t">{html.escape(title)}</text>',
           f'<rect x="0" y="34" width="{width}" height="{height}" rx="8" fill="{BG}"/>',
           f'<rect x="0" y="34" width="{width}" height="22" rx="8" fill="{BAR}"/>',
           f'<rect x="0" y="48" width="{width}" height="8" fill="{BAR}"/>']
    for i, c in enumerate(["#ff5f56", "#ffbd2e", "#27c93f"]):
        out.append(f'<circle cx="{14 + 16 * i}" cy="45" r="5" fill="{c}"/>')
    y = TOP + 20
    for i, c in enumerate(cmd_lines):
        prompt = "$ " if i == 0 else "  "  # a continued command has no prompt
        out.append(f'<text x="{PAD}" y="{y}" class="m" xml:space="preserve">'
                   f'<tspan fill="{PROMPT}">{prompt}</tspan>'
                   f'<tspan fill="{FG}">{html.escape(c)}</tspan></text>')
        y += LH
    y += 6
    for l in body:
        weight = ' font-weight="bold"' if colour(l) != FG else ""
        out.append(f'<text x="{PAD}" y="{y}" class="m" fill="{colour(l)}"{weight} '
                   f'xml:space="preserve">{html.escape(l)}</text>')
        y += LH
    out.append("</g>")
    return out


left_w = int(max(len(l) for l in plain + ["$ cargo test"]) * CW + 2 * PAD)
ours_cmd = ["RESIDUAL_RISK=1 cargo llvm-cov --no-report \\", "  --features coverage -- --nocapture"]
# bold lines are a little wider than CW per character
right_w = int(max(len(l) for l in ours + [c + "  " for c in ours_cmd]) * CW * 1.04 + 2 * PAD)
height = int(LH * (len(ours) + len(ours_cmd)) + 44)
W = left_w + GAP + right_w
H = 34 + height + 8
svg = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}">',
       "<style>.m{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;"
       f"font-size:{FS}px}} .t{{font-family:-apple-system,Segoe UI,Helvetica,Arial,sans-serif;"
       f"font-size:15px;font-weight:600;fill:#57606a}}</style>"]
svg += pane(0, "proptest", ["cargo test"], plain, left_w, height)
svg += pane(left_w + GAP, "with proptest-residual-risk", ours_cmd, ours, right_w, height)
svg.append("</svg>")
OUT.write_text("\n".join(svg) + "\n")
print(OUT, W, H)
print("\n".join(plain))
print("\n".join(ours))
