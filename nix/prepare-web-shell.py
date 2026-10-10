#!/usr/bin/env python3
"""Externalize Mosaic's generated bootstrap for Relay's strict browser CSP."""

from pathlib import Path
import gzip
import re
import sys


def prepare(root: Path) -> None:
    index = root / "index.html"
    page = index.read_text()
    styles = re.findall(r"<style>(.*?)</style>", page, flags=re.S)
    scripts = re.findall(r'<script type="module">(.*?)</script>', page, flags=re.S)
    if len(styles) != 1 or len(scripts) != 1:
        raise SystemExit("Expected one Mosaic style and one module bootstrap")
    (root / "app.css").write_text(styles[0] + "\n/* Relay's UI root fits the visible area; canvas pixels match the layout viewport. */\ncanvas { width: var(--relay-layout-width, 100vw); height: var(--relay-layout-height, 100vh); transform: translate(var(--relay-viewport-left, 0px), var(--relay-viewport-top, 0px)); touch-action: pinch-zoom; }\n#relay-text-input { position: fixed; width: 1px; height: 20px; margin: 0; padding: 0; border: 0; opacity: 0.01; pointer-events: none; font: 16px sans-serif; resize: none; }\n#loading { position: fixed; inset: 0; display: grid; place-items: center; margin: 0; color: #cdd6f4; font: 16px system-ui, sans-serif; }\n")
    script = scripts[0].replace("await init();", "await init();\nloading.remove();").replace("} else {", "} else {\nloading.remove();")
    (root / "bootstrap.js").write_text('const loading = document.getElementById("loading");\ntry {\n' + script + '\n} catch { loading.textContent = "Could not load Relay. Reload to try again."; loading.setAttribute("role", "alert"); }\n')
    page = re.sub(r"<style>.*?</style>", '<link rel="stylesheet" href="./app.css">', page, flags=re.S)
    page = re.sub(
        r'<script type="module">.*?</script>',
        '<script type="module" src="./bootstrap.js"></script>',
        page,
        flags=re.S,
    )
    page = page.replace("<body>", '<body>\n<p id="loading" role="status">Loading Relay…</p>')
    # Keep the unsupported state useful without browser-specific setup advice.
    page = re.sub(r'<div id="unsupported">.*?</div>', '<div id="unsupported"><h1>Relay needs WebGPU</h1><p id="missing">This browser does not support WebGPU.</p><p id="adapter">This browser could not access a graphics adapter.</p><p id="advice">Try a browser with WebGPU enabled and an available graphics adapter.</p></div>', page, flags=re.S)
    index.write_text(page)
    # Compress only static app files; account/workspace responses stay separate.
    for path in root.rglob("*"):
        if path.is_file() and path.suffix in {".wasm", ".js", ".css", ".html"}:
            path.with_name(path.name + ".gz").write_bytes(gzip.compress(path.read_bytes(), compresslevel=9, mtime=0))


if __name__ == "__main__":
    prepare(Path(sys.argv[1]))
