// The old suggestions page: its shelves live on Discover now.
import { redirect } from '@sveltejs/kit';

export function load(): never {
	redirect(308, '/discover');
}
