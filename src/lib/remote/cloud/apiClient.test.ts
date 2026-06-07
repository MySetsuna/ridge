import { describe, it, expect } from 'vitest';
import { isInsecureCloudDomain, cloudHttpScheme, cloudWsScheme } from './apiClient';

// 这些是 cloud 连接 scheme 选择的依据：base 域指向本机回环时走明文 http/ws
// （本地自托管 ridge-cloud 无 TLS 反代），真实公网域名恒走 https/wss。

describe('isInsecureCloudDomain', () => {
  it('treats localhost (with/without port) as insecure', () => {
    expect(isInsecureCloudDomain('localhost')).toBe(true);
    expect(isInsecureCloudDomain('localhost:5050')).toBe(true);
    expect(isInsecureCloudDomain('LOCALHOST:443')).toBe(true);
  });

  it('treats *.localhost (tenant subdomain) as insecure', () => {
    // {device}-{username}.localhost 在 Chromium/WebView2 自动解析到 127.0.0.1。
    expect(isInsecureCloudDomain('mylaptop-alice.localhost')).toBe(true);
    expect(isInsecureCloudDomain('mylaptop-alice.localhost:5050')).toBe(true);
  });

  it('treats loopback IPs as insecure', () => {
    expect(isInsecureCloudDomain('127.0.0.1')).toBe(true);
    expect(isInsecureCloudDomain('127.0.0.1:5050')).toBe(true);
    expect(isInsecureCloudDomain('127.1.2.3')).toBe(true);
    expect(isInsecureCloudDomain('0.0.0.0')).toBe(true);
    expect(isInsecureCloudDomain('::1')).toBe(true);
    expect(isInsecureCloudDomain('[::1]')).toBe(true);
  });

  it('treats real public domains as secure', () => {
    expect(isInsecureCloudDomain('remo2ridge.duckdns.org')).toBe(false);
    expect(isInsecureCloudDomain('mylaptop-alice.remo2ridge.duckdns.org')).toBe(false);
    expect(isInsecureCloudDomain('9527127.xyz')).toBe(false);
    // 非回环 IP 与「localhost 仅作为子串」不应误判为回环。
    expect(isInsecureCloudDomain('192.168.0.10:5050')).toBe(false);
    expect(isInsecureCloudDomain('notlocalhost.example.com')).toBe(false);
    expect(isInsecureCloudDomain('localhost.evil.com')).toBe(false);
  });
});

describe('cloudHttpScheme / cloudWsScheme', () => {
  it('returns plaintext schemes for loopback bases', () => {
    expect(cloudHttpScheme('localhost:5050')).toBe('http');
    expect(cloudWsScheme('localhost:5050')).toBe('ws');
    expect(cloudWsScheme('mylaptop-alice.localhost:5050')).toBe('ws');
  });

  it('returns TLS schemes for public bases', () => {
    expect(cloudHttpScheme('remo2ridge.duckdns.org')).toBe('https');
    expect(cloudWsScheme('remo2ridge.duckdns.org')).toBe('wss');
    expect(cloudWsScheme('mylaptop-alice.remo2ridge.duckdns.org')).toBe('wss');
  });
});
