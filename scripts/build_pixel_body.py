"""Packs a companion's rigged pixel-art animations into the body the app draws.

    python scripts/build_pixel_body.py lappland "C:/.../Pixel Art/output"
    python scripts/build_pixel_body.py kaltsit "C:/.../Pixel Art/Kaltsit/output/rig"

Reads the composed frames (frame_NNN.png) and rig.json of each rigged animation (or, for one
without a rig, its Aseprite sheet: <name>_sheet.png and _sheet.json, with no mouth points: she
does not talk in it), and writes
ui/personajes/<id>/: one horizontal strip per animation (<name>.png) and cuerpo.json with,
per animation, the frame size, the durations, whether it loops, where her feet are (the canvases
differ: the app lines them up by the feet) and where her mouth is in each frame (for lip-sync).
"""
import json
import sys
from pathlib import Path

from PIL import Image

# per companion: app animation -> (folder of a rig, or "sheet:<path to the sheet's json>", loops),
# relative to the folder given.
COMPANIONS = {
    "lappland": {
        "Relax": ("rig_v2/idle", True),
        "Move": ("new_actions_v3/caminar", True),
        "Interact": ("new_actions_v3/espadas", False),
        "Sit": ("rig_v2/trono", False),
        "Sleep": ("sheet:lappland_dormir_sheet.json", True),
    },
    "kaltsit": {
        "Relax": ("rig/idle", True),
        "Move": ("rig/caminar", True),
        "Interact": ("rig/jeringas", False),
        "Sit": ("rig/silla", False),
        "Sleep": ("sheet:kaltsit_dormir_sheet.json", True),
    },
}


def from_sheet(path):
    """Frames and durations of an Aseprite sheet (json next to its png)."""
    meta = json.loads(path.read_text(encoding="utf-8"))
    sheet = Image.open(path.with_suffix(".png")).convert("RGBA")
    items = list(meta["frames"].values()) if isinstance(meta["frames"], dict) else meta["frames"]
    frames = [sheet.crop((f["frame"]["x"], f["frame"]["y"], f["frame"]["x"] + f["frame"]["w"], f["frame"]["y"] + f["frame"]["h"])) for f in items]
    return frames, [f["duration"] for f in items]


def feet(img):
    """Bottom-centre of the figure: the lowest opaque row, and the middle of the opaque
    pixels in the lowest 14 rows."""
    a = img.getchannel("A")
    w, h = img.size
    px = a.load()
    rows = [y for y in range(h) if any(px[x, y] > 40 for x in range(w))]
    bottom = max(rows)
    xs = [x for y in range(max(0, bottom - 14), bottom + 1) for x in range(w) if px[x, y] > 40]
    xs.sort()
    return [xs[len(xs) // 2], bottom + 1]


def main():
    who, src = sys.argv[1], Path(sys.argv[2])
    out = Path(__file__).resolve().parent.parent / "ui" / "personajes" / who
    out.mkdir(parents=True, exist_ok=True)
    body = {"escala": 1, "animaciones": {}}
    for name, (folder, loop) in COMPANIONS[who].items():
        if folder.startswith("sheet:"):
            frames, durations = from_sheet(src / folder[len("sheet:"):])
            mouths = []
        else:
            d = src / folder
            rig = json.loads((d / "rig.json").read_text(encoding="utf-8"))
            frames = [Image.open(d / f"frame_{n:03d}.png").convert("RGBA") for n in range(rig["count"])]
            durations, mouths = rig["durations"], [p["anchors"]["mouth"] for p in rig["poses"]]
        w, h = frames[0].size
        strip = Image.new("RGBA", (w * len(frames), h))
        for n, f in enumerate(frames):
            strip.paste(f, (n * w, 0))
        strip.save(out / f"{name}.png", optimize=True)
        body["animaciones"][name] = {
            "w": w,
            "h": h,
            "duraciones": durations,
            "bucle": loop,
            "pies": feet(frames[0]),
            "bocas": mouths,
        }
        print(f"{name}: {len(frames)} fotogramas {w}x{h}, pies {body['animaciones'][name]['pies']}")
    # Asleep in the same place she sits (same canvas, same chair): lined up by her sitting feet,
    # or the chair's legs would be taken for her feet and she would jump aside.
    anims = body["animaciones"]
    if "Sleep" in anims and "Sit" in anims and (anims["Sleep"]["w"], anims["Sleep"]["h"]) == (anims["Sit"]["w"], anims["Sit"]["h"]):
        anims["Sleep"]["pies"] = anims["Sit"]["pies"]
    (out / "cuerpo.json").write_text(json.dumps(body, indent=1), encoding="utf-8")


if __name__ == "__main__":
    main()
