// Run with the browser dev shell and a Playwright installation supplied via
// RELAY_PLAYWRIGHT_MODULE. The browser binary is supplied via CHROMIUM_BIN.
import fs from 'node:fs';
import assert from 'node:assert/strict';
const { chromium } = await import(process.env.RELAY_PLAYWRIGHT_MODULE || 'playwright-core');
const source = fs.readFileSync(new URL('../src/browser_clipboard.js', import.meta.url), 'utf8');
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_BIN, headless: true, args: ['--no-sandbox'] });
try {
    const context = await browser.newContext({ permissions: ['clipboard-read', 'clipboard-write'] });
    const page = await context.newPage();
    await page.route('https://relay.test/**', route => route.fulfill({
        contentType: route.request().url().endsWith('.js') ? 'application/javascript' : 'text/html',
        body: route.request().url().endsWith('.js') ? source : '<canvas tabindex="0"></canvas><textarea></textarea>',
    }));
    await page.goto('https://relay.test/');
    await page.evaluate(async () => {
        window.received = []; window.otherPastes = 0; window.accepts = true;
        const { installFilePaste } = await import('/clipboard.js');
        installFilePaste(() => window.accepts, files => window.received.push(files.map(f => ({ name: f.name, type: f.type, size: f.size }))));
        document.addEventListener('paste', () => window.otherPastes++);
        const canvas = document.querySelector('canvas'); canvas.focus();
        const blob = await new Promise(resolve => { const c = document.createElement('canvas'); c.width = c.height = 2; c.toBlob(resolve, 'image/png'); });
        await navigator.clipboard.write([new ClipboardItem({ 'image/png': blob })]);
    });
    await page.keyboard.press('Control+v');
    await page.waitForFunction(() => window.received.length === 1);
    assert.equal(await page.evaluate(() => window.received[0][0].type), 'image/png');
    assert.equal(await page.evaluate(() => window.otherPastes), 0);
    // Mobile DOM editor and clipboard-item-only browsers take the same path.
    await page.evaluate(() => {
        const file = new File(['content'], 'notes.txt', { type: 'text/plain' });
        const event = new Event('paste', { bubbles: true, cancelable: true });
        Object.defineProperty(event, 'clipboardData', { value: { files: [], items: [{ kind: 'file', getAsFile: () => file }] } });
        document.querySelector('textarea').dispatchEvent(event);
    });
    assert.equal(await page.evaluate(() => window.received[1][0].name), 'notes.txt');
    await page.evaluate(async () => { await navigator.clipboard.writeText('plain text'); document.querySelector('textarea').focus(); });
    await page.keyboard.press('Control+v');
    assert.equal(await page.locator('textarea').inputValue(), 'plain text');
    assert.equal(await page.evaluate(() => window.received.length), 2);
    await page.evaluate(() => { window.accepts = false; const data = new DataTransfer(); data.items.add(new File(['x'], 'ignored.txt')); document.querySelector('textarea').dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true })); });
    assert.equal(await page.evaluate(() => window.received.length), 2);
    console.log('Browser image paste, file-item fallback, text paste, and focus gating passed');
} finally { await browser.close(); }
