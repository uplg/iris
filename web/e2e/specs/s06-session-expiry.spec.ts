// Scenario 6: the access cookie expires mid-playback (a laptop waking past its lifetime). The
// next range request answers 401, the engine's fetch refreshes the session and replays it
// (M13): playback goes on, on the same tier, never demoted to F, and every engine request to
// /stream carries X-Iris-Client.
import { expect, openWatch, play, stage, test, videoState } from '../lib/bench.ts';

for (const tier of ['B', 'C'] as const) {
	test(
		`tier ${tier}: an expired access cookie mid-playback is refreshed, not demoted`,
		{ tag: ['@chrome'] },
		async ({ page, state, logs, streamRequests }) => {
			const refreshes: number[] = [];
			page.on('response', (r) => {
				if (r.url().endsWith('/api/auth/refresh')) refreshes.push(r.status());
			});
			const unauthorized: string[] = [];
			page.on('response', (r) => {
				if (/\/stream/.test(r.url()) && r.status() === 401) unauthorized.push(r.url());
			});
			await openWatch(page, state, 'heavy', { tier });
			await play(page);
			const clock = async () =>
				tier === 'B'
					? ((await videoState(page))?.currentTime ?? 0)
					: Number(await page.getByRole('slider', { name: 'Seek' }).getAttribute('aria-valuenow'));
			await expect.poll(clock, { timeout: 30_000 }).toBeGreaterThan(3);

			// what the browser does with an expired Max-Age cookie: it stops sending it
			await page.context().clearCookies({ name: 'iris_access' });
			const before = streamRequests.length;
			// far outside what is buffered or cached: the engine must fetch again
			await stage(page).press('8');
			await expect.poll(clock, { timeout: 60_000 }).toBeGreaterThan(98);
			const after = await clock();
			await page.waitForTimeout(3000);
			expect(await clock()).toBeGreaterThan(after + 1.5);

			console.log(
				`[measure] tier ${tier}: 401s ${unauthorized.length}, refreshes ${refreshes.join(',')}, stream requests after expiry ${streamRequests.length - before}`
			);
			expect(unauthorized.length, 'the expired cookie never got a 401: the test did not exercise the refresh').toBeGreaterThan(0);
			expect(refreshes).toContain(200);
			expect(logs.matching(/tier [BC] → F/), 'demoted').toEqual([]);
			const engineReads = streamRequests.filter((r) => r.range);
			expect(engineReads.length).toBeGreaterThan(0);
			expect(
				engineReads.filter((r) => !r.client?.startsWith('web/')),
				'a /stream request without X-Iris-Client'
			).toEqual([]);
		}
	);
}
