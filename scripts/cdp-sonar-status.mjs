#!/usr/bin/env node
import http from 'node:http';

const port = Number(process.env.SONAR_CDP_PORT ?? 9222);
const project = process.env.SONAR_PROJECT_KEY ?? 'MySetsuna_ridge';

const json = (path) =>
  new Promise((resolve, reject) => {
    http.get({ host: '127.0.0.1', port, path }, (response) => {
      let body = '';
      response.on('data', (chunk) => (body += chunk));
      response.on('end', () => {
        try {
          resolve(JSON.parse(body));
        } catch (error) {
          reject(error);
        }
      });
    }).on('error', reject);
  });

const targets = await json('/json/list');
const target = targets.find(
  (item) => item.type === 'page' && item.url.includes('127.0.0.1:9000'),
);
if (!target) throw new Error('Sonar page target missing');

const socket = new WebSocket(target.webSocketDebuggerUrl);
let nextId = 0;
const pending = new Map();
socket.onmessage = (event) => {
  const message = JSON.parse(event.data);
  const resolve = pending.get(message.id);
  if (!resolve) return;
  pending.delete(message.id);
  resolve(message);
};
await new Promise((resolve, reject) => {
  socket.onopen = resolve;
  socket.onerror = reject;
});
const send = (method, params = {}) =>
  new Promise((resolve) => {
    const id = ++nextId;
    pending.set(id, resolve);
    socket.send(JSON.stringify({ id, method, params }));
  });
const expression = `(async () => {
  const response = await fetch('/api/qualitygates/project_status?projectKey=${project}', { credentials: 'include' });
  return { http: response.status, body: (await response.text()).slice(0, 12000), title: document.title, url: location.href };
})()`;
const result = await send('Runtime.evaluate', {
  expression,
  returnByValue: true,
  awaitPromise: true,
});
console.log(JSON.stringify(result.result?.result?.value ?? result.result?.exceptionDetails));
socket.close();
