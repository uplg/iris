import { describe, expect, it } from 'vitest';
import { ApiError } from './client';
import { isNoSeeders, isNotOnDisk, registerRefusal } from './refusals';

describe('refusals', () => {
	it('« not on disk yet », by its code or the words the server sends today', () => {
		expect(isNotOnDisk(new ApiError(400, 'not_on_disk', 'whatever'))).toBe(true);
		expect(isNotOnDisk(new ApiError(400, 'bad_request', 'file not yet on disk: download in progress (x)'))).toBe(true);
		expect(isNotOnDisk({ code: 'bad_request', message: 'not yet probable' })).toBe(true);
		expect(isNotOnDisk(new Error('no seeders'))).toBe(false);
		expect(isNotOnDisk(undefined)).toBe(false);
	});

	it('a dead swarm', () => {
		expect(isNoSeeders(new ApiError(409, 'conflict', 'stalled: no seeders for this file'))).toBe(true);
		expect(isNoSeeders(new ApiError(409, 'dead_torrent', 'this release has no seeders'))).toBe(true);
		expect(isNoSeeders(new Error('file not yet on disk'))).toBe(false);
	});

	it('which part of a registration was refused', () => {
		expect(registerRefusal(new ApiError(400, 'password_too_short', 'Use at least 8 characters.'))).toBe('password');
		expect(registerRefusal(new ApiError(400, 'bad_request', 'invalid or expired invitation'))).toBe('invitation');
		expect(registerRefusal(new ApiError(409, 'conflict', 'invitation already used'))).toBe('invitation');
		expect(registerRefusal(new ApiError(409, 'conflict', 'email already registered'))).toBe('email_taken');
		expect(registerRefusal(new ApiError(400, 'bad_request', 'invalid email'))).toBe('email_invalid');
		expect(registerRefusal(new ApiError(500, 'internal', 'internal server error'))).toBeNull();
	});
});
