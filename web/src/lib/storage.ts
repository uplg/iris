/** Where the browser keeps what the app remembers (localStorage), each key once. */
export const STORAGE = {
	theme: 'iris-theme',
	passkeyOffered: 'iris-passkey-offered',
	libraryView: 'iris-library-view',
	librarySort: 'iris-library-sort',
	searchView: 'iris-search-view',
	/** Shared with the React app, so a device keeps its volume across the switch. */
	volume: 'iris:volume',
	theater: 'iris:theater'
} as const;
