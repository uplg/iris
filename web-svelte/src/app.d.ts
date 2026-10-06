// See https://svelte.dev/docs/kit/types#app.d.ts
declare global {
	namespace App {
		// interface Error {}
	}
}

// a module, so that `declare global` above augments App
// oxlint-disable-next-line unicorn/require-module-specifiers
export {};
