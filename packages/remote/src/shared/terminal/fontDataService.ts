import { invoke } from '@tauri-apps/api/core';

const MAX_FONT_FILES = 32;
const MAX_FONT_BYTES = 96 * 1024 * 1024;
const MAX_SINGLE_FONT_BYTES = 32 * 1024 * 1024;
const FONT_CHUNK_BYTES = 2 * 1024 * 1024;
const FONT_CHUNK_CONCURRENCY = 2;
const FONT_CACHE_NAME = 'ridge-terminal-font-v1';
const FONT_CACHE_ORIGIN = 'https://ridge-font-cache.invalid';

export type FontDataInstaller = (data: Uint8Array) => boolean | void;

interface HostFontFace {
	family: string;
	contentHash: string;
	byteLen: number;
	dataBase64?: string;
}

interface HostFontResponse {
	stackHash: string;
	faces: HostFontFace[];
}

interface HostFontChunk {
	contentHash: string;
	offset: number;
	byteLen: number;
	dataBase64: string;
	eof: boolean;
}

interface CachedFontManifest {
	stackHash: string;
	faces: Array<Omit<HostFontFace, 'dataBase64'>>;
}

type InvokeFn = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

function fontError(code: string, message: string): Error {
	return new Error(`${code}: ${message}`);
}

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function parseHostFontResponse(value: unknown): HostFontResponse {
	if (
		!isRecord(value)
		|| typeof value.stackHash !== 'string'
		|| !/^[0-9a-f]{64}$/i.test(value.stackHash)
		|| !Array.isArray(value.faces)
	) {
		throw fontError('FONT_DATA_INVALID', 'Host font manifest has an unsupported shape');
	}
	if (value.faces.length === 0) {
		throw fontError('FONT_DATA_MISSING', 'selected font stack has no readable Host face');
	}
	if (value.faces.length > MAX_FONT_FILES) {
		throw fontError('FONT_DATA_LIMIT', 'Host font manifest exceeds the 32-face limit');
	}
	let total = 0;
	const faces = value.faces.map((face): HostFontFace => {
		if (
			!isRecord(face)
			|| typeof face.family !== 'string'
			|| face.family.length === 0
			|| face.family.length > 256
			|| typeof face.contentHash !== 'string'
			|| !/^[0-9a-f]{64}$/i.test(face.contentHash)
			|| !Number.isSafeInteger(face.byteLen)
			|| (face.byteLen as number) <= 0
			|| (face.byteLen as number) > MAX_SINGLE_FONT_BYTES
			|| (face.dataBase64 !== undefined && typeof face.dataBase64 !== 'string')
		) {
			throw fontError('FONT_DATA_INVALID', 'Host font manifest contains an invalid face');
		}
		total += face.byteLen as number;
		if (total > MAX_FONT_BYTES) {
			throw fontError('FONT_DATA_LIMIT', 'Host font manifest exceeds the 96 MiB limit');
		}
		return {
			family: face.family,
			contentHash: face.contentHash.toLowerCase(),
			byteLen: face.byteLen as number,
			...(face.dataBase64 === undefined ? {} : { dataBase64: face.dataBase64 }),
		};
	});
	return { stackHash: value.stackHash.toLowerCase(), faces };
}

function parseHostFontChunk(value: unknown): HostFontChunk {
	if (
		!isRecord(value)
		|| typeof value.contentHash !== 'string'
		|| !/^[0-9a-f]{64}$/i.test(value.contentHash)
		|| !Number.isSafeInteger(value.offset)
		|| (value.offset as number) < 0
		|| !Number.isSafeInteger(value.byteLen)
		|| (value.byteLen as number) <= 0
		|| (value.byteLen as number) > FONT_CHUNK_BYTES
		|| typeof value.dataBase64 !== 'string'
		|| typeof value.eof !== 'boolean'
	) {
		throw fontError('FONT_DATA_INVALID', 'Host font service returned an invalid chunk');
	}
	return value as unknown as HostFontChunk;
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
		throw fontError('FONT_DATA_INVALID', 'host font service returned invalid base64');
	}
	const bytes = new Uint8Array(decoded.length);
	for (let index = 0; index < decoded.length; index += 1) bytes[index] = decoded.charCodeAt(index);
	return bytes;
}

