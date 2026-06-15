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
    expect(isInsecureCloudDomain('9527127.xyz')).toBe(false);
    expect(isInsecureCloudDomain('mylaptop-alice.9527127.xyz')).toBe(false);
    expect(isInsecureCloudDomain('example.com')).toBe(false);
    // 非回环 IP 与「localhost 仅作为子串」不应误判为回环。
    expect(isInsecureCloudDomain('192.168.0.10:5050')).toBe(false);
    expect(isInsecureCloudDomain('notlocalhost.example.com')).toBe(false);
    expect(isInsecureCloudDomain('localhost.evil.com')).toBe(false);
  });
});

describe('cloudHttpScheme / cloudWsScheme', () => {
  // 前提：RIDGE_CLOUD_DEV_PLAINTEXT 未注入（或注入空串）时 DEV_PLAINTEXT=false，下方“默认”用例据此走 TLS。
  it('returns TLS schemes for loopback bases by default (dev TLS)', () => {
    expect(cloudHttpScheme('localhost:5050')).toBe('https');
    expect(cloudWsScheme('localhost:5050')).toBe('wss');
    expect(cloudWsScheme('mylaptop-alice.localhost:5050')).toBe('wss');
  });

  it('returns TLS schemes for public bases', () => {
    expect(cloudHttpScheme('9527127.xyz')).toBe('https');
    expect(cloudWsScheme('9527127.xyz')).toBe('wss');
    expect(cloudWsScheme('mylaptop-alice.9527127.xyz')).toBe('wss');
  });

  it('downgrades loopback bases to plaintext when plaintext flag set (escape hatch)', () => {
    expect(cloudHttpScheme('localhost:5050', true)).toBe('http');
    expect(cloudWsScheme('localhost:5050', true)).toBe('ws');
  });

  it('keeps public bases on TLS even with plaintext flag (never downgrade prod)', () => {
    expect(cloudHttpScheme('9527127.xyz', true)).toBe('https');
    expect(cloudWsScheme('9527127.xyz', true)).toBe('wss');
  });
});
