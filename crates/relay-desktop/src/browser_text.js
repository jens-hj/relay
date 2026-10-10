// The canvas remains the editor. This control supplies the browser input method.
let bridge;
export function installTextInput(snapshot, edit, key, resize) {
    const input = document.createElement('textarea');
    input.id = 'relay-text-input';
    input.tabIndex = -1;
    input.setAttribute('autocomplete', 'off');
    input.setAttribute('autocapitalize', 'sentences');
    input.spellcheck = true;
    document.body.appendChild(input);
    let current = null, composing = false, syncing = false, pinching = false;
    const read = () => { const json = snapshot(); return json ? JSON.parse(json) : null; };
    const sync = () => {
        if (composing) return;
        const next = read();
        if (!next) {
            input.blur(); input.value = ''; current = next;
            return;
        }
        current = next;
        if (!current) return;
        syncing = true;
        input.setAttribute('aria-label', current.label);
        input.setAttribute('enterkeyhint', current.multiline ? 'enter' : 'done');
        if (input.value !== current.text) input.value = current.text;
        if (input.selectionStart !== current.start || input.selectionEnd !== current.end)
            input.setSelectionRange(current.start, current.end);
        const canvas = document.querySelector('canvas');
        if (canvas) {
            const rect = canvas.getBoundingClientRect();
            // Keep the editable control in the visible viewport so Safari
            // doesn't scroll the whole canvas to an offscreen hidden input.
            const visible = window.visualViewport;
            const left = visible?.offsetLeft ?? 0, top = visible?.offsetTop ?? 0;
            const width = visible?.width ?? innerWidth, height = visible?.height ?? innerHeight;
            input.style.left = `${Math.max(left, Math.min(left + width - 16, rect.left + current.caret[0]))}px`;
            input.style.top = `${Math.max(top, Math.min(top + height - 20, rect.top + current.caret[1]))}px`;
        }
        syncing = false;
    };
    const update = () => {
        if (current) edit(current.id, input.value, input.selectionStart, input.selectionEnd, composing);
    };
    input.addEventListener('input', event => { event.stopPropagation(); update(); if (!composing) sync(); });
    input.addEventListener('compositionstart', event => { event.stopPropagation(); composing = true; });
    input.addEventListener('compositionupdate', event => event.stopPropagation());
    input.addEventListener('compositionend', event => { event.stopPropagation(); composing = false; update(); sync(); });
    document.addEventListener('selectionchange', () => {
        if (!syncing && !composing && document.activeElement === input && current &&
            (input.selectionStart !== current.start || input.selectionEnd !== current.end)) { update(); sync(); }
    });
    for (const name of ['copy', 'cut', 'paste', 'beforeinput', 'keyup'])
        input.addEventListener(name, event => event.stopPropagation());
    input.addEventListener('keydown', event => {
        event.stopPropagation();
        if (event.isComposing) return;
        const shortcut = (event.ctrlKey || event.metaKey) && !['a','c','v','x','z','y'].includes(event.key.toLowerCase());
        if (['Tab','Escape','ArrowLeft','ArrowRight','ArrowUp','ArrowDown','Home','End'].includes(event.key) || shortcut || (event.key === 'Enter' && !current?.multiline)) {
            if (key(event.key, event.shiftKey, event.ctrlKey, event.altKey, event.metaKey)) event.preventDefault();
            sync();
            if (event.key === 'Tab' && current) input.focus({preventScroll:true});
            if (event.key === 'Escape') input.blur();
        }
    });
    const focus = (x, y) => {
        if (pinching || !window.matchMedia('(pointer: coarse)').matches) return;
        sync();
        if (!current) return;
        const [left, top, width, height] = current.rect;
        if (x < left || y < top || x > left + width || y > top + height) return;
        input.focus({preventScroll:true});
    };
    // These bubble after the canvas listeners. Focus must stay synchronous:
    // a timer or an awaited promise loses iOS keyboard activation.
    window.addEventListener('pointerdown', event => {
        if (event.pointerType === 'touch' && event.isPrimary && event.target instanceof HTMLCanvasElement) {
            // Suppress the compatibility mouse press that would steal focus
            // back from the DOM editor after touchend. Pointer cancellation
            // does not suppress browser pinch zoom; touch-action owns that.
            event.preventDefault();
            if (document.activeElement !== input) event.target.focus({preventScroll:true});
        }
    }, {capture:true});
    window.addEventListener('pointerup', event => {
        if (event.target instanceof HTMLCanvasElement) {
            const rect = event.target.getBoundingClientRect(); focus(event.clientX - rect.left, event.clientY - rect.top);
        }
    });
    window.addEventListener('touchstart', event => {
        if (event.touches.length > 1) pinching = true;
    }, {capture:true, passive:true});
    window.addEventListener('touchend', event => {
        if (pinching) { if (!event.touches.length) pinching = false; return; }
        const touch = event.changedTouches[0];
        if (touch && event.target instanceof HTMLCanvasElement) {
            const rect = event.target.getBoundingClientRect(); focus(touch.clientX - rect.left, touch.clientY - rect.top);
        }
    });
    window.addEventListener('touchcancel', event => { if (!event.touches.length) pinching = false; });
    let layoutSize, visibleSize;
    const viewport = () => {
        const visible = window.visualViewport;
        // Use visible CSS pixels, including while zoomed. Multiplying by the
        // zoom scale would leave the editor's caret below the keyboard again.
        const height = visible?.height ?? innerHeight;
        const width = visible?.width ?? innerWidth;
        const top = visible?.offsetTop ?? 0, left = visible?.offsetLeft ?? 0;
        const style = document.documentElement.style;
        // Mosaic's pinned runtime sizes its surface from the layout viewport.
        // Keep canvas CSS and backing pixels at that size; Relay's root alone
        // fits the visible area, avoiding compressed text or a stale UI height.
        if (style.getPropertyValue('--relay-layout-height') !== `${innerHeight}px`)
            style.setProperty('--relay-layout-height', `${innerHeight}px`);
        if (style.getPropertyValue('--relay-layout-width') !== `${innerWidth}px`)
            style.setProperty('--relay-layout-width', `${innerWidth}px`);
        if (style.getPropertyValue('--relay-viewport-height') !== `${height}px`)
            style.setProperty('--relay-viewport-height', `${height}px`);
        if (style.getPropertyValue('--relay-viewport-width') !== `${width}px`)
            style.setProperty('--relay-viewport-width', `${width}px`);
        if (style.getPropertyValue('--relay-viewport-top') !== `${top}px`)
            style.setProperty('--relay-viewport-top', `${top}px`);
        if (style.getPropertyValue('--relay-viewport-left') !== `${left}px`)
            style.setProperty('--relay-viewport-left', `${left}px`);
        const size = `${innerWidth}:${innerHeight}:${width}:${height}`;
        const area = `${width}:${height}`;
        if (area !== visibleSize) {
            visibleSize = area;
            resize(width, height);
        }
        if (size !== layoutSize) {
            layoutSize = size;
            // Safari's keyboard can resize only the visual viewport. Wake
            // Mosaic after updating CSS even when innerHeight never changed.
            window.dispatchEvent(new Event('resize'));
        }
        sync();
    };
    window.visualViewport?.addEventListener('resize', viewport);
    window.visualViewport?.addEventListener('scroll', viewport);
    window.addEventListener('resize', viewport);
    viewport();
    // Also observe the current size when a browser skips a viewport event
    // during an input-method transition. Unchanged sizes do not write CSS.
    setInterval(viewport, 100);
    bridge = {focus};
}
export function focusTextInput(x, y) { bridge?.focus(x, y); }
