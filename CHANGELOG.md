# Changelog

## 1.0.0 — 2026-05-17
- Metadata bar: vertical layout with field labels, optional branding line.
- Font sizing: Auto (sublinear, readable on grid tiles and large frames), Pixels, Percent — for both timestamp and bar.
- Dark/light theme toggle, Repository button moved into status row.
- File list: counter in title, per-file Remove, wider panel.
- Progress bar fixed (no laggy animation, clamped 0–1); final status shows X/Y succeeded.
- GUI split into globals + per-section panels; magic ints replaced with Slint enums.
- `overwrite` default flipped to on.

## 0.1.0 — 2026-03-23
- Initial release: single / N×M grid / sequence modes, JPEG/PNG/WebP output, optional timestamp overlay and horizontal metadata bar. CLI + GUI.
