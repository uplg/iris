// Document Picture-in-Picture: an always-on-top window the player stage moves into (the engine's
// media element is carried across documents, playback survives the move). This owns the window:
// opening it at the last size, copying the app's styles in, noticing when it closes.

declare global {
	interface DocumentPictureInPictureOptions {
		width?: number;
		height?: number;
		disallowReturnToOpener?: boolean;
		preferInitialWindowPlacement?: boolean;
	}
	interface DocumentPictureInPicture {
		window: Window | null;
		requestWindow(options?: DocumentPictureInPictureOptions): Promise<Window>;
	}
	interface Window {
		documentPictureInPicture?: DocumentPictureInPicture;
	}
}

export function isDocumentPipSupported(): boolean {
	return typeof window !== 'undefined' && 'documentPictureInPicture' in window;
}

/** The app's stylesheets into the PiP document: same-origin sheets as text (dev and bundled
 * alike), anything unreadable by its URL. */
function adoptStyles(win: Window) {
	const inline: string[] = [];
	for (const sheet of Array.from(document.styleSheets)) {
		try {
			const text = Array.from(sheet.cssRules)
				.map((r) => r.cssText)
				.join('\n');
			if (text) inline.push(text);
		} catch {
			if (sheet.href) {
				const link = win.document.createElement('link');
				link.rel = 'stylesheet';
				link.href = sheet.href;
				win.document.head.appendChild(link);
			}
		}
	}
	if (inline.length) {
		const style = win.document.createElement('style');
		style.textContent = inline.join('\n');
		win.document.head.appendChild(style);
	}
	const theme = document.documentElement.dataset.theme;
	if (theme) win.document.documentElement.dataset.theme = theme;
	const body = win.document.body;
	body.style.margin = '0';
	body.style.display = 'flex';
	body.style.height = '100vh';
	win.document.documentElement.style.height = '100%';
	body.classList.add('pip-body');
}

export class DocumentPip {
	window = $state<Window | null>(null);
	#size: { w: number; h: number };

	constructor(width = 720, height = 405) {
		this.#size = { w: width, h: height };
	}

	get active() {
		return this.window !== null;
	}

	async toggle(): Promise<void> {
		const open = this.window;
		if (open) {
			this.#size = { w: open.innerWidth || this.#size.w, h: open.innerHeight || this.#size.h };
			try {
				open.close();
			} catch {
				// already closed
			}
			this.window = null;
			return;
		}
		const api = isDocumentPipSupported() ? window.documentPictureInPicture : undefined;
		if (!api) return;
		try {
			const win = await api.requestWindow({
				width: this.#size.w,
				height: this.#size.h,
				disallowReturnToOpener: false,
				preferInitialWindowPlacement: true
			});
			adoptStyles(win);
			win.addEventListener('pagehide', () => {
				if (this.window === win) this.window = null;
			});
			this.window = win;
		} catch (e) {
			console.warn('[iris-core] Document PiP requestWindow failed:', e);
		}
	}
}
