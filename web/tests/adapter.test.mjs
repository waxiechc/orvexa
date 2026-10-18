import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseNodeJson } from '../src/lossless.ts';

test('node integers retain exact u64 precision and uniform string types', () => {
  const parsed = parseNodeJson('{"protocol":1,"zero":0,"height":18446744073709551615,"nonce":9007199254740993,"address":"void_test_example","empty":null}');
  assert.equal(parsed.protocol, '1');
  assert.equal(parsed.zero, '0');
  assert.equal(parsed.height, '18446744073709551615');
  assert.equal(parsed.nonce, '9007199254740993');
  assert.equal(parsed.address, 'void_test_example');
  assert.equal(parsed.empty, null);
});
test('prototype-bearing node payloads and fractional numbers are rejected', () => {
  assert.throws(() => parseNodeJson('{"__proto__":{"polluted":true}}'));
  assert.throws(() => parseNodeJson('{"constructor":{"prototype":{}}}'));
  assert.throws(() => parseNodeJson('{"amount":1.5}'));
  assert.equal({}.polluted, undefined);
});