function installBounded(faces: Iterable<Uint8Array>, install: FontDataInstaller): number {
	const payloads = [...faces];
	if (payloads.length === 0) {
		throw fontError('FONT_DATA_MISSING', 'selected font stack has no readable Host face');
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

function cacheRequest(kind: 'stack' | 'face', key: string): Request {
	return new Request(`${FONT_CACHE_ORIGIN}/${kind}/${encodeURIComponent(key)}`);
}

async function openFontCache(): Promise<Cache | null> {
	if (typeof caches === 'undefined') return null;
	try {
		return await caches.open(FONT_CACHE_NAME);
	} catch {
		return null;
	}
}

async function readManifest(cache: Cache | null, families: string[]): Promise<CachedFontManifest | null> {
	if (!cache) return null;
	try {
		const response = await cache.match(cacheRequest('stack', families.join('\0').toLowerCase()));
		return response ? parseHostFontResponse(await response.json()) : null;
	} catch {
		return null;
	}
}

async function readCachedFaces(
	cache: Cache | null,
	manifest: CachedFontManifest | null,
): Promise<Map<string, Uint8Array>> {
	const result = new Map<string, Uint8Array>();
	if (!cache || !manifest || manifest.faces.length > MAX_FONT_FILES) return result;
	for (const face of manifest.faces) {
		if (face.byteLen <= 0 || face.byteLen > MAX_SINGLE_FONT_BYTES) continue;
		const response = await cache.match(cacheRequest('face', face.contentHash));
		if (!response) continue;
		const data = new Uint8Array(await response.arrayBuffer());
		if (data.byteLength === face.byteLen) result.set(face.contentHash, data);
	}
	return result;
}

async function cacheHostFonts(
	cache: Cache | null,
	families: string[],
	response: HostFontResponse,
	faces: Map<string, Uint8Array>,
): Promise<void> {
	if (!cache) return;
	try {
		await Promise.all(response.faces.map((face) => {
			const data = faces.get(face.contentHash);
			return data
				? cache.put(cacheRequest('face', face.contentHash), new Response(data.slice()))
				: Promise.resolve();
		}));
		const manifest: CachedFontManifest = {
			stackHash: response.stackHash,
			faces: response.faces.map(({ family, contentHash, byteLen }) => ({ family, contentHash, byteLen })),
		};
		await cache.put(
			cacheRequest('stack', families.join('\0').toLowerCase()),
			new Response(JSON.stringify(manifest), { headers: { 'content-type': 'application/json' } }),
		);
	} catch {
		// Cache failure only costs another Host transfer on the next load.
	}
}

async function readHostFace(
	face: HostFontFace,
	invokeCommand: InvokeFn,
): Promise<Uint8Array> {
	if (face.byteLen <= 0 || face.byteLen > MAX_SINGLE_FONT_BYTES) {
		throw fontError('FONT_DATA_LIMIT', `Host face exceeds the limit for ${face.family}`);
	}
	const data = new Uint8Array(face.byteLen);
	let offset = 0;
	while (offset < data.byteLength) {
		const chunk = parseHostFontChunk(await invokeCommand<unknown>('read_terminal_font_face_chunk', {
			contentHash: face.contentHash,
			offset,
			length: Math.min(FONT_CHUNK_BYTES, data.byteLength - offset),
		}));
		const bytes = decodeBase64Font(chunk.dataBase64);
		if (
			chunk.contentHash !== face.contentHash
			|| chunk.offset !== offset
			|| chunk.byteLen !== bytes.byteLength
			|| bytes.byteLength === 0
			|| offset + bytes.byteLength > data.byteLength
		) {
			throw fontError('FONT_DATA_INVALID', `invalid Host chunk for ${face.family}`);
		}
		data.set(bytes, offset);
		offset += bytes.byteLength;
		if (chunk.eof !== (offset === data.byteLength)) {
			throw fontError('FONT_DATA_INVALID', `unexpected Host chunk boundary for ${face.family}`);
		}
	}
	return data;
}

async function fillMissingFaces(
	response: HostFontResponse,
	payloads: Map<string, Uint8Array>,
	invokeCommand: InvokeFn,
): Promise<void> {
	const missing = response.faces.filter((face) => !payloads.has(face.contentHash));
	let next = 0;
	const worker = async () => {
		for (;;) {
			const index = next;
			next += 1;
			const face = missing[index];
			if (!face) return;
			payloads.set(face.contentHash, await readHostFace(face, invokeCommand));
		}
	};
	await Promise.all(Array.from(
		{ length: Math.min(FONT_CHUNK_CONCURRENCY, missing.length) },
		worker,
	));
}

export async function loadHostTerminalFonts(
	stack: string,
	install: FontDataInstaller,
	invokeCommand: InvokeFn = invoke,
): Promise<number> {
	const families = parseCssFontFamilies(stack);
	const cache = await openFontCache();
	const cached = await readCachedFaces(cache, await readManifest(cache, families));
	const response = parseHostFontResponse(await invokeCommand<unknown>('load_terminal_font_faces', {
		families,
		knownHashes: [...cached.keys()],
	}));
	let payloads = new Map(cached);
	for (const face of response.faces) {
		if (face.dataBase64 !== undefined) payloads.set(face.contentHash, decodeBase64Font(face.dataBase64));
	}
	await fillMissingFaces(response, payloads, invokeCommand);
	const ordered = response.faces.map((face) => {
		const data = payloads.get(face.contentHash);
		if (!data || data.byteLength !== face.byteLen) {
			throw fontError('FONT_DATA_INVALID', `Host face length mismatch for ${face.family}`);
		}
		return data;
	});
	const count = installBounded(ordered, install);
	void cacheHostFonts(cache, families, response, payloads);
	return count;
}

/** Desktop and Remote both resolve the Host's installed fonts through invoke RPC. */
export function loadTerminalFonts(stack: string, install: FontDataInstaller): Promise<number> {
	return loadHostTerminalFonts(stack, install);
}
