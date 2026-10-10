// Handle files before Mosaic's text-only clipboard listener. Clipboard data
// belongs to this paste event; no asynchronous clipboard permission is needed.
export function installFilePaste(accepts, receive) {
    document.addEventListener('paste', event => {
        const data = event.clipboardData;
        if (!data || !accepts()) return;
        const files = Array.from(data.files ?? []);
        if (!files.length) {
            for (const item of Array.from(data.items ?? [])) {
                if (item.kind !== 'file') continue;
                const file = item.getAsFile();
                if (file) files.push(file);
            }
        }
        if (!files.length) return; // Text remains with the existing editor.
        event.preventDefault();
        event.stopImmediatePropagation();
        receive(files);
    }, true);
}
