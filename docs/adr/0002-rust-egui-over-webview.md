# Rust + egui for the UI, not a webview framework

The installer must be a single self-contained executable on Windows and Linux with a fully custom Civ5 art-deco skin, and its backend does git operations, large downloads, MSI/CAB/ISO extraction, and compiler orchestration. We chose Rust + egui: one static binary with no system webview dependency, GPU-rendered custom skinning, and first-class Rust libraries for every backend need.

## Considered Options

- **Tauri / webview** - best design medium (HTML/CSS) but breaks "single exe that just works": needs webkit2gtk on Linux and the WebView2 runtime on Windows.
- **Qt / Flutter** - heavier runtimes, worse single-binary story.

## Consequences

The art-deco skin is hand-crafted (textures, nine-slice panels, embedded fonts) rather than styled with CSS - a deliberate cost accepted for dependency-free distribution.
