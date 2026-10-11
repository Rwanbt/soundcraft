// Browser-level smoke test for the actual bundled Rust/WASM SoundCraft web app.
// No mocks: the Trunk-built JS and WASM must load and mount a real egui canvas.
import { chromium } from 'playwright';

const base = process.env.SOUNDCRAFT_WEB_URL ?? 'http://127.0.0.1:8765/web/';
const browser = await chromium.launch({
  headless: true,
  args: [
    '--use-gl=angle',
    '--use-angle=swiftshader',
    '--enable-webgl',
    '--enable-unsafe-swiftshader',
  ],
});

let failures = 0;
try {
  for (const testCase of [
    { name: 'desktop-empty', query: '?webgl&empty', width: 1280, height: 800 },
    { name: 'mobile-demo', query: '?webgl', width: 393, height: 852 },
  ]) {
    const context = await browser.newContext({
      viewport: { width: testCase.width, height: testCase.height },
      deviceScaleFactor: 1,
    });
    const page = await context.newPage();
    const exceptions = [];
    page.on('pageerror', (error) => exceptions.push(error.message));
    try {
      const response = await page.goto(base + testCase.query, { waitUntil: 'domcontentloaded', timeout: 30_000 });
      if (!response?.ok()) throw new Error('HTTP response was not successful');
      await page.locator('#soundcraft_canvas').waitFor({ state: 'visible', timeout: 30_000 });
      await page.waitForFunction(() => document.getElementById('soundcraft_loading') === null, null, { timeout: 90_000 });
      const canvas = await page.locator('#soundcraft_canvas').evaluate((node) => ({
        width: node.width,
        height: node.height,
        visibleWidth: node.getBoundingClientRect().width,
        visibleHeight: node.getBoundingClientRect().height,
      }));
      if (canvas.width < 1 || canvas.height < 1 || canvas.visibleWidth < 1 || canvas.visibleHeight < 1) {
        throw new Error('WASM initialized but canvas has no renderable area: ' + JSON.stringify(canvas));
      }
      await page.mouse.click(Math.floor(testCase.width / 2), Math.floor(testCase.height / 2));
      await page.waitForTimeout(500);
      if (exceptions.length) throw new Error('Unhandled browser errors: ' + exceptions.join(' | '));
      console.log('PASS ' + testCase.name + ' ' + JSON.stringify(canvas));
    } catch (error) {
      failures++;
      const loading = await page.locator('#soundcraft_loading').textContent().catch(() => '');
      console.error('FAIL ' + testCase.name + ': ' + error + ' loading=' + loading + ' exceptions=' + exceptions.join(' | '));
      await page.screenshot({ path: 'web-smoke-' + testCase.name + '.png', fullPage: true }).catch(() => {});
    } finally {
      await context.close();
    }
  }
} finally {
  await browser.close();
}
if (failures) process.exitCode = 1;
