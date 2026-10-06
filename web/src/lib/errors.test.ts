import { describe, expect, it } from 'vitest';
import { ApiError } from '@iris/api/client';
import { errorText } from './errors.ts';

describe('errorText', () => {
	it('says a passkey request answered with no credential in words, not « cancelled »', () => {
		expect(errorText(new ApiError(0, 'cancelled', 'cancelled'))).toBe('The passkey request was cancelled. Try again when you are ready.');
	});

	it("keeps the server's own words otherwise", () => {
		expect(errorText(new ApiError(400, 'bad_request', 'This invitation has expired.'))).toBe('This invitation has expired.');
	});
});
