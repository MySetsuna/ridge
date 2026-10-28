// Let Vite resolve the same Svelte runtime used by the compiled component.
import '../src/app.css';
export { mount, unmount } from 'svelte';
export { createClassComponent } from 'svelte/legacy';
export { QueryClient } from '@tanstack/svelte-query';
