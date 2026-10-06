// A tracker's HTML description (c411), made safe and ours: DOMPurify keeps the structure
// (headings, lists, tables, links, images) and drops whatever can run script. It also drops
// every attribute that dresses the text (style, color, bgcolor, class, id): the tracker's
// palette is unreadable on ours, and its classes or ids could collide with the page's.

import DOMPurify from 'dompurify';

// an instance of our own: its hook never reaches another sanitizer
const purify = DOMPurify(window);

purify.addHook('afterSanitizeAttributes', (node) => {
	if (node.tagName === 'A' && node.hasAttribute('href')) {
		node.setAttribute('target', '_blank');
		node.setAttribute('rel', 'noopener noreferrer');
	}
	if (node.tagName === 'IMG') {
		node.setAttribute('loading', 'lazy');
		node.setAttribute('decoding', 'async');
		node.setAttribute('referrerpolicy', 'no-referrer');
	}
});

const TAGS = [
	'a',
	'b',
	'blockquote',
	'br',
	'code',
	'div',
	'em',
	'h1',
	'h2',
	'h3',
	'h4',
	'h5',
	'h6',
	'i',
	'img',
	'li',
	'ol',
	'p',
	'pre',
	'span',
	'strong',
	'table',
	'tbody',
	'td',
	'th',
	'thead',
	'tr',
	'u',
	'ul'
];

export function sanitizeHtml(source: string): string {
	return purify.sanitize(source, {
		ALLOWED_TAGS: TAGS,
		ALLOWED_ATTR: ['href', 'src', 'alt', 'title'],
		ALLOWED_URI_REGEXP: /^(?:https?:|magnet:)/i,
		FORBID_TAGS: ['script', 'style', 'iframe', 'form', 'input', 'button'],
		ALLOW_DATA_ATTR: false
	});
}
