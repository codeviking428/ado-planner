# Dev session

Two loops. Pick by what you need to observe.

## CDP — the user's window

Before launching Electron or taking a "what do they see" screenshot, probe `http://127.0.0.1:9222/json/version`.

If it answers, attach with Playwright `chromium.connectOverCDP('http://127.0.0.1:9222')` and use the first page. That page is the user's `pnpm dev` renderer (HMR). Screenshot, click, and read the DOM on that connection.

Done when you have a screenshot or locator result from that page, or the probe returned nothing.

## Mocks — scripted iteration

`pnpm e2e` launches its own Electron against the ADO mock. Use that for red-green specs, regressions, and anything that must be deterministic.

Done when the spec passes or fails with a Playwright error.

## Probe missed

Ask the user to start `pnpm dev` (the script opens CDP on 9222), then probe again. Fall back to mocks for scripted work while you wait.
