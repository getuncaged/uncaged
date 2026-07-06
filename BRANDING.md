# Rebranding Uncaged

Uncaged is built to be forked. Everything that makes it "Uncaged" — the name, the
palette, the logo, and the AI tools it offers — lives in **three** places. Change
these and the whole app follows. You should never need to hunt through the
codebase for a hard-coded name or colour.

---

## 1. Name, palette & logo paths — [`app/src/brand.rs`](app/src/brand.rs)

The single source of truth for text and colour:

| What | Constant |
|------|----------|
| Product name | `brand::NAME` (`"Uncaged"`) |
| Machine name (config dirs, ids) | `brand::NAME_LOWER` (`"uncaged"`) |
| Tagline | `brand::TAGLINE` |
| Project home URL | `brand::HOME_URL` |
| Accent (ember) palette | `brand::ember::{GOLD, ORANGE, RED, ACCENT}` |
| Ground (night-workbench) palette | `brand::ground::{BASE, PANEL, RAISED, LINE, INK, MUTED, FAINT}` |
| Logo asset paths | `brand::MARK_SVG`, `brand::ICON_SVG` |

Change the strings and the `u32` colour literals here, rebuild, and the name and
accent update across the UI.

---

## 2. The logo — [`app/assets/bundled/svg/brand/`](app/assets/bundled/svg/brand/)

Two SVGs, one mark:

- **`uncaged-mark.svg`** — the in-app monochrome glyph, `[ ❯_ ]`. It is drawn with a
  single `rgb(255,0,0)` **recolour sentinel**: the app's icon renderer tints it to
  whatever colour the UI asks for (theme accent, foreground, etc.). Keep it
  single-colour and keep the sentinel. This is what shows on tabs, the agent
  avatar, the footer, and anywhere `Icon::Oz` renders.
- **`uncaged-icon.svg`** — the full-colour **app-icon master** (1024×1024). This is
  what becomes the dock / Finder icon.

Both are wired through `Icon::Oz` / `Icon::OzCloud` in
[`crates/warp_core/src/ui/icons.rs`](crates/warp_core/src/ui/icons.rs) — so
repointing those two lines swaps the in-app glyph everywhere at once.

### Regenerating the platform app icons

After editing `uncaged-icon.svg`, regenerate the macOS `.icns` and channel PNGs:

```sh
# 1024 master -> iconset -> .icns  (needs librsvg + iconutil, macOS)
mkdir -p /tmp/Uncaged.iconset
for s in 16 32 128 256 512; do
  rsvg-convert -w $s      -h $s      app/assets/bundled/svg/brand/uncaged-icon.svg > /tmp/Uncaged.iconset/icon_${s}x${s}.png
  rsvg-convert -w $((s*2)) -h $((s*2)) app/assets/bundled/svg/brand/uncaged-icon.svg > /tmp/Uncaged.iconset/icon_${s}x${s}@2x.png
done
iconutil -c icns -o app/channels/oss/icon/AppIcon.icns /tmp/Uncaged.iconset

# Channel PNGs used by the build
rsvg-convert -w 512 -h 512 app/assets/bundled/svg/brand/uncaged-icon.svg > app/channels/oss/icon/512x512.png
rsvg-convert -w 512 -h 512 app/assets/bundled/svg/brand/uncaged-icon.svg > app/channels/oss/icon/no-padding/512x512.png
```

> **Asset embedding:** bundled SVGs are embedded at **compile time** (`rust-embed`
> `debug-embed`). After changing any file under `app/assets/bundled/`, run
> `touch crates/warp_assets/src/lib.rs` before building so the change is picked up.

---

## 3. The AI tools — [`crates/uncaged_engine/src/catalog.rs`](crates/uncaged_engine/src/catalog.rs)

The "Connect a model" gallery, the providers, their brand icons, default models,
and setup copy are all defined by the `Preset` catalog here. Add, remove, or
re-order providers in one place. Provider brand SVGs live alongside the logo in
[`app/assets/bundled/svg/`](app/assets/bundled/svg/) and are mapped to `Icon`
variants in [`crates/warp_core/src/ui/icons.rs`](crates/warp_core/src/ui/icons.rs).

---

## Building & deploying (macOS)

```sh
source "$HOME/.cargo/env"
touch crates/warp_assets/src/lib.rs           # if any bundled asset changed
CARGO_INCREMENTAL=0 cargo build --bin warp-oss

# Deploy into the signed app bundle, then RE-SIGN (required — AMFI kills an
# unsigned/modified bundle on launch):
cp target/debug/warp-oss ~/Applications/Uncaged.app/Contents/MacOS/warp-oss
codesign --force --deep --sign - ~/Applications/Uncaged.app
```

That's the whole surface. Name + colour in `brand.rs`, logo in
`svg/brand/`, AI tools in `catalog.rs`. Fork away.
