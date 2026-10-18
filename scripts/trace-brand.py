"""Reconstruct vector contours from the supplied ORVEXA.png master reference.
No raster is embedded. The original image is never modified. Requires Pillow.
"""
from pathlib import Path
from collections import defaultdict
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
image = Image.open(ROOT / "ORVEXA.png").convert("RGB")
out = ROOT / "web/public/brand"
out.mkdir(parents=True, exist_ok=True)


def simplify(points, tolerance=1.25):
    if len(points) < 3:
        return points
    a, b = points[0], points[-1]
    dx, dy = b[0] - a[0], b[1] - a[1]
    norm = (dx * dx + dy * dy) ** 0.5
    distances = [abs(dx * (a[1] - p[1]) - (a[0] - p[0]) * dy) / norm
                 if norm else ((p[0] - a[0]) ** 2 + (p[1] - a[1]) ** 2) ** 0.5
                 for p in points]
    index = max(range(len(distances)), key=distances.__getitem__)
    if distances[index] <= tolerance:
        return [a, b]
    return simplify(points[:index + 1], tolerance)[:-1] + simplify(points[index:], tolerance)


def trace(box):
    crop = image.crop(box)
    w, h = crop.size
    pixels = crop.load()
    white = {(x, y) for y in range(h) for x in range(w) if min(pixels[x, y]) >= 180}
    edges = defaultdict(list)
    for x, y in white:
        if (x, y - 1) not in white:
            edges[(x, y)].append((x + 1, y))
        if (x + 1, y) not in white:
            edges[(x + 1, y)].append((x + 1, y + 1))
        if (x, y + 1) not in white:
            edges[(x + 1, y + 1)].append((x, y + 1))
        if (x - 1, y) not in white:
            edges[(x, y + 1)].append((x, y))
    contours = []
    while edges:
        start = next(iter(edges))
        p = start
        points = [p]
        while p in edges:
            n = edges[p].pop()
            if not edges[p]:
                del edges[p]
            points.append(n)
            p = n
            if p == start:
                break
        area = abs(sum(a[0]*b[1] - b[0]*a[1] for a, b in zip(points, points[1:]))) / 2
        if area >= 20:
            points = simplify(points)
            contours.append("M" + " L".join(f"{x},{y}" for x, y in points[:-1]) + "Z")
    return w, h, " ".join(contours)


symbol = trace((395, 245, 845, 750))
wordmark = trace((195, 815, 1068, 938))
for name, (w, h, path) in [("symbol", symbol), ("wordmark", wordmark)]:
    svg = f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" role="img" aria-label="ORVEXA {name}">
  <title>ORVEXA {name}</title>
  <desc>Vector reconstruction of the approved ORVEXA.png reference; not an embedded bitmap.</desc>
  <path fill="#E6F2FF" fill-rule="evenodd" d="{path}"/>
</svg>
'''
    (out / f"orvexa-{name}.svg").write_text(svg, encoding="utf-8")
sw, sh, sp = symbol
ww, wh, wp = wordmark
lockup = f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1060 150" role="img" aria-label="ORVEXA">
  <title>ORVEXA</title>
  <g fill="#E6F2FF" fill-rule="evenodd">
    <path transform="translate(0 4) scale({142 / sh})" d="{sp}"/>
    <path transform="translate(175 14)" d="{wp}"/>
  </g>
</svg>
'''
(out / "orvexa-logo.svg").write_text(lockup, encoding="utf-8")
print("Wrote symbol, wordmark and horizontal lockup as real SVG paths.")
