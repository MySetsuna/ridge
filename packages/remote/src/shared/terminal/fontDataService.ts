import { invoke } from '@tauri-apps/api/core';

const MAX_FONT_FILES = 32;
const MAX_FONT_BYTES = 96 * 1024 * 1024;
const MAX_SINGLE_FONT_BYTES = 32 * 1024 * 1024;

export type FontDataInstaller = (data: Uint8Array) => boolean | void;

interface NativeFontFace {
	family: string;
	dataBase64: string;
}

export interface LocalFontFace {
	family: string;
	fullName?: string;
	postscriptName?: string;
	style?: string;
	blob(): Promise<Blob>;
}

export interface BrowserFontSource {
	hasTransientActivation(): boolean;
	queryLocalFonts(): Promise<readonly LocalFontFace[]>;
}

type InvokeFn = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

function fontError(code: string, message: string): Error {
	return new Error(`${code}: ${message}`);
}

/** Parse a CSS family stack without splitting commas inside quoted names. */
export function parseCssFontFamilies(stack: string): string[] {
	const result: string[] = [];
	let quote = '';
	let current = '';
	for (const char of stack) {
		if ((char === "'" || char === '"') && (quote === '' || quote === char)) {
			quote = quote === '' ? char : '';
			continue;
		}
		if (char === ',' && quote === '') {
			const family = current.trim();
			if (family) result.push(family);
			current = '';
			continue;
		}
		current += char;
	}
	const family = current.trim();
	if (family) result.push(family);
	return result.filter((name, index) =>
		index === result.findIndex((seen) => seen.toLowerCase() === name.toLowerCase()));
}

export function decodeBase64Font(data: string): Uint8Array {
	if (data.length > Math.ceil(MAX_SINGLE_FONT_BYTES * 4 / 3) + 4) {
		throw fontError('FONT_DATA_LIMIT', 'encoded font face exceeds the 32 MiB limit');
	}
	let decoded: string;
	try {
		decoded = atob(data);
	} catch {
		throw fontError('FONT_DATA_INVALID', 'native font service returned invalid base64');
	}
	const bytes = new Uint8Array(decoded.length);
	for (let index = 0; index < decoded.length; index += 1) {
		bytes[index] = decoded.charCodeAt(index);
	}
	return bytes;
}

function installBounded(
	faces: Iterable<Uint8Array>,
	install: FontDataInstaller,
): number {
	const payloads = [...faces];
	if (payloads.length === 0) {
		throw fontError('FONT_DATA_MISSING', 'selected font stack has no readable local face');
	}
	let total = 0;
	for (const data of payloads) {
		if (data.byteLength === 0 || data.byteLength > MAX_SINGLE_FONT_BYTES) {
			throw fontError('FONT_DATA_LIMIT', 'font face exceeds the 32 MiB limit');
		}
		total += data.byteLength;
		if (payloads.length > MAX_FONT_FILES || total > MAX_FONT_BYTES) {
			throw fontError('FONT_DATA_LIMIT', 'selected font stack exceeds the rasterizer limit');
		}
	}
	for (const data of payloads) install(data);
	return payloads.length;
}

export async function loadNativeTerminalFonts(
	stack: string,
	install: FontDataInstaller,
	invokeCommand: InvokeFn = invoke,
): Promise<number> {
	const families = parseCssFontFamilies(stack);
	const faces = await invokeCommand<NativeFontFace[]>('load_terminal_font_faces', { families });
	return installBounded(faces.map((face) => decodeBase64Font(face.dataBase64)), install);
}

function defaultBrowserFontSource(): BrowserFontSource | null {
	if (typeof window === 'undefined' || typeof navigator === 'undefined') return null;
	const fontWindow = window as typeof window & {
		queryLocalFonts?: () => Promise<readonly LocalFontFace[]>;
	};
	if (typeof fontWindow.queryLocalFonts !== 'function') return null;
	return {
		hasTransientActivation: () => navigator.userActivation?.isActive === true,
		queryLocalFonts: () => fontWindow.queryLocalFonts!(),
	};
}

export async function loadBrowserTerminalFonts(
	stack: string,
	install: FontDataInstaller,
	source: BrowserFontSource | null = defaultBrowserFontSource(),
): Promise<number> {
	if (!source) {
		throw fontError('FONT_ACCESS_UNSUPPORTED', 'browser cannot expose installed system fonts');
	}
	if (!source.hasTransientActivation()) {
		throw fontError('FONT_ACCESS_REQUIRED', 'click Enable local fonts to authorize terminal text');
	}

	// queryLocalFonts must be called while the click's transient activation is
	// still live, so call it before the first await in this function.
	const pendingFaces = source.queryLocalFonts();
	const requested = parseCssFontFamilies(stack).map((family) => family.toLowerCase());
	const order = new Map(requested.map((family, index) => [family, index]));
	const faces = [...await pendingFaces]
		.filter((face) => order.has(face.family.trim().toLowerCase()))
		.sort((left, right) =>
			(order.get(left.family.trim().toLowerCase()) ?? Number.MAX_SAFE_INTEGER)
			- (order.get(right.family.trim().toLowerCase()) ?? Number.MAX_SAFE_INTEGER));

	const seen = new Set<string>();
	const payloads: Uint8Array[] = [];
	let totalBytes = 0;
	for (const face of faces) {
		const identity = face.postscriptName
			?? `${face.family}\0${face.fullName ?? ''}\0${face.style ?? ''}`;
		if (seen.has(identity)) continue;
		seen.add(identity);
		if (payloads.length >= MAX_FONT_FILES) {
			throw fontError('FONT_DATA_LIMIT', 'selected font stack has too many faces');
		}
		const blob = await face.blob();
		totalBytes += blob.size;
		if (blob.size === 0 || blob.size > MAX_SINGLE_FONT_BYTES || totalBytes > MAX_FONT_BYTES) {
			throw fontError('FONT_DATA_LIMIT', 'selected font stack exceeds the rasterizer limit');
		}
		payloads.push(new Uint8Array(await blob.arrayBuffer()));
	}
	return installBounded(payloads, install);
}

export function loadTerminalFonts(
	stack: string,
	install: FontDataInstaller,
): Promise<number> {
	return import.meta.env.RIDGE_WEB_REMOTE === true
		? loadBrowserTerminalFonts(stack, install)
		: loadNativeTerminalFonts(stack, install);
}
