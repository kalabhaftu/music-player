# Legacy Music Player Interface (Archived)

> **Archived:** This document describes the retired WinUI 3 + WebView2 UI in `legacy/src/MusicPlayer.App/`. For the primary Rust + Slint design system, see [`../DESIGN.md`](../DESIGN.md).

The WinUI 3 window hosts the locally packaged HTML, CSS, and JavaScript interface (`WebUI/`) through WebView2.

## Visual system

Use the near-black background hierarchy, flat compact navigation, flexible library area, and contextual Now Playing pane. Keep the restrained lime accent, compact type, dense song rows, small-radius artwork, hover-revealed play actions, and persistent bottom player. Avoid marketing copy, decorative statistics, blanket borders, and generic dashboard cards.

Prefer embedded or local artwork. For missing art, use a stable gradient derived from album or artist identity with initials; never treat fallback art as library metadata.

## Layout and behavior

At desktop widths keep the navigation rail, main library, right pane, and bottom player visible. At narrower widths let the right pane yield first, compact the navigation, reduce card columns, and hide secondary table columns while keeping playback and queue access reachable.

C# owns playback, library and playlist state, scanning, settings, filesystem access, and Windows services. The WebView bridge accepts explicit allowlisted commands and returns bounded, paged data. Artwork is lazy-loaded and cached.

The local frontend is vanilla JavaScript: `scripts/api.js` carries correlated bridge commands and events, `scripts/library.js` renders library/detail views, `scripts/player.js` renders authoritative playback state, and `scripts/app.js` wires navigation and interactions. `styles/tokens.css`, `layout.css`, and `components.css` define the shared visual system.
